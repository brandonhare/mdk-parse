use crate::{gltf, OutputWriter, Reader, Vec3};

use super::mesh::REFERENCE_POINTS_NAME;

#[derive(PartialEq)]
pub struct Animation<'a> {
	pub speed: f32,
	pub target_vectors: Vec<Vec3>, // todo what exactly are these?
	pub parts: Vec<AnimationPart<'a>>,
}

#[derive(PartialEq)]
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
		let mut any_ref_point = false;
		for _ in 0..num_reference_points {
			let points_path = Vec3::swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
			if !any_ref_point && points_path.iter().any(|vec| *vec == Default::default()) {
				any_ref_point = true;
			}
			reference_points.push(points_path);
		}

		if any_ref_point {
			parts.push(AnimationPart {
				name: REFERENCE_POINTS_NAME,
				point_paths: reference_points,
			});
		}

		max_pos = max_pos.max(data.position());
		reader.skip(max_pos); // update base reader

		Some(Animation {
			speed,
			target_vectors,
			parts,
		})
	}

	pub fn parse(reader: &mut Reader<'a>) -> Self {
		Self::try_parse(reader).expect("failed to parse animation")
	}

	pub fn num_frames(&self) -> usize {
		self.target_vectors.len()
	}

	pub fn add_to_gltf(
		&self, gltf: &mut gltf::Gltf, name: &str,
		part_joints: &[(&str, std::ops::Range<gltf::NodeIndex>)], cache: &mut Vec<Vec3>,
	) {
		let num_frames = self.num_frames();

		let fps = 30.0;

		//let cube_mesh = Some(gltf.get_cube_mesh());
		let animation = gltf.create_animation(name.into());
		let base_timestamps = gltf.create_animation_timestamps(num_frames, fps / self.speed);
		let interpolation = Some(gltf::AnimationInterpolationMode::Step);

		// todo target vectors
		/*
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
		*/

		for part in &self.parts {
			let Some((_, joint_nodes)) = part_joints
				.iter()
				.find(|(joint_name, _)| part.name == *joint_name)
			else {
				continue;
			};

			for (target_node, path) in (joint_nodes.start.0..joint_nodes.end.0)
				.map(gltf::NodeIndex)
				.zip(part.point_paths.iter())
			{
				let node_pos = gltf.get_node_position(target_node);
				cache.clear();
				cache.extend(path.iter().map(|pos| *pos - node_pos));
				gltf.add_animation_translation(
					animation,
					target_node,
					base_timestamps,
					cache,
					interpolation,
				);
			}
		}
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		let num_frames = self.num_frames();

		let fps = 30.0;

		let mut gltf = gltf::Gltf::new(name.into());
		let cube_mesh = Some(gltf.get_cube_mesh());
		let animation = gltf.create_animation(name.into());
		let base_timestamps = gltf.create_animation_timestamps(num_frames, fps / self.speed);
		let interpolation = Some(gltf::AnimationInterpolationMode::Step);

		if self.target_vectors.iter().any(|p| *p != Vec3::default()) {
			let node = gltf.create_root_node("Target Vectors".into(), cube_mesh);
			gltf.add_animation_translation(
				animation,
				node,
				base_timestamps,
				&self.target_vectors,
				interpolation,
			);
		}

		for part in &self.parts {
			let part_node = gltf.create_root_node(part.name.into(), None);
			let start_index = gltf
				.create_node_range(
					part_node,
					cube_mesh,
					part.point_paths.iter().map(|list| *list.first().unwrap()),
				)
				.start
				.0;
			for (i, path) in part.point_paths.iter().enumerate() {
				gltf.add_animation_translation(
					animation,
					gltf::NodeIndex(start_index + i),
					base_timestamps,
					path,
					interpolation,
				);
			}
		}

		output.write(name, "anim.gltf", gltf.render_json().as_bytes());
	}

	pub fn check_joints(&self, name: &str, report: &mut String) {
		use std::fmt::Write;
		writeln!(report, "{name}").unwrap();
		for part in &self.parts {
			let part_name = part.name;
			let num_points = part.point_paths.len();
			if num_points < 4 {
				writeln!(report, "part {part_name} has too few points {num_points}").unwrap();
				continue;
			}
			let mut best = [0, 1, 2, 3];
			let mut best_sum = f32::INFINITY;
			for i in 0..num_points {
				let p0 = part.point_paths[i][0];
				for j in 0..num_points - 2 {
					let p1 = part.point_paths[j][0];
					let v1 = (p1 - p0);
					if v1.length() < 0.001 {
						continue;
					}
					let v1 = v1.normalized();
					for k in j + 1..num_points - 1 {
						let p2 = part.point_paths[k][0];
						let v2 = (p2 - p0);
						if v2.length() < 0.001 || (p2 - p1).length() < 0.001 {
							continue;
						}
						let v2 = v2.normalized();
						let n1 = v1.dot(v2);
						for l in k + 1..num_points {
							let p3 = part.point_paths[l][0];
							let v3 = (p3 - p0);
							if v3.length() < 0.001
								|| (p3 - p1).length() < 0.001
								|| (p3 - p2).length() < 0.001
							{
								continue;
							}
							let v3 = p2.normalized();
							let n2 = v1.dot(v3);
							let n3 = v2.dot(v3);
							let sum = n1.abs() + n2.abs() + n3.abs();
							if sum < best_sum {
								best_sum = sum;
								best = [i, j, k, l];
							}
						}
					}
				}
			}

			let make_mat = |index| {
				//euclid::default::Transform3D::from_array(
				euclid::default::Transform3D::from_arrays(
					best.map(|i| Vec3::to_point(part.point_paths[i][index])),
				)
				//	.to_array_transposed(),
				//)
			};

			let src @ [p0, p1, p2, p3] = best.map(|i| part.point_paths[i][0]);
			let Some(inv_src) = make_mat(0).inverse() else {
				continue;
			};

			for frame in 0..part.point_paths[0].len() {
				let dest_mat = make_mat(frame);
				let transform = inv_src.then(&dest_mat);

				let dest = best.map(|i| part.point_paths[i][frame]);
				for (src, dest) in src.iter().copied().zip(dest) {
					let w = transform.transform_point3d_homogeneous((*src).into());
					let x = Vec3::new(w.x, w.y, w.z);
					let w = w.w;
					if (x - dest).length() > 0.001 || (w - 1.0).abs() > 0.001 {
						writeln!(report, "part: {part_name}, frame: {frame}\nsrc: {src:.1?}\ninv_src: {inv_src:.1?}\ndest_mat: {dest_mat:.1?}\ntransform: {transform:.1?}\ndest: {dest:.1?}\nsrc: {src:.1?}\nx: {x:.1?} ({w:.1})\n").unwrap();
						return;
					}
				}
			}
		}
		report.push('\n');
	}
}
