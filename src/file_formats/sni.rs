use crate::reader::Reader;
use crate::{save_anim, try_parse_anim, Anim, Bsp, OutputWriter, Wav};

#[derive(Debug)]
pub struct Sni<'a> {
	pub sounds: Vec<(&'a str, Wav<'a>, i32)>,
	pub bsps: Vec<(&'a str, Bsp<'a>)>,
	pub anims: Vec<(&'a str, Vec<Anim>)>,
}

impl<'a> Sni<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Sni<'a> {
		let filesize = reader.u32() + 4;
		assert_eq!(reader.len(), filesize as usize, "filesize does not match");
		reader.rebase_start();

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
			let entry_type = reader.i32();
			let entry_offset = reader.u32() as usize;
			let mut entry_size = reader.u32() as usize;
			if entry_size == 0xFFFFFFFF {
				entry_size = reader.clone_at(entry_offset).u32() as usize + 4;
			}

			last_end = last_end.max(entry_offset + entry_size);

			let mut entry_reader = reader.resized(entry_offset..entry_offset + entry_size);

			if entry_type == -1 {
				let anim = try_parse_anim(&mut entry_reader).expect("failed to parse sni anim");
				anims.push((entry_name, anim));
			} else if entry_type == 0 {
				let bsp = Bsp::parse(&mut entry_reader);
				bsps.push((entry_name, bsp));
			} else {
				let wav = Wav::parse(&mut entry_reader);
				sounds.push((entry_name, wav, entry_type));
			}
		}

		last_end = last_end.max(reader.position()).next_multiple_of(4);
		reader.set_position(last_end);
		let filename2 = reader.str(12);
		assert_eq!(filename, filename2, "incorrect sni footer");

		Sni {
			sounds,
			bsps,
			anims,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		let mut sound_summary =
			String::from("name\tchannels\tsample rate\tbit depth\tduration (s)\tentry type\n");
		for (name, sound, entry_type) in self.sounds.iter() {
			sound.save_as(name, output);

			use std::fmt::Write;
			writeln!(
				sound_summary,
				"{name}\t{}\t{}\t{}\t{}\t{entry_type:X}",
				sound.num_channels,
				sound.samples_per_second,
				sound.bits_per_sample,
				sound.duration_secs
			)
			.unwrap();
		}
		output.write("sounds", "tsv", &sound_summary);

		for (name, bsp) in self.bsps.iter() {
			bsp.save_as(name, output);
		}
		for (name, anim) in self.anims.iter() {
			save_anim(name, anim, 30, output, None);
		}
	}
}
