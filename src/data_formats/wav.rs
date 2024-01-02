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
	let file_data = reader.clone_at(start_pos).try_slice(file_length + 8)?;

	if reader.try_slice(4) != Some(b"WAVE") {
		return None;
	}
	if reader.try_slice(4) != Some(b"fmt ") {
		return None;
	}
	let header_size = reader.try_u32()? as usize;
	if header_size < 16 || reader.remaining_len() < header_size {
		return None;
	}
	let audio_format = reader.u16();
	if audio_format != 1 {
		// PCM
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
	let _ = reader.slice(header_size - 16); // skip extra header data

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

	assert!(reader.position() - start_pos <= file_length + 8);

	Some(Wav {
		file_data: NoDebug(file_data),
		num_channels,
		samples_per_second,
		bits_per_sample,
		duration_secs,
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
