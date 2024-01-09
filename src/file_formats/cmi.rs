use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::data_formats::cmi_bytecode::CmiCallOrigin;
use crate::data_formats::{cmi_bytecode, Animation, Mesh, Spline};
use crate::{OutputWriter, Reader};

#[derive(Default)]
pub struct Cmi<'a> {
	pub filename: &'a str,
	pub arenas: HashMap<&'a str, CmiArena<'a>>,
	pub meshes: HashMap<&'a str, Option<Mesh<'a>>>,
	pub animations: HashMap<u32, Animation<'a>>,
	pub splines: HashMap<u32, Spline>,
	pub scripts: HashMap<u32, cmi_bytecode::CmiScript<'a>>,
}

#[derive(Default)]
pub struct CmiArena<'a> {
	pub name: &'a str,
	pub song: &'a str,
	pub entities: HashMap<&'a str, CmiEntity<'a>>,
}

#[derive(Default)]
pub struct CmiEntity<'a> {
	pub animations: Vec<u32>,
	pub animation_names: Vec<&'a str>,
	pub splines: Vec<u32>,
	pub scripts: Vec<u32>,
}

impl<'a> CmiArena<'a> {
	fn new(name: &'a str) -> Self {
		CmiArena {
			name,
			entities: HashMap::from_iter(Some((name, Default::default()))),
			song: "",
		}
	}
	fn get_entity(&mut self, name: &'a str) -> &mut CmiEntity<'a> {
		self.entities.entry(name).or_default()
	}
}

impl<'a> std::ops::Deref for CmiArena<'a> {
	type Target = CmiEntity<'a>;
	fn deref(&self) -> &Self::Target {
		&self.entities[self.name]
	}
}
impl<'a> std::ops::DerefMut for CmiArena<'a> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.entities.get_mut(self.name).unwrap()
	}
}

impl<'a> Cmi<'a> {
	fn get_arena(&mut self, name: &'a str) -> &mut CmiArena<'a> {
		self.arenas
			.entry(name)
			.or_insert_with(|| CmiArena::new(name))
	}

	fn parse_all_scripts(&mut self, reader: &Reader<'a>, mut queue: Vec<(u32, CmiCallOrigin<'a>)>) {
		while let Some((target_offset, origin)) = queue.pop() {
			let script = self.scripts.entry(target_offset).or_insert_with(|| {
				let script =
					cmi_bytecode::CmiScript::parse(reader.clone_at(target_offset as usize));
				queue.extend(script.called_scripts.iter().map(|s| {
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

			let entity = self
				.arenas
				.get_mut(origin.arena_name)
				.unwrap()
				.entities
				.entry(origin.target_name)
				.or_default();

			entity.animation_names.extend_from_slice(&script.anim_names);
			entity.animations.extend_from_slice(&script.anim_offsets);
			entity.splines.extend_from_slice(&script.path_offsets);
			entity.scripts.push(target_offset);

			script.call_origins.push(origin);
		}
	}

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
		for _ in 0..num_init_scripts {
			let name = reader.pascal_str();
			let init_script_offset = reader.u32();

			assert_ne!(init_script_offset, 0, "found null init script for {name}");

			let (arena_name, entity_name) = name.split_once('$').unwrap();
			let (entity_name, entity_id) =
				entity_name.split_once('_').unwrap_or((entity_name, "?"));

			scripts.push((
				init_script_offset,
				CmiCallOrigin {
					arena_name,
					source_name: entity_name,
					target_name: entity_name,
					source_offset: 0,
					reason: format!("Init (id: {entity_id})").into(),
				},
			));
		}

		// meshes
		let num_meshes = reader.u32() as usize;
		result.meshes.reserve(num_meshes);
		for _ in 0..num_meshes {
			let name = reader.pascal_str();
			let offset = reader.u32() as usize;

			if offset == 0 {
				result.meshes.insert(name, None);
				continue;
			}

			let mut mesh_reader = reader.clone_at(offset);
			let is_multimesh = mesh_reader.u32();
			assert!(is_multimesh <= 1);
			let mesh = Mesh::parse(&mut mesh_reader, is_multimesh != 0);
			result.meshes.insert(name, Some(mesh));
		}

		// setup scripts
		let num_setup_scripts = reader.u32() as usize;
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
		for _ in 0..num_arenas {
			let name = reader.pascal_str();
			let offset = reader.u32();
			let mut arena_reader = reader.clone_at(offset as usize);

			let music1 = arena_reader.pascal_str();
			let music2 = arena_reader.pascal_str();
			assert!(music1.is_empty() || music1 == "NONE");

			let script_offset = arena_reader.u32();

			let arena = result.get_arena(name);
			arena.song = music2;

			if script_offset != 0 {
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
		}

		result.parse_all_scripts(&reader, scripts);

		// parse animations and splines
		for arena in result.arenas.values_mut() {
			for entity in arena.entities.values_mut() {
				entity.animations.sort();
				entity.animations.dedup();
				for &anim_offset in &entity.animations {
					result.animations.entry(anim_offset).or_insert_with(|| {
						Animation::parse(&mut reader.resized(anim_offset as usize..))
					});
				}

				entity.splines.sort();
				entity.splines.dedup();
				for &spline_offset in &entity.splines {
					result.splines.entry(spline_offset).or_insert_with(|| {
						Spline::parse(&mut reader.resized(spline_offset as usize..))
					});
				}

				entity.scripts.sort();
				entity.scripts.dedup();
			}
		}

		result
	}

	pub fn save(&self, output: &mut OutputWriter) {
		let mut temp1 = String::new();
		let mut temp2 = String::new();
		let mut temp3: Vec<&str> = Vec::new();

		let mut used_anims = HashSet::new();
		let mut used_meshes = HashSet::new();
		let mut used_splines = HashSet::new();

		for (&arena_name, arena) in self.arenas.iter() {
			let mut output = output.push_dir(arena_name);

			if !arena.song.is_empty() {
				output.write("song", "txt", arena.song);
			}

			for (&entity_name, entity) in &arena.entities {
				if entity.scripts.is_empty() {
					continue;
				}
				let mut output = output.push_dir(entity_name);

				// save mesh
				used_meshes.insert(entity_name);
				match self.meshes.get(entity_name) {
					Some(Some(mesh)) => mesh.save_as(entity_name, &mut output),
					Some(None) => output.write("mesh", "txt", entity_name),
					None => {} // todo missing meshes?
				}

				// save animations
				if !entity.animations.is_empty() {
					let mut output = output.push_dir("Animations");
					for anim_offset in &entity.animations {
						used_anims.insert(*anim_offset);
						let anim = &self.animations[anim_offset];
						temp1.clear();
						write!(temp1, "{anim_offset:06X}").unwrap();
						anim.save_as(&temp1, &mut output);
					}
				}

				// save splines
				if !entity.splines.is_empty() {
					let mut output = output.push_dir("Splines");
					for spline_offset in &entity.splines {
						used_splines.insert(*spline_offset);
						let spline = &self.splines[spline_offset];
						temp1.clear();
						write!(temp1, "{spline_offset:06x}").unwrap();
						spline.save_as(&temp1, &mut output);
					}
				}

				// save scripts

				let mut script_output = None;
				for offset in &entity.scripts {
					temp2.clear();
					temp3.clear();
					let script = &self.scripts[offset];
					let mut any = false;
					for origin in &script.call_origins {
						if origin.arena_name != arena_name || origin.target_name != entity_name {
							continue;
						}
						if !any {
							any = true;
							temp2.clear();
							temp2.push_str("Called from:\n");
							script_output = Some(output.push_dir("Scripts"));
						}
						temp3.push(&origin.reason);
						writeln!(
							temp2,
							"\t{:06X} {} via {}",
							origin.source_offset, origin.source_name, origin.reason
						)
						.unwrap();
					}
					let Some(output) = script_output.as_mut() else {
						continue;
					};
					temp2.push('\n');
					temp2.push_str(&script.summary);

					temp1.clear();
					write!(temp1, "{offset:06X}").unwrap();
					temp3.sort();
					temp3.dedup();
					for reason in &temp3 {
						temp1.push(' ');
						temp1.push_str(reason);
					}

					output.write(&temp1, "txt", &temp2);
				}
			}
		}

		// unused
		let mut mesh_iter = self
			.meshes
			.iter()
			.filter(|(name, _)| !used_meshes.contains(*name))
			.peekable();
		if mesh_iter.peek().is_some() {
			let mut output = output.push_dir("Unused Meshes");
			for (name, mesh) in mesh_iter {
				if let Some(mesh) = mesh {
					mesh.save_as(name, &mut output);
				} else {
					output.write(name, "txt", "");
				}
			}
		}
	}
}
