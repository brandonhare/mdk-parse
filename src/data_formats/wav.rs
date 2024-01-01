use crate::reader::Reader;
use crate::{NoDebug, OutputWriter};

#[derive(Debug)]
pub struct Wav<'a> {
	pub file_data: NoDebug<&'a [u8]>,
	pub num_channels: u16,
	pub samples_per_second: u32,
	pub bits_per_sample: u16,
	pub duration_secs: f32,
}

fn try_parse_inner<'a>(reader: &mut Reader<'a>) -> Option<Wav<'a>> {
	let start_pos = reader.position();

	if reader.try_slice(4) != Some(b"RIFF") {
		return None;
	}
	let file_length = reader.try_u32()? as usize;
	if reader.remaining_len() < file_length {
		return None;
	}
	if reader.try_slice(4) != Some(b"WAVE") {
		return None;
	}
	if reader.try_slice(4) != Some(b"fmt ") {
		return None;
	}
	let fmt_size = reader.try_u32()? as usize;
	if fmt_size < 16 || reader.remaining_len() < fmt_size {
		return None;
	}
	let fmt_tag = reader.u16();
	let num_channels = reader.u16();
	let samples_per_second = reader.u32();
	let _bytes_per_sec = reader.u32();
	let bytes_per_sample = reader.u16() as usize; // for all channels
	let bits_per_sample = reader.u16(); // for invidial channel sample
	if bits_per_sample % 8 != 0 {
		return None;
	}
	let _ = reader.slice(fmt_size - 16);

	let num_samples = loop {
		let chunk_id = reader.try_str(4)?;
		let chunk_size = reader.try_u32()? as usize;
		if reader.remaining_len() < chunk_size {
			return None;
		}
		if chunk_id == "data" {
			assert_eq!(chunk_size % bytes_per_sample, 0);
			break (chunk_size / bytes_per_sample) as u32;
		} else {
			reader.skip(chunk_size);
		}
	};

	reader.set_position(start_pos);
	let file_data = reader.slice(file_length + 8);

	Some(Wav {
		file_data: NoDebug(file_data),
		num_channels,
		samples_per_second,
		bits_per_sample,
		duration_secs: num_samples as f32 / samples_per_second as f32,
	})
}

impl<'a> Wav<'a> {
	pub fn try_parse(reader: &mut Reader<'a>) -> Option<Wav<'a>> {
		let start_pos = reader.position();
		let result = try_parse_inner(reader);
		if result.is_none() {
			reader.set_position(start_pos);
		}
		result
	}
	pub fn parse(reader: &mut Reader<'a>) -> Wav<'a> {
		Self::try_parse(reader).expect("failed to parse wav file")
	}

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		output.write(name, "wav", self.file_data.0)
	}
}
