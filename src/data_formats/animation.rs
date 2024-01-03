use crate::{gltf, OutputWriter, Reader, Vec3};

pub struct Animation<'a> {
	pub speed: f32,
	pub target_vectors: Vec<Vec3>, // todo what exactly are these?
	pub reference_points: Vec<Vec<Vec3>>,
	pub parts: Vec<AnimationPart<'a>>,
}

pub struct AnimationPart<'a> {
	pub name: &'a str,
	pub point_paths: Vec<Vec<Vec3>>,
}

impl<'a> Animation<'a> {
	pub fn try_parse(reader: &mut Reader<'a>) -> Option<Self> {
		let speed = reader.try_f32()?;
		let mut data = reader.rebased();

		let num_parts = data.try_u32()? as usize;
		let num_frames = data.try_u32()? as usize;
		if num_parts == 0 || num_parts > 1000 || num_frames == 0 || num_frames > 1000 {
			return None;
		}

		let mut parts = Vec::with_capacity(num_parts);
		let mut max_pos = data.position();
		for _ in 0..num_parts {
			let part_offset = data.try_u32()? as usize;
			if part_offset >= data.len() {
				return None;
			}

			let mut data = data.clone_at(part_offset);

			let part_name = data.try_str(12)?;
			let num_points = data.try_u32()? as usize;
			if num_points > 1000 {
				return None;
			}
			let scale = data.try_f32()?;
			let mut point_paths: Vec<Vec<Vec3>> = Vec::new();
			point_paths.resize_with(num_points, || Vec::with_capacity(num_frames));

			if scale != 0.0 {
				// origin points
				for path in &mut point_paths {
					path.push(data.try_vec3()?.swizzle());
				}
				// frames
				for _ in 0..num_frames {
					let frame_index = data.try_u16()? as usize;
					if frame_index > num_frames {
						break;
					}

					for path in &mut point_paths {
						if frame_index < path.len() {
							return None; // frames out of order
						}
						let prev = *path.last().unwrap();
						path.resize(frame_index, prev); // duplicate potential gaps so our timeline is full

						let pos = data.try_get::<[i8; 3]>()?;
						let pos = Vec3::from(pos.map(|i| i as f32 * scale)).swizzle();
						path.push(prev + pos);
					}

					if frame_index == num_frames {
						assert_eq!(data.u16(), 0xFFFF);
						break;
					}
				}
			} else {
				// transforms

				let scale_vec = 1.0 / (0x8000u32 >> (data.try_u8()? & 0x3F)) as f32;
				let scale_pos = 1.0 / (0x8000u32 >> (data.try_u8()? & 0x3F)) as f32;

				let origin_points = data.try_get_vec::<Vec3>(num_points)?;
				// don't swizzle until after processing

				for _ in 0..num_frames {
					let transform = data.try_get::<[[i16; 4]; 3]>()?;
					let [r1, r2, r3] = transform.map(|[x, y, z, w]| {
						[
							x as f32 * scale_vec,
							y as f32 * scale_vec,
							z as f32 * scale_vec,
							w as f32 * scale_pos,
						]
					});

					for (path, &Vec3 { x, y, z }) in point_paths.iter_mut().zip(&origin_points) {
						path.push(
							Vec3::from([
								r1[0] * x + r1[1] * y + r1[2] * z + r1[3],
								r2[0] * x + r2[1] * y + r2[2] * z + r2[3],
								r3[0] * x + r3[1] * y + r3[2] * z + r3[3],
							])
							.swizzle(),
						)
					}
				}
			}

			for path in &mut point_paths {
				if path.len() > num_frames {
					return None;
				}
				path.resize(num_frames, *path.last().unwrap()); // duplicate until the end of the timeline
			}

			max_pos = max_pos.max(data.position());

			parts.push(AnimationPart {
				name: part_name,
				point_paths,
			});
		}

		let mut target_vectors = Vec3::swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
		for i in 1..target_vectors.len() {
			// todo added in gameplay
			target_vectors[i] = target_vectors[i] + target_vectors[i - 1];
		}

		let num_reference_points = data.try_u32()? as usize;
		if num_reference_points > 8 || num_reference_points * num_frames * 12 > data.remaining_len()
		{
			return None;
		}
		let mut reference_points: Vec<Vec<Vec3>> = Vec::with_capacity(num_reference_points);
		for _ in 0..num_reference_points {
			let points_path = Vec3::swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
			reference_points.push(points_path);
		}

		max_pos = max_pos.max(data.position());
		reader.skip(max_pos); // update base reader

		Some(Animation {
			speed,
			target_vectors,
			reference_points,
			parts,
		})
	}

	pub fn parse(reader: &mut Reader<'a>) -> Self {
		Self::try_parse(reader).expect("failed to parse animation")
	}

	pub fn num_frames(&self) -> usize {
		self.target_vectors.len()
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		let num_frames = self.num_frames();

		let fps = 30.0;

		let mut gltf = gltf::Gltf::new(name.into());
		let cube_mesh = Some(gltf.get_cube_mesh());
		let animation = gltf.create_animation(name.into());
		let root_node = gltf.get_root_node();
		let base_timestamps = gltf.create_animation_timestamps(num_frames, fps / self.speed);
		let interpolation = Some(gltf::AnimationInterpolationMode::Step);

		if self.target_vectors.iter().any(|p| *p != Vec3::default()) {
			let node = gltf.create_child_node(root_node, "Target Vectors".into(), cube_mesh);
			gltf.add_animation_translation(
				animation,
				node,
				base_timestamps,
				&self.target_vectors,
				interpolation,
			);
		}

		if self
			.reference_points
			.iter()
			.any(|p| p.iter().any(|p| *p != Vec3::default()))
		{
			let ref_node = gltf.create_child_node(root_node, "Reference Points".into(), None);
			for (i, path) in self.reference_points.iter().enumerate() {
				let node = gltf.create_child_node(ref_node, i.to_string(), cube_mesh);
				gltf.add_animation_translation(
					animation,
					node,
					base_timestamps,
					path,
					interpolation,
				);
			}
		}

		for part in &self.parts {
			let part_node = gltf.create_child_node(root_node, part.name.into(), None);
			for (i, path) in part.point_paths.iter().enumerate() {
				let point_node = gltf.create_child_node(part_node, i.to_string(), cube_mesh);
				gltf.add_animation_translation(
					animation,
					point_node,
					base_timestamps,
					path,
					interpolation,
				);
			}
		}

		output.write(name, "anim.gltf", gltf.render_json().as_bytes());
	}
}
