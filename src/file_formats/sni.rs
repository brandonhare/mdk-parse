use crate::reader::Reader;
use crate::{save_anim, try_parse_anim, Anim, Bsp, OutputWriter, Wav};

#[derive(Debug)]
pub struct Sni<'a> {
	pub filename: &'a str,
	pub sounds: Vec<(&'a str, SoundInfo<'a>)>,
	pub bsps: Vec<(&'a str, Bsp<'a>)>,
	pub anims: Vec<(&'a str, Vec<Anim>)>,
}

#[derive(Debug)]
pub struct SoundInfo<'a> {
	pub wav: Wav<'a>,
	pub sound_kind: i32,
}
impl<'a> SoundInfo<'a> {
	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		self.wav.save_as(name, output)
	}
	pub fn create_report_tsv(sounds: &[(&str, Self)]) -> String {
		use std::fmt::Write;
		let mut summary =
			String::from("name\tchannels\tsample rate\tbit depth\tduration (s)\tkind\n");
		for (name, sound) in sounds {
			writeln!(
				summary,
				"{name}\t{}\t{}\t{}\t{}\t{:X}",
				sound.wav.num_channels,
				sound.wav.samples_per_second,
				sound.wav.bits_per_sample,
				sound.wav.duration_secs,
				sound.sound_kind
			)
			.unwrap();
		}
		summary
	}
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
				sounds.push((
					entry_name,
					SoundInfo {
						wav,
						sound_kind: entry_type,
					},
				));
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
		let sound_summary = SoundInfo::create_report_tsv(&self.sounds);
		output.write("sounds", "tsv", &sound_summary);

		if !self.bsps.is_empty() {
			let mut bsp_output = output.push_dir("bsps");
			for (name, bsp) in self.bsps.iter() {
				bsp.save_as(name, &mut bsp_output);
			}
		}

		if !self.anims.is_empty() {
			let mut anim_output = output.push_dir("animations");
			for (name, anim) in self.anims.iter() {
				save_anim(name, anim, 30, &mut anim_output, None);
			}
		}
	}
}
