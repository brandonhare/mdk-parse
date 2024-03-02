use std::collections::HashMap;

use crate::data_formats::mesh::{self, Mesh, MeshPart, MeshTri};
use crate::{OutputWriter, Reader, Vec3};

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
	pub fn parse(data: &mut Reader<'a>, name: &'a str) -> Bsp<'a> {
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

		let full_bbox = Vec3::calculate_bbox(&verts);

		let parts = split_by_id(name, verts, tris);

		let mut mesh = Mesh {
			materials,
			parts,
			bbox: full_bbox,
			reference_points: Vec::new(),
		};

		mesh.remove_unused_materials();

		Bsp { planes, mesh }
	}

	pub fn save_as(
		&self, name: &str, output: &mut OutputWriter, materials: Option<&mesh::Materials>,
	) {
		self.mesh.save_as(name, output, materials, &[])
	}
}

fn split_by_id<'a>(name: &'a str, verts: Vec<Vec3>, mut tris: Vec<MeshTri>) -> Vec<MeshPart<'a>> {
	let mut result: Vec<MeshPart> = Vec::new();
	type VertKey = (u8, u16); // part id, vertex index
	let mut tri_map: HashMap<VertKey, u16> = HashMap::new();

	for tri in tris {
		if tri.is_hidden() {
			continue;
		}
		let id = tri.id();

		if id as usize >= result.len() {
			result.resize_with(id as usize + 1, Default::default);
		}
		let part = &mut result[id as usize];

		let mut new_tri = tri.clone();
		for index in &mut new_tri.indices {
			*index = *tri_map.entry((id, *index)).or_insert_with(|| {
				let new_index = part.verts.len();
				part.verts.push(verts[*index as usize]);
				new_index as u16
			});
		}
		part.tris.push(new_tri);
	}

	let mut i = 0;
	result.retain_mut(|part| {
		let index = i;
		i += 1;
		if part.tris.is_empty() {
			return false;
		}

		if index == 0 {
			part.name = name.into();
		} else {
			part.name = format!("id {index}").into();
		}
		part.bbox = Vec3::calculate_bbox(&part.verts);

		true
	});

	result
}
