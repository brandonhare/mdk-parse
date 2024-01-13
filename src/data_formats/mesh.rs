use crate::data_formats::{image_formats::ColourMap, Texture};
use crate::file_formats::mti::Pen;
use crate::{gltf, OutputWriter, Reader, Vec2, Vec3};

pub struct Mesh<'a> {
	pub materials: Vec<&'a str>,
	pub mesh_data: MeshType<'a>,
	pub reference_points: Vec<Vec3>,
}

pub enum MeshType<'a> {
	Single(MeshGeo),
	Multimesh {
		submeshes: Vec<Submesh<'a>>,
		bbox: [Vec3; 2],
	},
}

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
			if (-256..=-1).contains(&tri.material_index) {
				colours.push((-1 - tri.material_index) as u8);
			}
		}
	}
}

pub struct Submesh<'a> {
	pub mesh_data: MeshGeo,
	pub name: &'a str,
	pub origin: Vec3,
}

const TRIFLAG_HIDDEN: u32 = 0x2;
const TRIFLAG_OUTLINE_12: u32 = 0x10_00_00;
const TRIFLAG_OUTLINE_23: u32 = 0x20_00_00;
const TRIFLAG_OUTLINE_13: u32 = 0x40_00_00;
const TRIFLAG_DRAW_OUTLINE: u32 = 0x80_00_00;
const TRIFLAG_OUTLINE_MASK: u32 = 0xF0_00_00;
const TRIFLAG_ID_MASK: u32 = 0xFF_00_00_00;

pub struct MeshTri {
	pub indices: [u16; 3],
	pub material_index: i16,
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
			let indices: [u16; 3] = reader.try_get()?;
			let material_index: i16 = reader.try_i16()?;
			if material_index > 256 {
				return None;
			}
			let uvs: [[f32; 2]; 3] = reader.try_get_unvalidated()?;
			let flags = reader.try_u32()?;

			result.push(MeshTri {
				indices,
				material_index,
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
				for tri in mesh_data.verts.iter_mut() {
					*tri -= origin;
				}
				submeshes.push(Submesh {
					mesh_data,
					name,
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

		Some(Mesh {
			materials: textures,
			mesh_data,
			reference_points,
		})
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
					let submesh = create_submesh(gltf, sub.name.to_owned(), &sub.mesh_data);
					let sub_node =
						gltf.create_child_node(target, sub.name.to_owned(), Some(submesh));
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
				// filter degenerates
				let degen = match (i1 == i2, i1 == i3, i2 == i3) {
					(false, false, false) => false,
					(true, true, true) => continue,
					(_, _, _) if flags & TRIFLAG_DRAW_OUTLINE == 0 => continue, // no outlines
					// outlines on lines are ok
					(false, _, _) if flags & TRIFLAG_OUTLINE_12 != 0 => true,
					(_, false, _) if flags & TRIFLAG_OUTLINE_13 != 0 => true,
					(_, _, false) if flags & TRIFLAG_OUTLINE_23 != 0 => true,
					_ => continue,
				};

				let [p1, p2, p3] = indices.map(|i| geo.verts[i]);

				let mut tri_mat = Pen::new(tri.material_index as i32);

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

				if degen {
					continue;
				}

				// try textured
				if let Pen::Texture(texture_index) = tri_mat {
					let texture_index = texture_index as usize;
					let mat = &mut materials[texture_index];
					match &mat.0 {
						TextureResult::None => tri_mat = Pen::Colour(0xFF), // todo check in-game missing texture
						TextureResult::Pen(pen) => tri_mat = *pen,
						TextureResult::SaveRef { width, height, .. }
						| TextureResult::SaveEmbed(Texture { width, height, .. }) => {
							let prim = &mut prims[texture_index];
							if prim.material.is_none() {
								// init prim
								if mat.1.is_none() {
									// create material
									let material_name = self.materials[texture_index].to_owned();
									match &mat.0 {
										TextureResult::SaveRef { path, .. } => {
											mat.1 = Some(gltf.create_texture_material_ref(
												material_name,
												path.clone(),
											));
										}
										TextureResult::SaveEmbed(tex) => {
											mat.1 = Some(gltf.create_texture_material_embedded(
												material_name,
												&tex.create_png(Some(palette)),
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
					let submesh = create_submesh(gltf, sub.name.to_owned(), &sub.mesh_data);
					let sub_node =
						gltf.create_child_node(target, sub.name.to_owned(), Some(submesh));
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
	},
	SaveEmbed(Texture<'a>),
}
