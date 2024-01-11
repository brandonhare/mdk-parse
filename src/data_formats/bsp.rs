use crate::data_formats::mesh::{Mesh, MeshGeo, MeshTri, MeshType, TextureHolder};
use crate::{gltf, OutputWriter, Reader, Vec3};

pub struct BspPlane {
	pub normal: Vec3,
	pub dist: f32,
	pub plane_index_behind: i16,
	pub plane_index_front: i16,
	pub tris_front_count: u16,
	pub tris_front_index: u16,
	pub tris_back_count: u16,
	pub tris_back_index: u16,
}

pub struct Bsp<'a> {
	pub planes: Vec<BspPlane>,
	pub mesh: Mesh<'a>,
}

impl<'a> Bsp<'a> {
	pub fn parse(data: &mut Reader<'a>) -> Bsp<'a> {
		let num_materials = data.u32();
		assert!(num_materials < 100, "too many bsp materials");
		let materials = (0..num_materials)
			.map(|_| data.str(10))
			.collect::<Vec<&str>>();

		data.align(4);

		let num_planes = data.u32() as usize;
		assert!(num_planes < 10000, "too many bsp planes");
		let mut planes = Vec::with_capacity(num_planes);
		for _ in 0..num_planes {
			let result = BspPlane {
				normal: data.vec3(),
				dist: data.f32(),
				plane_index_behind: data.i16(),
				plane_index_front: data.i16(),
				tris_front_count: data.u16(),
				tris_front_index: data.u16(),
				tris_back_count: data.u16(),
				tris_back_index: data.u16(),
			};

			let zeroes = data.get::<[u32; 4]>();
			assert_eq!(zeroes, [0; 4]);

			assert!((-1..=num_planes as i16).contains(&result.plane_index_behind));
			assert!((-1..=num_planes as i16).contains(&result.plane_index_front));

			assert!((result.normal.iter().map(|f| f * f).sum::<f32>() - 1.0).abs() <= 0.0001);
			planes.push(result);
		}

		let num_tris = data.u32() as usize;
		let tris = MeshTri::try_parse_slice(data, num_tris).unwrap();

		let num_verts = data.u32() as usize;
		assert!(num_verts < 10000);
		let verts = Vec3::swizzle_vec(data.get_vec::<Vec3>(num_verts));

		let num_things = data.u32();
		assert!(num_things < 10000);
		let things = data.slice(num_things as usize);
		assert!(things.iter().all(|c| *c == 255)); // todo what are these?

		//assert_eq!(data.position(), data.len());

		let bbox = Vec3::calculate_bbox(&verts);

		Bsp {
			planes,
			mesh: Mesh {
				materials,
				reference_points: Vec::new(),
				mesh_data: MeshType::Single(MeshGeo { verts, tris, bbox }),
			},
		}
	}

	pub fn save_as(
		&self, name: &str, output: &mut OutputWriter, textures: Option<&dyn TextureHolder>,
	) {
		self.mesh.save_as(name, output, textures)
	}
}
