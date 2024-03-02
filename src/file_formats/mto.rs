use crate::data_formats::{Animation, Bsp, Mesh, SoundInfo, Wav};
use crate::file_formats::Mti;
use crate::{OutputWriter, Reader};

pub struct Mto<'a> {
	pub filename: &'a str,
	pub arenas: Vec<MtoArena<'a>>,
}

pub struct MtoArena<'a> {
	pub name: &'a str,
	pub animations: Vec<(&'a str, Animation<'a>)>,
	pub meshes: Vec<(&'a str, Mesh<'a>)>,
	pub sounds: Vec<(&'a str, SoundInfo<'a>)>,
	pub bsp: Bsp<'a>,
	pub palette: &'a [u8],
	pub mti: Mti<'a>,
}

impl<'a> Mto<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Self {
		let filesize = reader.u32() + 4;
		assert_eq!(reader.len() as u32, filesize, "filesize does not match");

		let filename = reader.str(12);
		let filesize2 = reader.u32();
		assert_eq!(filesize, filesize2 + 12, "filesizes do not match");
		let num_arenas = reader.u32() as usize;

		let mut arenas = Vec::with_capacity(num_arenas);

		for _ in 0..num_arenas {
			let arena_name = reader.str(8);
			let arena_offset = reader.u32() as usize;

			let mut arena_reader = reader.resized(arena_offset..);
			let asset_filesize = arena_reader.u32() as usize;
			arena_reader.rebase_length(asset_filesize);

			let subfile_offset = arena_reader.u32() as usize;
			let pal_offset = arena_reader.u32() as usize;
			let bsp_offset = arena_reader.u32() as usize;
			let matfile_offset = arena_reader.position();

			let mut animations;
			let mut meshes;
			let mut sounds;
			{
				// parse subfile
				arena_reader.set_position(subfile_offset);
				let subfile_length = arena_reader.u32() as usize;
				let mut subfile_reader = arena_reader.rebased_length(subfile_length);

				let num_animations = subfile_reader.u32() as usize;
				let num_meshes = subfile_reader.u32() as usize;
				let num_sounds = subfile_reader.u32() as usize;

				animations = Vec::with_capacity(num_animations);
				meshes = Vec::with_capacity(num_meshes);
				sounds = Vec::with_capacity(num_sounds);

				for _ in 0..num_animations {
					let name = subfile_reader.str(8);
					let offset = subfile_reader.u32() as usize;

					let anim = Animation::parse(&mut subfile_reader.resized(offset..));
					animations.push((name, anim));
				}
				for _ in 0..num_meshes {
					let name = subfile_reader.str(8);
					let offset = subfile_reader.u32() as usize;

					let mut mesh_reader = subfile_reader.resized(offset..);
					let is_multimesh = mesh_reader.u32();
					assert!(is_multimesh <= 1, "invalid multimesh value");
					let mesh = Mesh::parse(&mut mesh_reader, name, is_multimesh != 0);
					meshes.push((name, mesh));
				}
				for _ in 0..num_sounds {
					let name = subfile_reader.str(12);
					let sound_flags = subfile_reader.u32(); // todo
					let sound_offset = subfile_reader.u32() as usize;
					let sound_length = subfile_reader.u32() as usize;
					let mut sound_reader =
						subfile_reader.resized(sound_offset..sound_offset + sound_length);
					let wav = Wav::parse(&mut sound_reader);
					sounds.push((
						name,
						SoundInfo {
							wav,
							flags: sound_flags,
						},
					));
				}
			}

			// parse palette
			let palette_size = bsp_offset - pal_offset;
			assert_eq!(palette_size, 336);
			arena_reader.set_position(pal_offset);
			let palette = arena_reader.slice(palette_size);

			// parse bsp
			arena_reader.set_position(bsp_offset);
			let bsp = Bsp::parse(&mut arena_reader, arena_name);

			// output matfile
			let mti = Mti::parse(arena_reader.resized(matfile_offset..));

			arenas.push(MtoArena {
				name: arena_name,
				animations,
				meshes,
				sounds,
				bsp,
				palette,
				mti,
			})
		}

		reader.set_position(reader.len() - 12);
		let footer = reader.str(12);
		assert_eq!(filename, footer, "invalid mto footer");

		Mto { filename, arenas }
	}

	pub fn save(&self, output: &mut OutputWriter) {
		for arena in &self.arenas {
			let mut output = output.push_dir(arena.name);

			if !arena.animations.is_empty() {
				let mut output = output.push_dir("animations");
				for (name, anim) in &arena.animations {
					anim.save_as(name, &mut output);
				}
			}
			if !arena.meshes.is_empty() {
				let mut output = output.push_dir("meshes");
				for (name, mesh) in &arena.meshes {
					mesh.save_as(name, &mut output, None, &[]); // todo animations, materials
				}
			}
			if !arena.sounds.is_empty() {
				let mut output = output.push_dir("sounds");
				for (name, sound_info) in &arena.sounds {
					sound_info.save_as(name, &mut output);
				}
				let sound_summary = SoundInfo::create_report_tsv(&arena.sounds);
				output.write("sounds", "tsv", &sound_summary);
			}

			arena.bsp.save_as(arena.name, &mut output, None); // todo materials

			output.write_palette("PAL", arena.palette);

			// todo write full palette

			if !arena.mti.is_empty() {
				let mut output = output.push_dir("materials");
				arena.mti.save(&mut output, None); // todo palette
			}
		}
	}
}
