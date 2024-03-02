use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::{Range, RangeInclusive};

use crate::data_formats::{image_formats::ColourMap, Animation, Texture};
use crate::file_formats::mti::Pen;
use crate::gltf::AlphaMode;
use crate::{gltf, OutputWriter, Reader, Vec2, Vec3};

const TRIFLAG_HIDDEN: u32 = 0x12;
const TRIFLAG_OUTLINE_12: u32 = 0x10_00_00;
const TRIFLAG_OUTLINE_23: u32 = 0x20_00_00;
const TRIFLAG_OUTLINE_13: u32 = 0x40_00_00;
const TRIFLAG_OUTLINE_MASK_LINES: u32 = 0x70_00_00;
const TRIFLAG_DRAW_OUTLINE: u32 = 0x80_00_00;
const TRIFLAG_OUTLINE_MASK: u32 = 0xF0_00_00;
const TRIFLAG_ID_MASK: u32 = 0xFF_00_00_00;
const TRIFLAG_ID_SHIFT: usize = 24;

pub const REFERENCE_POINTS_NAME: &str = "Reference Points";

pub struct Mesh<'a> {
	pub parts: Vec<MeshPart<'a>>,
	pub materials: Vec<&'a str>,
	pub reference_points: Vec<Vec3>,
	pub bbox: [Vec3; 2],
}

#[derive(Default)]
pub struct MeshPart<'a> {
	pub verts: Vec<Vec3>,
	pub tris: Vec<MeshTri>,
	pub bbox: [Vec3; 2],
	pub name: Cow<'a, str>,
	pub origin: Vec3,
}

#[derive(Clone)]
pub struct MeshTri {
	pub indices: [u16; 3],
	pub material: Pen,
	pub uvs: [Vec2; 3],
	pub flags: u32, // bsp id and flags, 0 for normal meshes
}

impl<'a> MeshPart<'a> {
	pub fn try_parse(reader: &mut Reader, name: &'a str) -> Option<Self> {
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

		Some(Self {
			verts,
			tris,
			bbox,
			name: name.into(),
			origin: Vec3::default(),
		})
	}
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
			let material = Pen::new(material_index);
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
	pub fn is_hidden(&self) -> bool {
		self.flags & TRIFLAG_HIDDEN != 0
	}
}

impl<'a> Mesh<'a> {
	pub fn parse(reader: &mut Reader<'a>, name: &'a str, is_multimesh: bool) -> Mesh<'a> {
		Self::try_parse(reader, name, is_multimesh).expect("failed to read mesh")
	}
	pub fn try_parse(
		reader: &mut Reader<'a>, name: &'a str, is_multimesh: bool,
	) -> Option<Mesh<'a>> {
		let num_materials = reader.try_u32().filter(|n| *n <= 500)? as usize;
		let mut materials = Vec::with_capacity(num_materials);
		for _ in 0..num_materials {
			materials.push(reader.try_str(16)?);
		}

		let full_bbox: [Vec3; 2];
		let parts = if !is_multimesh {
			let part = MeshPart::try_parse(reader, name)?;
			full_bbox = part.bbox;
			vec![part]
		} else {
			let num_parts = reader.try_u32().filter(|n| *n < 100)? as usize;
			let mut parts = Vec::with_capacity(num_parts);

			for _ in 0..num_parts {
				let name = reader.try_str(12)?;
				let origin = reader.try_vec3()?.swizzle();
				let mut part = MeshPart::try_parse(reader, name)?;
				// part.origin = origin;
				// for point in part.verts.iter_mut() {
				// 	*point -= origin;
				// }
				parts.push(part);
			}

			let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = reader.try_get()?;
			full_bbox = [
				Vec3::new(min_x, min_y, min_z).swizzle(),
				Vec3::new(max_x, max_y, max_z).swizzle(),
			];

			parts
		};

		let num_reference_points = reader.try_u32().filter(|n| *n < 100)?;
		let reference_points =
			Vec3::swizzle_vec(reader.try_get_vec(num_reference_points as usize)?);

		let mut result = Mesh {
			materials,
			parts,
			reference_points,
			bbox: full_bbox,
		};

		result.remove_unused_materials();

		Some(result)
	}

	pub fn remove_unused_materials(&mut self) {
		let mut remap_list = vec![0u8; self.materials.len()];

		for part in &self.parts {
			for tri in &part.tris {
				if let Pen::Texture(index) = tri.material {
					remap_list[index as usize] = 1;
				}
			}
		}

		let mut i = 0;
		let mut count = 0;
		self.materials.retain(|_| {
			if remap_list[i] != 0 {
				remap_list[i] = count;
				count += 1;
				i += 1;
				true
			} else {
				i += 1;
				false
			}
		});

		if i == count as usize {
			return;
		}

		for part in &mut self.parts {
			for tri in &mut part.tris {
				if let Pen::Texture(index) = &mut tri.material {
					*index = remap_list[*index as usize];
				}
			}
		}
	}

	/*
	pub fn flatten_materials(&mut self, mut is_pen: impl FnMut(&str) -> Pen) {
		let mut remap_list: Vec<Pen> = self
			.materials
			.iter()
			.map(|mat| match is_pen(*mat) {
				Pen::Texture(_) => Pen::Texture(0),
				pen => pen,
			})
			.collect();

		for part in &mut self.parts {
			for tri in &mut part.tris {
				if let Pen::Texture(index) = tri.material {
					match &mut remap_list[index as usize] {
						Pen::Texture(used) => *used = 1,
						pen => tri.material = *pen,
					}
				}
			}
		}

		let mut i = 0;
		let mut remaining_count = 0;
		self.materials.retain(|mat| match &mut remap_list[i] {
			Pen::Texture(n) if *n != 0 => {
				*n = remaining_count;
				remaining_count += 1;
				i += 1;
				true
			}
			_ => {
				i += 1;
				false
			}
		});
		if i == remaining_count as usize {
			return;
		}

		for part in &mut self.parts {
			for tri in &mut part.tris {
				if let Pen::Texture(index) = tri.material {
					tri.material = remap_list[index as usize];
				}
			}
		}
	}
	*/

	pub fn is_anim_compatible(&self, anim: &Animation) -> bool {
		/*if self.reference_points.len() < anim.reference_points.len() {
			return false;
		}*/

		let mut has_any = false;
		for anim_part in &anim.parts {
			if let Some(mesh_part) = self
				.parts
				.iter()
				.find(|mesh_part| mesh_part.name == anim_part.name)
			{
				if anim_part.point_paths.len() != mesh_part.verts.len() {
					return false;
				}
				has_any = true;
			}
		}

		has_any
	}

	/*
	pub fn save_as(&self, name: &str, output: &mut OutputWriter, anims: &[(&str, &Animation)]) {
		let mut gltf = gltf::Gltf::new(name.to_owned());

		let root = gltf.get_root_node();
		self.add_to_gltf(&mut gltf, name, Some(root));

		output.write(name, "gltf", gltf.render_json().as_bytes());
	}
	pub fn save_textured_as(
		&self, name: &str, output: &mut OutputWriter, textures: &mut impl TextureHolder<'a>,
		anims: &[(&str, &Animation)],
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
						TextureResult::None => tri_mat = Pen::Colour(0xFF), // todo check in-game missing texture
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
					gltf.create_mesh_primitive(mesh, &prim.verts, &prim.indices, prim.material);
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

	*/

	pub fn get_used_vertex_colours(&self) -> ColourMap {
		let mut result = ColourMap::new();
		for part in &self.parts {
			for tri in &part.tris {
				if let Pen::Colour(n) = tri.material {
					result.push(n);
				}
			}
		}
		result
	}

	pub fn save_as(
		&self, mesh_name: &str, output: &mut OutputWriter, materials: Option<&Materials>,
		anims: &[(&str, &Animation)],
	) {
		let palette = materials.map(|mat| mat.palette);
		let mut primitives = Primitives::default();
		if let Some(materials) = materials {
			primitives.textured.resize_with(
				materials
					.materials
					.iter()
					.rev()
					.skip_while(|mat| {
						!matches!(
							mat,
							ResolvedMaterial::TextureEmbed { .. }
								| ResolvedMaterial::TextureRef { .. },
						)
					})
					.count(),
				Default::default,
			);
		}

		let mut gltf = gltf::Gltf::new(mesh_name.to_string());

		let mut part_joints: Vec<(&str, Range<gltf::NodeIndex>)> = Vec::new();
		/*let skeleton_nodes = if anims.is_empty() {
			gltf::NodeIndex(0)
		} else {
			gltf.create_root_node(String::from("Skeletons"), None)
		};*/

		for part in &self.parts {
			let mesh = gltf.create_mesh(part.name.to_string());
			let part_node = gltf.create_root_node(part.name.to_string(), Some(mesh));
			//gltf.set_node_position(part_node, part.origin);

			let part_is_animated = anims.iter().any(|(_anim_name, anim)| {
				anim.parts
					.iter()
					.any(|anim_part| anim_part.name == part.name)
			});

			if part_is_animated {
				//let skeleton_node = gltf.create_child_node(skeleton_nodes, format!("{} Skeleton", part.name), None);
				let skeleton_node = part_node;
				let skin = gltf.create_skin(skeleton_node);
				gltf.set_node_skin(part_node, skin);
				let node_range = gltf.create_node_range(
					skeleton_node,
					None,
					part.verts.iter().copied(),
					//(0..part.verts.len()).map(|_| Default::default()),
				);
				gltf.set_skin_joints(skin, node_range.clone());
				part_joints.push((&part.name, node_range));
			}

			if let Some(materials) = materials {
				primitives.clear();
				split_into_primitives(part, &mut primitives, materials, part_is_animated);
			} else {
				primitives.coloured.base.clear();
				flatten_into_primitive(part, &mut primitives.coloured.base, part_is_animated);
			}

			// save textured prims
			for (tex_index, prim) in primitives.textured.iter_mut().enumerate() {
				if prim.base.indices.is_empty() {
					continue;
				}

				let texture_name = self.materials[tex_index];

				let texture = &materials.unwrap().materials[tex_index];
				let (width, height) = match texture {
					ResolvedMaterial::TextureRef { width, height, .. } => (*width, *height),
					ResolvedMaterial::TextureEmbed { texture, .. } => {
						(texture.width, texture.height)
					}
					_ => unreachable!("textured prim should only reference valid texture indices"),
				};
				let width = if width != 0 { 1.0 / width as f32 } else { 1.0 };
				let height = if height != 0 {
					1.0 / height as f32
				} else {
					1.0
				};
				for [u, v] in &mut prim.uvs {
					*u *= width;
					*v *= height;
				}

				let mat_index = *prim.base.material.get_or_insert_with(|| {
					match &materials.as_ref().unwrap().materials[tex_index] {
						ResolvedMaterial::TextureRef { path, masked, .. } => gltf
							.create_texture_material_ref(
								texture_name.to_string(),
								path.to_string(),
								masked.then_some(AlphaMode::Mask),
							),
						ResolvedMaterial::TextureEmbed { texture, masked } => gltf
							.create_texture_material_embedded(
								texture_name.to_string(),
								&texture.create_png(palette),
								masked.then_some(AlphaMode::Mask),
							),
						_ => unreachable!("found invalid texture index after mesh primitive split"),
					}
				});

				let prim_index =
					prim.base
						.add_to_gltf(&mut gltf, texture_name.to_string(), mesh, mat_index);
				gltf.add_primitive_uvs(prim_index, &prim.uvs);

				debug_assert_eq!(
					prim.base.indices.len() % 3,
					0,
					"invalid primitive index count"
				);
			}

			// save coloured prims
			if !primitives.coloured.base.indices.is_empty() {
				let prim = &mut primitives.coloured;
				let name = String::from("Vertex Colours");
				let mat_index = *prim
					.base
					.material
					.get_or_insert_with(|| gltf.create_colour_material(name.clone(), [1.0; 4]));
				let prim_index = prim.base.add_to_gltf(&mut gltf, name, mesh, mat_index);
				gltf.add_primitive_colours(prim_index, &prim.colours);

				debug_assert_eq!(
					prim.base.indices.len() % 3,
					0,
					"invalid primitive index count"
				);
			}
			// save translucent prims
			if !primitives.translucent.base.indices.is_empty() {
				let prim = &mut primitives.translucent;
				let name = String::from("Translucent Colours");
				let mat_index = *prim
					.base
					.material
					.get_or_insert_with(|| gltf.create_translucent_material(name.clone()));
				let prim_index = prim.base.add_to_gltf(&mut gltf, name, mesh, mat_index);
				gltf.add_primitive_colours(prim_index, &prim.colours);

				debug_assert_eq!(
					prim.base.indices.len() % 3,
					0,
					"invalid primitive index count"
				);
			}
			// save outlines
			if !primitives.outlines.base.indices.is_empty() {
				let prim = &mut primitives.outlines;
				let name = String::from("Outlines");
				let mat_index = *prim
					.base
					.material
					.get_or_insert_with(|| gltf.create_translucent_material(name.clone()));
				let prim_index = prim.base.add_to_gltf(&mut gltf, name, mesh, mat_index);
				gltf.add_primitive_colours(prim_index, &prim.colours);
				gltf.set_primitive_mode(prim_index, gltf::PrimitiveMode::Lines);

				debug_assert_eq!(
					prim.base.indices.len() % 2,
					0,
					"invalid textured primitive index count"
				);
			}

			// save shiny prims
			if !primitives.shiny.base.indices.is_empty() {
				let prim = &mut primitives.shiny;
				let name = String::from("Shiny");
				let mat_index = *prim
					.base
					.material
					.get_or_insert_with(|| gltf.create_shiny_material(name.clone()));
				let prim_index = prim.base.add_to_gltf(&mut gltf, name, mesh, mat_index);
				gltf.add_primitive_normals(prim_index, &prim.normals);

				debug_assert_eq!(
					prim.base.indices.len() % 3,
					0,
					"invalid textured primitive index count"
				);
			}

			// save unknown material prims
			if !primitives.unknown.indices.is_empty() {
				let prim = &mut primitives.unknown;
				let name = String::from("Unknown");
				let mat_index = *prim.material.get_or_insert_with(|| {
					println!("Unknown material used in mesh {mesh_name}");
					gltf.create_colour_material(name.clone(), [1.0, 0.0, 1.0, 1.0])
				});
				prim.add_to_gltf(&mut gltf, name, mesh, mat_index);

				debug_assert_eq!(
					prim.indices.len() % 3,
					0,
					"invalid textured primitive index count"
				);
			}
		}

		let mut num_ref_points = self.reference_points.len();
		if num_ref_points != 0 {
			for (_, anim) in anims {
				for part in &anim.parts {
					if part.name == REFERENCE_POINTS_NAME {
						num_ref_points = num_ref_points.max(part.point_paths.len());
						break;
					}
				}
			}

			let ref_point_node = gltf.create_root_node(String::from(REFERENCE_POINTS_NAME), None);
			let cube = gltf.get_cube_mesh();
			let nodes = gltf.create_node_range(
				ref_point_node,
				Some(cube),
				self.reference_points
					.iter()
					.copied()
					.chain(std::iter::repeat(Default::default()))
					.take(num_ref_points),
			);
			if !anims.is_empty() {
				part_joints.push((REFERENCE_POINTS_NAME, nodes.clone()));
			}
		}

		// save animations
		let mut anim_cache = Vec::new();
		for &(anim_name, anim) in anims {
			anim.add_to_gltf(&mut gltf, anim_name, &part_joints, &mut anim_cache);
		}
		for (_, Range { start, end }) in &part_joints {
			for index in start.0..end.0 {
				gltf.set_node_position(gltf::NodeIndex(index), Default::default());
			}
		}

		output.write(mesh_name, "gltf", gltf.render_json().as_bytes());
	}
}

#[derive(Default)]
struct MeshPrimitive {
	material: Option<gltf::MaterialIndex>,
	verts: Vec<Vec3>,
	indices: Vec<u16>,
	anim_indices: Vec<u16>,
}
#[derive(Default)]
struct TexturedPrimitive {
	base: MeshPrimitive,
	uvs: Vec<Vec2>,
}
#[derive(Default)]
struct ColourPrimitive {
	base: MeshPrimitive,
	colours: Vec<[u8; 4]>,
}
#[derive(Default)]
struct ShinyPrimitive {
	base: MeshPrimitive,
	normals: Vec<Vec3>,
}

#[derive(Default)]
struct Primitives {
	textured: Vec<TexturedPrimitive>,
	coloured: ColourPrimitive,
	outlines: ColourPrimitive,
	translucent: ColourPrimitive,
	shiny: ShinyPrimitive,
	unknown: MeshPrimitive,
}

impl MeshPrimitive {
	fn clear(&mut self) {
		self.verts.clear();
		self.indices.clear();
		self.anim_indices.clear();
	}
	fn add_to_gltf(
		&self, gltf: &mut gltf::Gltf, name: String, mesh: gltf::MeshIndex,
		material: gltf::MaterialIndex,
	) -> gltf::PrimitiveIndex {
		let result = gltf.create_mesh_primitive(
			mesh,
			Some(name),
			&self.verts,
			&self.indices,
			Some(material),
		);
		gltf.add_primitive_joints(result, &self.anim_indices);
		result
	}
}
impl Primitives {
	fn clear(&mut self) {
		for tex in &mut self.textured {
			tex.base.clear();
			tex.uvs.clear();
		}
		self.coloured.base.clear();
		self.coloured.colours.clear();
		self.outlines.base.clear();
		self.outlines.colours.clear();
		self.translucent.base.clear();
		self.translucent.colours.clear();
		self.shiny.base.clear();
		self.shiny.normals.clear();
		self.unknown.clear();
	}
}

fn flatten_into_primitive(part: &MeshPart, result: &mut MeshPrimitive, is_animated: bool) {
	result.verts.extend_from_slice(&part.verts);
	result.indices.extend(part.tris.iter().flat_map(|tri| {
		let [i1, i2, i3] = tri.indices;
		[i1, i3, i2]
	}));
	if is_animated {
		result.anim_indices.extend(0..part.verts.len() as u16);
	}
}
fn split_into_primitives<'a>(
	part: &MeshPart,
	Primitives {
		textured,
		coloured,
		outlines,
		translucent,
		shiny,
		unknown,
	}: &'a mut Primitives,
	materials: &Materials, is_animated: bool,
) {
	for tri in &part.tris {
		let material: Pen = if let Pen::Texture(tex_index) = tri.material {
			match &materials.materials[tex_index as usize] {
				ResolvedMaterial::Missing => Pen::Colour(0),
				ResolvedMaterial::Pen(pen) => *pen,
				ResolvedMaterial::TextureRef { .. } | ResolvedMaterial::TextureEmbed { .. } => {
					tri.material
				}
			}
		} else {
			tri.material
		};

		// outlines
		if tri.flags & TRIFLAG_DRAW_OUTLINE != 0 {
			let col = match material {
				Pen::Translucent(col) => materials.translucent_colours[col as usize],
				pen => {
					println!("outline on invalid mesh tri {pen:?}");
					[0; 4]
				}
			};

			let l12 = tri.flags & TRIFLAG_OUTLINE_12 != 0;
			let l13 = tri.flags & TRIFLAG_OUTLINE_13 != 0;
			let l23 = tri.flags & TRIFLAG_OUTLINE_23 != 0;
			let v1 = l12 || l13;
			let v2 = l12 || l23;
			let v3 = l13 || l23;

			let prim = &mut outlines.base;
			let index = prim.verts.len();

			for (i, has_vert) in [v1, v2, v3].iter().enumerate() {
				if !has_vert {
					continue;
				}
				let index = tri.indices[i];
				prim.verts.push(part.verts[index as usize]);
				outlines.colours.push(col);
				if is_animated {
					prim.anim_indices.push(index);
				}
			}

			let index = index as u16;
			if l12 {
				prim.indices.extend([index, index + 1]);
			}
			if l13 {
				if v2 {
					prim.indices.extend([index, index + 2]);
				} else {
					prim.indices.extend([index, index + 1]);
				}
			}
			if l23 {
				if v1 {
					prim.indices.extend([index + 1, index + 2]);
				} else {
					prim.indices.extend([index, index + 1]);
				}
			}
		}

		if tri.indices[0] == tri.indices[1]
			|| tri.indices[0] == tri.indices[2]
			|| tri.indices[1] == tri.indices[2]
		{
			// degenerate
			continue;
		}

		let prim: &mut MeshPrimitive = match material {
			Pen::Texture(tex_index) => {
				let prim = &mut textured[tex_index as usize];
				prim.uvs.extend(tri.uvs);
				&mut prim.base
			}
			Pen::Colour(col) => {
				let col = &materials.palette[col as usize * 3..];
				let col = [col[0], col[1], col[2], 255];
				coloured.colours.extend([col, col, col]);
				&mut coloured.base
			}
			Pen::Translucent(col) => {
				let col = materials.translucent_colours[col as usize];
				translucent.colours.extend([col, col, col]);
				&mut translucent.base
			}
			Pen::Shiny(shiny_angle) => &mut shiny.base, // todo normals
			Pen::Unknown(_) => unknown,
		};
		let index = prim.verts.len() as u16;
		prim.indices.extend([index, index + 2, index + 1]); // swizzle indices
		prim.verts
			.extend(tri.indices.iter().map(|i| part.verts[*i as usize]));
		if is_animated {
			prim.anim_indices.extend(tri.indices);
		}
	}
}

pub struct Materials<'a> {
	pub materials: &'a [ResolvedMaterial<'a>],
	pub palette: &'a [u8],
	pub translucent_colours: [[u8; 4]; 4],
}

pub enum ResolvedMaterial<'a> {
	Missing,
	Pen(Pen),
	TextureRef {
		width: u16,
		height: u16,
		path: &'a str,
		masked: bool,
	},
	TextureEmbed {
		texture: Texture<'a>,
		masked: bool,
	},
}
