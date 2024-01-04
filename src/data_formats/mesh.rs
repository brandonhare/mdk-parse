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

	pub fn add_to_gltf(
		&self, gltf: &mut gltf::Gltf, name: String, target: Option<gltf::NodeIndex>,
	) -> gltf::NodeIndex {
		// todo materials

		let target = target.unwrap_or_else(|| gltf.create_node(name.clone(), None));
		assert!(
			gltf.get_node_mesh(target).is_none(),
			"tried to replace mesh node"
		);

		let indices: Vec<u16> = self
			.tris
			.iter()
			.flat_map(|tri| [tri.indices[0], tri.indices[2], tri.indices[1]]) // swizzle indices
			.collect();
		let mesh = gltf.create_mesh_from_primitive(name, &self.verts, &indices, None, None);
		gltf.set_node_mesh(target, mesh);

		target
	}
}

pub struct Submesh<'a> {
	pub mesh_data: MeshGeo,
	pub name: &'a str,
	pub origin: Vec3,
}

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
	pub fn outlines(&self) -> [bool; 3] {
		[
			self.flags & 0x100000 != 0,
			self.flags & 0x200000 != 0,
			self.flags & 0x400000 != 0,
		]
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
		self.add_to_gltf(&mut gltf, name.to_owned(), Some(root));

		output.write(name, "gltf", gltf.render_json().as_bytes());
	}

	pub fn add_to_gltf(
		&self, gltf: &mut gltf::Gltf, name: String, target: Option<gltf::NodeIndex>,
	) -> gltf::NodeIndex {
		let target = target.unwrap_or_else(|| gltf.create_node(name.clone(), None));

		match &self.mesh_data {
			MeshType::Single(geo) => {
				geo.add_to_gltf(gltf, name, Some(target));
			}
			MeshType::Multimesh { submeshes, .. } => {
				for sub in submeshes {
					let sub_node = sub.mesh_data.add_to_gltf(gltf, sub.name.to_owned(), None);
					gltf.set_node_position(sub_node, sub.origin);
					gltf.set_node_parent(target, sub_node);
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
}
