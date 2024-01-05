use std::fmt::Write;

use crate::data_formats::{cmi_bytecode, Animation, Mesh, Spline};
use crate::{OutputWriter, Reader};

pub struct Cmi<'a> {
	pub filename: &'a str,
	pub arenas: Vec<CmiArena<'a>>,
	pub scripts: Vec<(&'a str, String)>,
	pub meshes: Vec<(&'a str, Option<Mesh<'a>>)>,
	pub animations: Vec<(u32, Animation<'a>)>,
	pub splines: Vec<(u32, Spline)>,
}

pub struct CmiArena<'a> {
	pub name: &'a str,
	pub song: &'a str,
}

impl<'a> Cmi<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Self {
		let filesize = reader.u32() as usize;
		assert_eq!(reader.len(), filesize + 4, "filesize does not match");
		reader.rebase();

		let filename = reader.str(12);
		let filesize2 = reader.u32() as usize;
		assert_eq!(filesize, filesize2 + 8, "filesizes do not match");

		let mut script_offsets = cmi_bytecode::CmiOffsets::default();

		let mut scripts = Vec::new();

		// init scripts
		let num_init_scripts = reader.u32() as usize;
		scripts.reserve(num_init_scripts);
		for _ in 0..num_init_scripts {
			let name = reader.pascal_str();
			let init_script_offset = reader.u32();

			let summary = cmi_bytecode::parse_cmi(
				filename,
				name,
				&mut reader.clone_at(init_script_offset as usize),
				&mut script_offsets,
			);
			scripts.push((name, summary));
			// todo
		}

		// meshes
		let num_meshes = reader.u32() as usize;
		let mut meshes = Vec::with_capacity(num_meshes);
		for _ in 0..num_meshes {
			let name = reader.pascal_str();
			let offset = reader.u32() as usize;

			if offset == 0 {
				meshes.push((name, None));
				continue;
			}

			let mut mesh_reader = reader.clone_at(offset);
			let is_multimesh = mesh_reader.u32();
			assert!(is_multimesh <= 1);
			let mesh = Mesh::parse(&mut mesh_reader, is_multimesh != 0);
			meshes.push((name, Some(mesh)));
		}

		// setup scripts
		let num_setup_scripts = reader.u32() as usize;
		scripts.reserve(num_setup_scripts);
		for _ in 0..num_setup_scripts {
			let name = reader.pascal_str();
			let offset = reader.u32();

			let summary = cmi_bytecode::parse_cmi(
				filename,
				name,
				&mut reader.clone_at(offset as usize),
				&mut script_offsets,
			);
			scripts.push((name, summary));
			// todo
		}

		// arenas
		let num_arenas = reader.u32() as usize;
		let mut arenas = Vec::with_capacity(num_arenas);
		scripts.reserve(num_arenas);
		for _ in 0..num_arenas {
			let name = reader.pascal_str();
			let offset = reader.u32();
			let mut arena_reader = reader.clone_at(offset as usize);

			let music1 = arena_reader.pascal_str();
			let music2 = arena_reader.pascal_str();
			assert!(music1.is_empty() || music1 == "NONE");

			let script_offset = arena_reader.u32();
			if script_offset != 0 {
				arena_reader.set_position(script_offset as usize);
				let summary =
					cmi_bytecode::parse_cmi(filename, name, &mut arena_reader, &mut script_offsets);
				scripts.push((name, summary));
				// todo
			}

			arenas.push(CmiArena { name, song: music2 });
		}

		// animations
		script_offsets.anim_offsets.sort();
		script_offsets.anim_offsets.dedup();
		let animations = script_offsets
			.anim_offsets
			.iter()
			.map(|&offset| {
				(
					offset,
					Animation::parse(&mut reader.resized(offset as usize..)),
				)
			})
			.collect();

		// splines
		script_offsets.path_offsets.sort();
		script_offsets.path_offsets.dedup();
		let splines = script_offsets
			.path_offsets
			.iter()
			.map(|&offset| (offset, Spline::parse(&mut reader.clone_at(offset as usize))))
			.collect();

		scripts.sort_by_key(|(name, _)| *name);
		for s in scripts.windows(2) {
			if s[0].0 == s[1].0 {
				println!("{filename} cmi duplicate script name {}", s[0].0);
			}
		}

		Cmi {
			filename,
			arenas,
			scripts,
			meshes,
			animations,
			splines,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		// scripts
		if !self.scripts.is_empty() {
			let mut output = output.push_dir("scripts");
			for (name, script) in &self.scripts {
				output.write(name, "txt", script);
			}
		}

		// meshes
		if !self.meshes.is_empty() {
			let mut output = output.push_dir("meshes");
			let mut mesh_references = String::new();
			for (name, mesh) in &self.meshes {
				if let Some(mesh) = mesh {
					mesh.save_as(name, &mut output);
				} else {
					mesh_references.push_str(name);
					mesh_references.push('\n');
				}
			}
			if !mesh_references.is_empty() {
				output.write("mesh references", "txt", &mesh_references);
			}
		}

		let mut temp = String::new();

		// songs
		if !self.arenas.is_empty() {
			for arena in &self.arenas {
				writeln!(temp, "{}\t{}", arena.name, arena.song).unwrap();
			}
			output.write("songs", "txt", &temp);
		}

		// animations
		if !self.animations.is_empty() {
			let mut output = output.push_dir("animations");
			for (offset, anim) in &self.animations {
				temp.clear();
				write!(temp, "{offset:06X}").unwrap();
				anim.save_as(&temp, &mut output);
			}
		}

		// splines
		if !self.splines.is_empty() {
			temp.clear();
			for (i, (offset, spline)) in self.splines.iter().enumerate() {
				writeln!(temp, "[{i}] {offset:06X} ({})", spline.points.len()).unwrap();
				for point in &spline.points {
					writeln!(
						temp,
						"\t[{:3}] {:.2} {:.2} {:.2}",
						point.t, point.pos1, point.pos2, point.pos3
					)
					.unwrap();
				}
				temp.push('\n');
			}
			output.write("splines", "txt", &temp);
			// todo output better
		}
	}
}
