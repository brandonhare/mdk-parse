use std::collections::HashMap;
use std::fmt::Write;

use crate::data_formats::cmi_bytecode::CmiCallOrigin;
use crate::data_formats::{cmi_bytecode, Animation, Mesh, Spline};
use crate::{OutputWriter, Reader};

#[derive(Default)]
pub struct Cmi<'a> {
	pub filename: &'a str,
	pub arenas: Vec<CmiArena<'a>>,
	pub animations: HashMap<u32, Animation<'a>>,
	pub splines: HashMap<u32, Spline>,
	pub scripts: HashMap<u32, cmi_bytecode::CmiScript<'a>>,
	pub entities: HashMap<&'a str, CmiEntity<'a>>,
}

#[derive(Default)]
pub struct CmiArena<'a> {
	pub name: &'a str,
	pub song: &'a str,
	pub entities: Vec<&'a str>,
}

#[derive(Default)]
pub struct CmiEntity<'a> {
	pub mesh: Option<Mesh<'a>>,
	pub animations: Vec<u32>,
	pub animation_names: Vec<&'a str>,
	pub splines: Vec<u32>,
	pub scripts: Vec<u32>,
	pub arenas: Vec<&'a str>,
}

impl<'a> Cmi<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Self {
		let filesize = reader.u32() as usize;
		assert_eq!(reader.len(), filesize + 4, "filesize does not match");
		reader.rebase();

		let filename = reader.str(12);
		let filesize2 = reader.u32() as usize;
		assert_eq!(filesize, filesize2 + 8, "filesizes do not match");

		let mut result = Cmi {
			filename,
			..Default::default()
		};

		let mut scripts: Vec<(u32, CmiCallOrigin)> = Vec::new();

		// init scripts
		let num_init_scripts = reader.u32() as usize;
		scripts.reserve(num_init_scripts);
		for _ in 0..num_init_scripts {
			let name = reader.pascal_str();
			let init_script_offset = reader.u32();

			assert_ne!(init_script_offset, 0, "found null init script for {name}");

			let (arena_name, entity_name) = name.split_once('$').unwrap();
			let (entity_name, entity_id) =
				entity_name.split_once('_').unwrap_or((entity_name, "None"));

			scripts.push((
				init_script_offset,
				CmiCallOrigin {
					arena_name,
					source_name: entity_name,
					target_name: entity_name,
					source_offset: 0,
					reason: format!("Init (id {entity_id})").into(),
				},
			));
		}

		// meshes
		let num_meshes = reader.u32() as usize;
		result.entities.reserve(num_meshes);
		for _ in 0..num_meshes {
			let name = reader.pascal_str();
			let offset = reader.u32() as usize;

			let mesh: Option<Mesh> = if offset == 0 {
				None
			} else {
				let mut mesh_reader = reader.clone_at(offset);
				let is_multimesh = mesh_reader.u32();
				assert!(is_multimesh <= 1);
				Some(Mesh::parse(&mut mesh_reader, is_multimesh != 0))
			};

			let new_entity = result
				.entities
				.insert(
					name,
					CmiEntity {
						mesh,
						..Default::default()
					},
				)
				.is_none();
			assert!(new_entity);
		}

		// setup scripts
		let num_setup_scripts = reader.u32() as usize;
		scripts.reserve(num_setup_scripts);
		for _ in 0..num_setup_scripts {
			let name = reader.pascal_str();
			let setup_script_offset = reader.u32();

			assert_ne!(setup_script_offset, 0, "found null setup script for {name}");

			let (arena_name, entity_name) = name.split_once('$').unwrap();

			scripts.push((
				setup_script_offset,
				CmiCallOrigin {
					arena_name,
					source_name: entity_name,
					source_offset: 0,
					target_name: entity_name,
					reason: "Setup".into(),
				},
			));
		}

		// arenas
		let num_arenas = reader.u32() as usize;
		result.arenas.reserve(num_arenas);
		scripts.reserve(num_arenas);
		for _ in 0..num_arenas {
			let name = reader.pascal_str();
			let offset = reader.u32();
			let mut arena_reader = reader.clone_at(offset as usize);

			let music1 = arena_reader.pascal_str();
			let music2 = arena_reader.pascal_str();
			assert!(music1.is_empty() || music1 == "NONE");

			let script_offset = arena_reader.u32();

			result.arenas.push(CmiArena {
				name,
				song: music2,
				entities: Vec::new(),
			});

			scripts.push((
				script_offset,
				CmiCallOrigin {
					arena_name: name,
					source_name: name,
					target_name: name,
					source_offset: 0,
					reason: "Setup".into(),
				},
			));
		}

		// parse all scripts
		while let Some((target_offset, origin)) = scripts.pop() {
			let script = result.scripts.entry(target_offset).or_insert_with(|| {
				// new script
				let script =
					cmi_bytecode::CmiScript::parse(reader.clone_at(target_offset as usize));

				scripts.extend(script.called_scripts.iter().map(|s| {
					(
						s.target_offset,
						CmiCallOrigin {
							arena_name: origin.arena_name,
							source_offset: target_offset,
							source_name: origin.target_name,
							target_name: s.target_name,
							reason: s.reason.into(),
						},
					)
				}));

				script
			});

			let entity = result.entities.entry(origin.target_name).or_default();

			entity.animation_names.extend_from_slice(&script.anim_names);
			entity.animations.extend_from_slice(&script.anim_offsets);
			entity.splines.extend_from_slice(&script.path_offsets);
			entity.scripts.push(target_offset);
			entity.arenas.push(origin.arena_name);

			if origin.target_name != origin.arena_name {
				result
					.arenas
					.iter_mut()
					.find(|a| a.name == origin.arena_name)
					.unwrap()
					.entities
					.push(origin.target_name);
			}

			script.call_origins.push(origin);
		}
		// finished parsing entities

		for script in result.scripts.values_mut() {
			script.call_origins.sort_unstable();
			script.call_origins.dedup();
		}
		for arena in &mut result.arenas {
			arena.entities.sort_unstable();
			arena.entities.dedup();
		}

		// parse animations and splines
		for entity in result.entities.values_mut() {
			entity.arenas.sort_unstable();
			entity.arenas.dedup();

			entity.animation_names.sort_unstable();
			entity.animation_names.dedup();

			entity.animations.sort_unstable();
			entity.animations.dedup();
			for &anim_offset in &entity.animations {
				result.animations.entry(anim_offset).or_insert_with(|| {
					Animation::parse(&mut reader.resized(anim_offset as usize..))
				});
			}

			entity.splines.sort_unstable();
			entity.splines.dedup();
			for &spline_offset in &entity.splines {
				result.splines.entry(spline_offset).or_insert_with(|| {
					Spline::parse(&mut reader.resized(spline_offset as usize..))
				});
			}

			entity.scripts.sort_unstable();
			entity.scripts.dedup();
		}

		result
	}

	pub fn save(&self, output: &mut OutputWriter) {
		let mut temp_filename = String::new();
		let mut temp_data = String::new();
		let mut temp_reason_list: Vec<&str> = Vec::new();
		let mut temp_arena_list: Vec<&str> = Vec::new();

		// arenas
		for arena in &self.arenas {
			let mut output = output.push_dir(arena.name);
			if !arena.song.is_empty() {
				output.write("Song", "txt", arena.song);
			}

			if !arena.entities.is_empty() {
				temp_data.clear();
				for entity in &arena.entities {
					temp_data.push_str(entity);
					temp_data.push('\n');
				}
				output.write("Entities", "txt", &temp_data);
			}
		}

		// entities
		for (&entity_name, entity) in self.entities.iter() {
			let mut output = output.push_dir(entity_name);

			// save mesh
			if let Some(mesh) = &entity.mesh {
				mesh.save_as(entity_name, &mut output);
			}

			// save animations
			if !entity.animations.is_empty() || !entity.animation_names.is_empty() {
				let mut output = output.push_dir("Animations");
				for anim_offset in &entity.animations {
					temp_filename.clear();
					write!(temp_filename, "{anim_offset:06X}").unwrap();
					self.animations[anim_offset].save_as(&temp_filename, &mut output);
				}
				if !entity.animation_names.is_empty() {
					temp_data.clear();
					for anim_name in &entity.animation_names {
						temp_data.push_str(anim_name);
						temp_data.push('\n');
					}
					output.write("Animation Refs", "txt", &temp_data);
				}
			}

			// save splines
			if !entity.splines.is_empty() {
				let mut output = output.push_dir("Splines");
				for spline_offset in &entity.splines {
					temp_filename.clear();
					write!(temp_filename, "{spline_offset:06X}").unwrap();
					self.splines[spline_offset].save_as(&temp_filename, &mut output);
				}
			}

			// save scripts
			if !entity.scripts.is_empty() {
				let output = output.push_dir("Scripts");
				for script_offset in &entity.scripts {
					let script = &self.scripts[script_offset];

					temp_data.clear();
					temp_data.push_str("Called by:\n");

					let mut shared = false;

					// create filename from reasons
					temp_reason_list.clear();
					temp_arena_list.clear();
					for origin in &script.call_origins {
						if origin.target_name == entity_name {
							temp_reason_list.push(&origin.reason);
							temp_arena_list.push(origin.arena_name);
							writeln!(
								temp_data,
								"\t[{}] from {} ({:06X}): {}",
								origin.arena_name,
								origin.source_name,
								origin.source_offset,
								origin.reason
							)
							.unwrap();
						} else if *script_offset != 0 {
							shared = true;
						}
					}
					temp_reason_list.sort_unstable();
					temp_reason_list.dedup();
					temp_arena_list.sort_unstable();
					temp_arena_list.dedup();
					temp_filename.clear();

					let mut output = if temp_arena_list.len() == 1 {
						output.push_dir(temp_arena_list[0])
					} else {
						output.push_dir("Shared")
					};

					write!(temp_filename, "{script_offset:06X}").unwrap();
					for reason in &temp_reason_list {
						write!(temp_filename, " {reason}").unwrap();
					}

					if shared {
						temp_data.push_str("\nShared by:\n");
						for origin in &script.call_origins {
							if origin.target_name != entity_name {
								writeln!(
									temp_data,
									"\t[{}] {} from {} ({:06X}): {}",
									origin.arena_name,
									origin.target_name,
									origin.source_name,
									origin.source_offset,
									origin.reason
								)
								.unwrap();
							}
						}
					}

					temp_data.push('\n');
					temp_data.push_str(&script.summary);

					output.write(&temp_filename, "txt", &temp_data);
				}
			}
		}
	}
}
