use crate::{NoDebug, OutputWriter, Reader};

#[derive(Debug)]
pub struct Wav<'a> {
	pub file_data: NoDebug<&'a [u8]>,
	pub num_channels: u16,
	pub samples_per_second: u32,
	pub bits_per_sample: u16,
	pub duration_secs: f32,
}

#[derive(Debug)]
pub struct SoundInfo<'a> {
	pub wav: Wav<'a>,
	pub flags: u32, // todo
}

impl<'a> Wav<'a> {
	pub fn try_parse(base_reader: &mut Reader<'a>) -> Option<Wav<'a>> {
		let mut reader = base_reader.rebased();

		if reader.try_slice(4) != Some(b"RIFF") {
			return None;
		}
		let file_length = reader.try_u32()? as usize;
		if file_length > reader.remaining_len() {
			return None;
		}
		reader.set_end(file_length + 8);

		if reader.try_slice(8) != Some(b"WAVEfmt ") {
			return None;
		}
		let header_size = reader.try_u32()? as usize;
		if header_size < 16 || reader.remaining_len() < header_size {
			return None;
		}
		let audio_format = reader.u16();
		if audio_format != 1 {
			return None;
		}
		let num_channels = reader.u16();
		let samples_per_second = reader.u32();
		let _bytes_per_sec = reader.u32();
		let bytes_per_sample = reader.u16() as usize; // for all channels
		let bits_per_sample = reader.u16(); // for invidial channel sample
		if bits_per_sample % 8 != 0 {
			return None;
		}
		reader.skip(header_size - 16); // skip extra header data

		let samples = loop {
			let chunk_id = reader.try_str(4)?;
			let chunk_size = reader.try_u32()? as usize;
			let chunk_data = reader.try_slice(chunk_size)?;
			if chunk_id == "data" {
				break chunk_data;
			}
		};

		let num_samples = samples.len() / bytes_per_sample;
		let duration_secs = num_samples as f32 / samples_per_second as f32;

		// return entire file from original reader
		let file_data = base_reader.slice(file_length + 8);

		Some(Wav {
			file_data: NoDebug(file_data),
			num_channels,
			samples_per_second,
			bits_per_sample,
			duration_secs,
		})
	}

	pub fn parse(reader: &mut Reader<'a>) -> Wav<'a> {
		Self::try_parse(reader).expect("failed to parse wav file")
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		output.write(name, "wav", self.file_data.0)
	}
}

impl<'a> SoundInfo<'a> {
	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		self.wav.save_as(name, output)
	}
	pub fn create_report_tsv(sounds: &[(&str, Self)]) -> String {
		use std::fmt::Write;
		let mut summary = String::from(
			"name\tchannels\tsample rate\tbit depth\tduration (s)\tflags 1\tflags 2\n",
		);
		for (name, sound) in sounds {
			writeln!(
				summary,
				"{name}\t{}\t{}\t{}\t{}\t{:X}\t{:X}",
				sound.wav.num_channels,
				sound.wav.samples_per_second,
				sound.wav.bits_per_sample,
				sound.wav.duration_secs,
				sound.flags & 0xFFFF,
				(sound.flags >> 16) & 0xFFFF,
			)
			.unwrap();
		}
		summary
	}
}
