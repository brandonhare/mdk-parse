use std::borrow::Cow;

use crate::output_writer::OutputWriter;

#[derive(PartialEq, Eq)]
pub struct Texture<'a> {
	pub width: u16,
	pub height: u16,
	pub pixels: Cow<'a, [u8]>,
}

impl<'a> Texture<'a> {
	pub fn save_as(&self, name: &str, output: &mut OutputWriter, palette: Option<&[u8]>) {
		output.write_png(
			name,
			self.width as u32,
			self.height as u32,
			self.pixels.as_ref(),
			palette,
		)
	}

	pub fn save_animated(
		frames: &[Self], name: &str, fps: u16, output: &mut OutputWriter, palette: Option<&[u8]>,
	) {
		let first = &frames[0];

		assert!(frames[1..]
			.iter()
			.all(|frame| frame.width == first.width && frame.height == first.height));

		let mut encoder = output.start_animated_png(
			name,
			first.width as u32,
			first.height as u32,
			fps,
			frames.len() as u32,
			palette,
		);
		for frame in frames {
			encoder.write_image_data(frame.pixels.as_ref()).unwrap();
		}
		encoder.finish().unwrap()
	}
}

impl<'a> std::fmt::Debug for Texture<'a> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Texture")
			.field("width", &self.width)
			.field("height", &self.height)
			.finish()
	}
}
