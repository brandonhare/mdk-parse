use std::borrow::Cow;
use std::collections::HashMap;

use crate::data_formats::{Pen, Texture, image_formats::ColourMap};
use crate::gltf::AlphaMode;
use crate::{OutputWriter, Reader, Vec2, Vec3, gltf};

#[derive(PartialEq)]
pub struct Mesh<'a> {
	pub materials: Vec<&'a str>,
	pub mesh_data: MeshType<'a>,
	pub reference_points: Vec<Vec3>,
}

#[derive(PartialEq)]
pub enum MeshType<'a> {
	Single(MeshGeo),
	Multimesh {
		submeshes: Vec<Submesh<'a>>,
		bbox: [Vec3; 2],
	},
}

#[derive(Default, PartialEq)]
pub struct MeshGeo {
	pub verts: Vec<Vec3>,
	pub tris: Vec<MeshTri>,
	pub bbox: [Vec3; 2],
}

impl MeshGeo {
	pub fn try_parse(reader: &mut Reader) -> Option<Self> {
		let num_verts = reader.try_u32().filter(|n| *n < 10000)? as usize;
		let verts = Vec3::swizzle_vec(reader.try_get_vec::<Vec3>(num_verts)?);

		let num_tris = reader.try_u32()? as usize;
		let tris = MeshTri::try_parse_slice(reader, num_tris)?;

		assert!(
			tris.iter().all(|tri| tri.flags == 0),
			"found non-bsp mesh with non-zero triangle flags!"
		);

		let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = reader.try_get()?;
		let bbox = [
			Vec3::new(min_x, min_y, min_z).swizzle(),
			Vec3::new(max_x, max_y, max_z).swizzle(),
		];

		Some(MeshGeo { verts, tris, bbox })
	}

	fn get_used_colours(&self, colours: &mut ColourMap) {
		for tri in &self.tris {
			if let Pen::Colour(colour) = tri.material {
				colours.push(colour);
			}
		}
	}

	pub fn split_by_id(mut self) -> MeshType<'static> {
		let mut submeshes: Vec<Submesh> = Vec::new();
		let mut tri_map: HashMap<(u8, u16), u16> = HashMap::new();

		self.tris.retain(|tri| {
			let id = (tri.flags & TRIFLAG_ID_MASK) >> TRIFLAG_ID_SHIFT;
			if id == 0 {
				return true;
			}

			let id_key = id as f32;
			let target = if let Some(sub) = submeshes.iter_mut().find(|sub| sub.origin[0] == id_key)
			{
				&mut sub.mesh_data
			} else {
				submeshes.push(Submesh {
					name: id.to_string().into(),
					origin: Vec3::new(id_key, 0.0, 0.0),
					..Default::default()
				});
				&mut submeshes.last_mut().unwrap().mesh_data
			};

			let mut tri = tri.clone();
			for i in &mut tri.indices {
				*i = *tri_map.entry((id as u8, *i)).or_insert_with(|| {
					let n = target.verts.len();
					target.verts.push(self.verts[*i as usize]);
					n as u16
				});
			}
			target.tris.push(tri);

			false
		});

		if submeshes.is_empty() {
			MeshType::Single(self)
		} else {
			// remove unused verts
			tri_map.clear();
			for tri in &self.tris {
				for i in tri.indices {
					tri_map.insert((0, i), 0);
				}
			}
			let mut i = 0;
			let mut count = 0;
			self.verts.retain(|_| {
				if let Some(v) = tri_map.get_mut(&(0, i)) {
					*v = count;
					count += 1;
					i += 1;
					true
				} else {
					i += 1;
					false
				}
			});
			for tri in &mut self.tris {
				for i in &mut tri.indices {
					*i = *tri_map.get(&(0, *i)).unwrap();
				}
			}

			for sub in &mut submeshes {
				sub.origin = Default::default();
				sub.mesh_data.bbox = Vec3::calculate_bbox(&sub.mesh_data.verts);
			}

			let bbox = self.bbox;
			submeshes.insert(
				0,
				Submesh {
					mesh_data: self,
					name: "Base".into(),
					origin: Default::default(),
				},
			);
			submeshes[1..].sort_unstable_by(|a, b| {
				a.name
					.len()
					.cmp(&b.name.len())
					.then_with(|| a.name.cmp(&b.name))
			});
			MeshType::Multimesh { submeshes, bbox }
		}
	}
}

#[derive(Default, PartialEq)]
pub struct Submesh<'a> {
	pub mesh_data: MeshGeo,
	pub name: Cow<'a, str>,
	pub origin: Vec3,
}

const TRIFLAG_HIDDEN: u32 = 0x12;
const TRIFLAG_OUTLINE_12: u32 = 0x10_00_00;
const TRIFLAG_OUTLINE_23: u32 = 0x20_00_00;
const TRIFLAG_OUTLINE_13: u32 = 0x40_00_00;
const TRIFLAG_OUTLINE_MASK_LINES: u32 = 0x70_00_00;
const TRIFLAG_DRAW_OUTLINE: u32 = 0x80_00_00;
const TRIFLAG_OUTLINE_MASK: u32 = 0xF0_00_00;
const TRIFLAG_ID_MASK: u32 = 0xFF_00_00_00;
const TRIFLAG_ID_SHIFT: u32 = 24;

#[derive(Clone, PartialEq)]
pub struct MeshTri {
	pub indices: [u16; 3],
	pub material: Pen,
	pub uvs: [Vec2; 3],
	pub flags: u32, // bsp id and flags, 0 for normal meshes
}
impl MeshTri {
	pub fn try_parse_slice(reader: &mut Reader, count: usize) -> Option<Vec<Self>> {
		if count > 10000 {
			return None;
		}
		let mut result = Vec::with_capacity(count);
		for _ in 0..count {
			let indices @ [i1, i2, i3]: [u16; 3] = reader.try_get()?;
			let material_index: i16 = reader.try_i16()?;
			if material_index > 256 {
				return None;
			}
			let material = Pen::new(material_index as i32);
			let uvs: [[f32; 2]; 3] = reader.try_get_unvalidated()?;
			let mut flags = reader.try_u32()?;

			// remove invalid outline flags
			if flags & TRIFLAG_DRAW_OUTLINE == 0 {
				flags &= !TRIFLAG_OUTLINE_MASK; // clear unused flags
			} else {
				// remove degenerate lines
				if i1 == i2 {
					flags &= !TRIFLAG_OUTLINE_12;
				}
				if i1 == i3 {
					flags &= !TRIFLAG_OUTLINE_13;
				}
				if i2 == i3 {
					flags &= !TRIFLAG_OUTLINE_23;
				}
				// none left
				if flags & TRIFLAG_OUTLINE_MASK_LINES == 0 {
					flags &= !TRIFLAG_OUTLINE_MASK; // clear main bit
				}
			}

			// skip degenerate tris
			if (i1 == i2 || i1 == i3 || i2 == i3) && (flags & TRIFLAG_OUTLINE_MASK == 0) {
				continue;
			}

			result.push(MeshTri {
				indices,
				material,
				uvs,
				flags,
			});
		}
		Some(result)
	}

	pub fn id(&self) -> u8 {
		(self.flags >> 24) as u8
	}
}

impl<'a> Mesh<'a> {
	pub fn parse(reader: &mut Reader<'a>, is_multimesh: bool) -> Mesh<'a> {
		Self::try_parse(reader, is_multimesh).expect("failed to read mesh")
	}
	pub fn try_parse(reader: &mut Reader<'a>, is_multimesh: bool) -> Option<Mesh<'a>> {
		let num_textures = reader.try_u32()? as usize;
		if num_textures > 500 {
			return None;
		};
		let mut textures = Vec::with_capacity(num_textures);
		for _ in 0..num_textures {
			textures.push(reader.try_str(16)?);
		}

		let mesh_data = if !is_multimesh {
			MeshType::Single(MeshGeo::try_parse(reader)?)
		} else {
			let num_submeshes = reader.try_u32().filter(|n| *n < 100)? as usize;
			let mut submeshes = Vec::with_capacity(num_submeshes);

			for _ in 0..num_submeshes {
				let name = reader.try_str(12)?;
				let origin = reader.try_vec3()?.swizzle();
				let mut mesh_data = MeshGeo::try_parse(reader)?;
				for point in mesh_data.verts.iter_mut() {
					*point -= origin;
				}
				submeshes.push(Submesh {
					mesh_data,
					name: name.into(),
					origin,
				});
			}

			let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = reader.try_get()?;
			let bbox = [
				Vec3::new(min_x, min_y, min_z).swizzle(),
				Vec3::new(max_x, max_y, max_z).swizzle(),
			];

			MeshType::Multimesh { submeshes, bbox }
		};

		let num_reference_points = reader.try_u32()?;
		let reference_points =
			Vec3::swizzle_vec(reader.try_get_vec(num_reference_points as usize)?);

		let mut result = Mesh {
			materials: textures,
			mesh_data,
			reference_points,
		};

		result.remove_unused_materials();

		Some(result)
	}

	pub fn for_tris_mut(&mut self, mut func: impl FnMut(&mut [MeshTri])) {
		match &mut self.mesh_data {
			MeshType::Single(geo) => func(&mut geo.tris),
			MeshType::Multimesh { submeshes, .. } => {
				for mesh in submeshes {
					func(&mut mesh.mesh_data.tris);
				}
			}
		}
	}

	pub fn remove_unused_materials(&mut self) {
		let mut used = vec![0; self.materials.len()];

		self.for_tris_mut(|tris| {
			for tri in tris {
				if let Pen::Texture(index) = tri.material {
					used[index as usize] = 1;
				}
			}
		});

		let mut i = 0;
		let mut count = 0;
		self.materials.retain(|_| {
			if used[i] != 0 {
				used[i] = count;
				count += 1;
				i += 1;
				true
			} else {
				i += 1;
				false
			}
		});

		self.for_tris_mut(|tris| {
			for tri in tris {
				if let Pen::Texture(index) = &mut tri.material {
					*index = used[*index as usize];
				}
			}
		});
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		let mut gltf = gltf::Gltf::new(name.to_owned());

		let root = gltf.get_root_node();
		self.add_to_gltf(&mut gltf, name, Some(root));

		output.write(name, "gltf", gltf.render_json().as_bytes());
	}
	pub fn save_textured_as(
		&self, name: &str, output: &mut OutputWriter, textures: &mut impl TextureHolder<'a>,
	) {
		let mut gltf = gltf::Gltf::new(name.to_owned());

		let root = gltf.get_root_node();
		self.add_to_gltf_textured(&mut gltf, name, Some(root), textures);

		output.write(name, "gltf", gltf.render_json().as_bytes());
	}

	pub fn add_to_gltf(
		&self, gltf: &mut gltf::Gltf, name: &str, target: Option<gltf::NodeIndex>,
	) -> gltf::NodeIndex {
		let target = target.unwrap_or_else(|| gltf.create_node(name.to_owned(), None));

		let create_submesh =
			|gltf: &mut gltf::Gltf, name: String, geo: &MeshGeo| -> gltf::MeshIndex {
				let indices: Vec<_> = geo
					.tris
					.iter()
					.flat_map(|tri| {
						let [i1, i2, i3] = tri.indices;
						[i1, i3, i2]
					})
					.collect();
				gltf.create_mesh_from_primitive(name, &geo.verts, &indices, None, None)
			};

		match &self.mesh_data {
			MeshType::Single(geo) => {
				let submesh = create_submesh(gltf, name.to_owned(), geo);
				gltf.set_node_mesh(target, submesh);
			}
			MeshType::Multimesh { submeshes, .. } => {
				for sub in submeshes {
					let submesh = create_submesh(gltf, sub.name.to_string(), &sub.mesh_data);
					let sub_node =
						gltf.create_child_node(target, sub.name.to_string(), Some(submesh));
					gltf.set_node_position(sub_node, sub.origin);
				}
			}
		}

		if !self.reference_points.is_empty() {
			gltf.create_points_nodes(
				"Reference Points".into(),
				&self.reference_points,
				Some(target),
			);
		}

		target
	}

	pub fn add_to_gltf_textured(
		&self, gltf: &mut gltf::Gltf, name: &str, target: Option<gltf::NodeIndex>,
		textures: &mut impl TextureHolder<'a>,
	) -> gltf::NodeIndex {
		let mut materials: Vec<(TextureResult, Option<gltf::MaterialIndex>)> = self
			.materials
			.iter()
			.map(|mat| (textures.lookup(mat), None))
			.collect();

		let palette = textures.get_palette();
		let mut translucent_colours: Option<[[u8; 4]; 4]> = None;

		let mut colour_mat: Option<gltf::MaterialIndex> = None;
		let mut translucent_mat: Option<gltf::MaterialIndex> = None;
		let mut shiny_mat: Option<gltf::MaterialIndex> = None;

		#[derive(Default)]
		struct MeshPrimitive {
			verts: Vec<Vec3>,
			indices: Vec<u16>,
			uvs: Vec<Vec2>,
			colours: Vec<[u8; 4]>,
			material: Option<gltf::MaterialIndex>,
			uv_scale: Vec2,
		}
		impl MeshPrimitive {
			fn clear(&mut self) {
				self.verts.clear();
				self.indices.clear();
				self.uvs.clear();
				self.colours.clear();
				self.material = None;
				self.uv_scale = [1.0; 2];
			}
		}

		let mut prims = Vec::<MeshPrimitive>::new();
		prims.resize_with(materials.len(), Default::default);
		let mut colour_prim = MeshPrimitive::default();
		let mut translucent_prim = MeshPrimitive::default();
		let mut lines_prim = MeshPrimitive::default();
		let mut shiny_prim = MeshPrimitive::default();

		let mut create_submesh = |gltf: &mut gltf::Gltf,
		                          name: String,
		                          geo: &MeshGeo|
		 -> gltf::MeshIndex {
			for prim in &mut prims {
				prim.clear()
			}
			colour_prim.clear();
			translucent_prim.clear();
			lines_prim.clear();
			shiny_prim.clear();

			for tri in &geo.tris {
				let indices @ [i1, i2, i3] = tri.indices.map(|n| n as usize);

				let flags = tri.flags;

				if flags & TRIFLAG_HIDDEN != 0 {
					continue;
				}

				let [p1, p2, p3] = indices.map(|i| geo.verts[i]);

				let mut tri_mat = tri.material;

				// outlines
				if flags & TRIFLAG_OUTLINE_MASK > TRIFLAG_DRAW_OUTLINE {
					// if outline flag and at least one side is set

					if lines_prim.material.is_none() {
						if translucent_mat.is_none() {
							translucent_mat =
								Some(gltf.create_translucent_material("Translucent".to_owned()));
						}
						lines_prim.material = translucent_mat;
					}

					let colour: [u8; 4] = if let Pen::Translucent(index) = tri_mat {
						translucent_colours
							.get_or_insert_with(|| textures.get_translucent_colours())[index as usize]
					} else {
						// fallback (unused)
						//eprintln!("unexpected material {tri_mat:?} on mesh {name} outline");
						let index = if let Pen::Colour(index) = tri_mat {
							index as usize
						} else {
							1
						};
						let [r, g, b] = palette[index * 3..index * 3 + 3].try_into().unwrap();
						[r, g, b, 255]
					};

					let i1 = lines_prim.verts.len() as u16;
					if flags & (TRIFLAG_OUTLINE_12 | TRIFLAG_OUTLINE_13) != 0 {
						lines_prim.verts.push(p1);
						lines_prim.colours.push(colour);
					}
					let i2 = lines_prim.verts.len() as u16;
					if flags & (TRIFLAG_OUTLINE_12 | TRIFLAG_OUTLINE_23) != 0 {
						lines_prim.verts.push(p2);
						lines_prim.colours.push(colour);
					}
					let i3 = lines_prim.verts.len() as u16;
					if flags & (TRIFLAG_OUTLINE_13 | TRIFLAG_OUTLINE_23) != 0 {
						lines_prim.verts.push(p3);
						lines_prim.colours.push(colour);
					}
					if flags & TRIFLAG_OUTLINE_12 != 0 {
						lines_prim.indices.extend([i1, i2]);
					}
					if flags & TRIFLAG_OUTLINE_13 != 0 {
						lines_prim.indices.extend([i1, i3]);
					}
					if flags & TRIFLAG_OUTLINE_23 != 0 {
						lines_prim.indices.extend([i2, i3]);
					}
				} // end outlines

				// filter partial degenerates (after outlines)
				if i1 == i2 || i1 == i3 || i2 == i3 {
					continue;
				}

				// try textured
				if let Pen::Texture(texture_index) = tri_mat {
					let texture_index = texture_index as usize;
					let mat = &mut materials[texture_index];
					match &mat.0 {
						TextureResult::None => tri_mat = Pen::Colour(0xFF), // missing textures are white in-game (e.g. ramp to level 2 boss)
						TextureResult::Pen(pen) => tri_mat = *pen,
						TextureResult::SaveRef {
							width,
							height,
							masked,
							..
						}
						| TextureResult::SaveEmbed {
							texture: Texture { width, height, .. },
							masked,
						} => {
							let prim = &mut prims[texture_index];
							if prim.material.is_none() {
								// init prim
								if mat.1.is_none() {
									// create material
									let material_name = self.materials[texture_index].to_owned();
									let alpha_mode = if *masked {
										AlphaMode::Mask
									} else {
										AlphaMode::Opaque
									};
									match &mat.0 {
										TextureResult::SaveRef { path, .. } => {
											mat.1 = Some(gltf.create_texture_material_ref(
												material_name,
												path.clone(),
												Some(alpha_mode),
											));
										}
										TextureResult::SaveEmbed { texture, .. } => {
											mat.1 = Some(gltf.create_texture_material_embedded(
												material_name,
												&texture.create_png(Some(palette)),
												Some(alpha_mode),
											));
										}
										_ => unreachable!(),
									}
								}
								prim.uv_scale = [(*width as f32).recip(), (*height as f32).recip()];
								prim.material = mat.1;
							}

							// add data to textured prim

							let i1 = prim.verts.len() as u16;
							prim.verts.extend([p1, p2, p3]);
							for [u, v] in tri.uvs {
								prim.uvs.push([u * prim.uv_scale[0], v * prim.uv_scale[1]]);
							}
							prim.indices.extend([i1, i1 + 2, i1 + 1]); // swizzle indices

							continue;
						}
					}
				}

				// not textured
				let prim: &mut MeshPrimitive;
				let mut colour: Option<[u8; 4]> = None;
				match tri_mat {
					Pen::Colour(colour_index) => {
						prim = &mut colour_prim;
						if prim.material.is_none() {
							if colour_mat.is_none() {
								colour_mat = Some(
									gltf.create_colour_material("Colour".to_owned(), [1.0; 4]),
								);
							}
							prim.material = colour_mat;
						}
						let colour_index = colour_index as usize;
						let [r, g, b] = palette[colour_index * 3..colour_index * 3 + 3]
							.try_into()
							.unwrap();
						colour = Some([r, g, b, 255]);
					}
					Pen::Shiny(_shiny_index) => {
						// todo use shiny index
						prim = &mut shiny_prim;
						if prim.material.is_none() {
							if shiny_mat.is_none() {
								shiny_mat = Some(gltf.create_shiny_material("Shiny".to_owned()));
							}
							prim.material = shiny_mat;
						}
					}
					Pen::Translucent(translucent_index) => {
						prim = &mut translucent_prim;
						if prim.material.is_none() {
							if translucent_mat.is_none() {
								translucent_mat = Some(
									gltf.create_translucent_material("Translucent".to_owned()),
								);
							}
							prim.material = translucent_mat;
						}
						colour = Some(
							translucent_colours
								.get_or_insert_with(|| textures.get_translucent_colours())
								[translucent_index as usize],
						);
					}
					Pen::Texture(_) => unreachable!(),
					Pen::Unknown(_n) => {
						// todo
						//eprintln!("unknown mesh material {n} in {name}");
						continue;
					}
				};

				let i1 = prim.verts.len() as u16;
				prim.verts.extend([p1, p2, p3]);
				prim.indices.extend([i1, i1 + 2, i1 + 1]); // swizzle indices
				if let Some(colour) = colour {
					prim.colours.extend([colour, colour, colour]);
				}
			}

			// finished populating primitives, create mesh

			let mesh = gltf.create_mesh(name);
			for prim in
				prims
					.iter()
					.chain([&colour_prim, &translucent_prim, &shiny_prim, &lines_prim])
			{
				if prim.material.is_none() {
					continue;
				}
				assert!(!prim.verts.is_empty());

				let prim_id =
					gltf.add_mesh_primitive(mesh, &prim.verts, &prim.indices, prim.material);
				// these are no-ops if unused
				gltf.add_primitive_uvs(prim_id, &prim.uvs);
				gltf.add_primitive_colours(prim_id, &prim.colours);

				if std::ptr::eq(prim, &lines_prim) {
					gltf.set_primitive_mode(prim_id, gltf::PrimitiveMode::Lines);
				}
			}

			mesh
		};

		let target = target.unwrap_or_else(|| gltf.create_node(name.to_owned(), None));
		match &self.mesh_data {
			MeshType::Single(geo) => {
				let submesh = create_submesh(gltf, name.to_owned(), geo);
				gltf.set_node_mesh(target, submesh);
			}
			MeshType::Multimesh { submeshes, .. } => {
				for sub in submeshes {
					let submesh = create_submesh(gltf, sub.name.to_string(), &sub.mesh_data);
					let sub_node =
						gltf.create_child_node(target, sub.name.to_string(), Some(submesh));
					gltf.set_node_position(sub_node, sub.origin);
				}
			}
		}

		if !self.reference_points.is_empty() {
			gltf.create_points_nodes(
				"Reference Points".into(),
				&self.reference_points,
				Some(target),
			);
		}

		target
	}

	pub fn get_used_colours(&self, textures: &impl TextureHolder<'a>) -> ColourMap {
		let mut result = ColourMap::new();
		for mat in &self.materials {
			textures.get_used_colours(mat, &mut result);
		}
		match &self.mesh_data {
			MeshType::Single(geo) => geo.get_used_colours(&mut result),
			MeshType::Multimesh { submeshes, .. } => {
				for mesh in submeshes {
					mesh.mesh_data.get_used_colours(&mut result);
				}
			}
		}
		result
	}
}

pub trait TextureHolder<'a> {
	fn lookup(&mut self, name: &str) -> TextureResult<'a>;
	fn get_used_colours(&self, name: &str, colours: &mut ColourMap);
	fn get_palette(&self) -> &[u8];
	fn get_translucent_colours(&self) -> [[u8; 4]; 4];
}

pub enum TextureResult<'a> {
	None,
	Pen(Pen),
	SaveRef {
		width: u16,
		height: u16,
		path: String,
		masked: bool,
	},
	SaveEmbed {
		texture: Texture<'a>,
		masked: bool,
	},
}
