use crate::data_formats::{Bsp, Texture, Wav, image_formats::parse_animation};
use crate::{OutputWriter, Reader};

pub struct Sni<'a> {
	pub filename: &'a str,
	pub sounds: Vec<(&'a str, Wav<'a>)>,
	pub bsps: Vec<(&'a str, Bsp<'a>)>,
	pub anims: Vec<(&'a str, Vec<Texture<'a>>)>,
}

impl<'a> Sni<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Sni<'a> {
		let filesize = reader.u32() + 4;
		assert_eq!(reader.len(), filesize as usize, "filesize does not match");
		reader.rebase();

		let filename = reader.str(12);
		let filesize2 = reader.u32();
		assert_eq!(filesize, filesize2 + 12);
		let num_entries = reader.u32();

		let mut sounds = Vec::new();
		let mut bsps = Vec::new();
		let mut anims = Vec::new();

		let mut last_end = 0;
		for _ in 0..num_entries {
			let entry_name = reader.str(12);
			let entry_type = reader.u32();
			let entry_offset = reader.u32() as usize;
			let mut entry_size = reader.u32() as usize;
			if entry_size == 0xFFFFFFFF {
				entry_size = reader.clone_at(entry_offset).u32() as usize + 4;
			}

			last_end = last_end.max(entry_offset + entry_size);

			let mut entry_reader = reader.resized(entry_offset..entry_offset + entry_size);

			if entry_type == u32::MAX {
				let anim = parse_animation(&mut entry_reader);
				anims.push((entry_name, anim));
			} else if entry_type == 0 {
				let bsp = Bsp::parse(&mut entry_reader);
				bsps.push((entry_name, bsp));
			} else {
				let mut wav = Wav::parse(&mut entry_reader);
				wav.flags = entry_type;
				sounds.push((entry_name, wav));
			}
		}

		last_end = last_end.max(reader.position()).next_multiple_of(4);
		reader.set_position(last_end);
		let filename2 = reader.str(12);
		assert_eq!(filename, filename2, "incorrect sni footer");

		Sni {
			filename,
			sounds,
			bsps,
			anims,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		for (name, sound) in &self.sounds {
			sound.save_as(name, output);
		}
		let sound_summary = Wav::create_report_tsv(&self.sounds);
		output.write("sounds", "tsv", &sound_summary);

		if !self.bsps.is_empty() {
			let mut bsp_output = output.push_dir("bsps");
			for (name, bsp) in self.bsps.iter() {
				bsp.save_as(name, &mut bsp_output);
			}
		}

		if !self.anims.is_empty() {
			let mut anim_output = output.push_dir("animations");
			for (name, frames) in self.anims.iter() {
				Texture::save_animated(frames, name, 30, &mut anim_output, None);
			}
		}
	}
}
