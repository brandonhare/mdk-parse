use crate::{
	add_mesh_to_gltf, get_bbox, gltf, output_writer::OutputWriter, reader::Reader, swizzle_vec,
	try_parse_mesh_tris, Mesh, Vec3,
};

#[derive(Debug)]
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

#[derive(Debug)]
pub struct Bsp<'a> {
	pub planes: Vec<BspPlane>,
	pub mesh: Mesh<'a>,
}

impl<'a> Bsp<'a> {
	pub fn parse(data: &mut Reader<'a>) -> Bsp<'a> {
		let num_materials = data.u32();
		assert!(num_materials < 100, "too many bsp materials");
		let textures = (0..num_materials)
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
		let tris = try_parse_mesh_tris(data, num_tris).unwrap();

		let num_verts = data.u32() as usize;
		assert!(num_verts < 10000);
		let verts = swizzle_vec(data.get_vec::<Vec3>(num_verts));

		let num_things = data.u32();
		assert!(num_things < 10000);
		let things = data.slice(num_things as usize);
		assert!(things.iter().all(|c| *c == 255)); // todo what are these?

		//assert_eq!(data.position(), data.len());

		Bsp {
			planes,
			mesh: Mesh {
				textures,
				bbox: get_bbox(&verts),
				verts,
				tris,
				reference_points: Vec::new(),
			},
		}
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		let mut gltf = gltf::Gltf::new(name.to_owned());

		let root = gltf.get_root_node();
		add_mesh_to_gltf(&mut gltf, name.to_owned(), &self.mesh, &[], Some(root));

		gltf.combine_buffers();
		output.write(
			name,
			"gltf",
			serde_json::to_string(&gltf).unwrap().as_bytes(),
		);

		//save_bsp_debug(name, bsp, output);
	}

	pub fn save_debug_as(&self, name: &str, output: &mut OutputWriter) {
		fn recurse(
			gltf: &mut gltf::Gltf, temp_mesh: &mut Mesh, bsp: &Bsp, index: usize,
			node: gltf::NodeIndex,
		) {
			let plane = &bsp.planes[index];

			let front_index = plane.plane_index_front;
			if front_index >= 0 {
				let right_node = gltf.create_child_node(node, format!("front_{front_index}"), None);
				recurse(gltf, temp_mesh, bsp, front_index as usize, right_node);
			}

			temp_mesh.tris.clear();
			for i in 0..plane.tris_front_count {
				let tri = &bsp.mesh.tris[(plane.tris_front_index + i) as usize];
				if tri.indices[0] == tri.indices[1] && tri.indices[0] == tri.indices[2] {
					continue;
				}
				temp_mesh.tris.push(tri.clone());
			}
			for i in 0..plane.tris_back_count {
				let tri = &bsp.mesh.tris[(plane.tris_back_index + i) as usize];
				if tri.indices[0] == tri.indices[1] && tri.indices[0] == tri.indices[2] {
					continue;
				}
				temp_mesh.tris.push(tri.clone());
			}

			if !temp_mesh.tris.is_empty() {
				let mesh_node = gltf.create_child_node(node, format!("mesh_{index}"), None);
				add_mesh_to_gltf(gltf, format!("{index}"), temp_mesh, &[], Some(mesh_node));

				let flags_summary: Vec<_> = temp_mesh
				.tris
				.iter()
				.enumerate()
				.filter(|(_,t)| t.flags != 0)
				.map(
					|(i,t)| serde_json::json!({"index": i, "id": t.id(), "outlines": t.outlines(), "flags": t.flags & 0x008F_FFFF}),
				)
				.collect();
				gltf.set_node_extras(mesh_node, "flags", flags_summary);
			}

			let behind_index = plane.plane_index_behind;
			if behind_index >= 0 {
				let left_node =
					gltf.create_child_node(node, format!("behind_{behind_index}"), None);
				recurse(gltf, temp_mesh, bsp, behind_index as usize, left_node);
			}
		}

		let mut gltf = gltf::Gltf::new(name.to_owned());
		let node = gltf.get_root_node();
		recurse(&mut gltf, &mut self.mesh.clone(), self, 0, node);

		gltf.combine_buffers();
		output.write(
			name,
			"debug.gltf",
			serde_json::to_string(&gltf).unwrap().as_bytes(),
		);
	}
}
