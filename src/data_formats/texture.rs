use std::borrow::Cow;

use crate::OutputWriter;

#[derive(Clone, PartialEq, Eq)]
pub struct Texture<'a> {
	pub width: u16,
	pub height: u16,
	pub pixels: Cow<'a, [u8]>,
	pub position: (i16, i16), // for some animations
}

impl<'a> Texture<'a> {
	pub fn new(width: u16, height: u16, pixels: impl Into<Cow<'a, [u8]>>) -> Self {
		let pixels = pixels.into();
		assert_eq!(
			width as usize * height as usize,
			pixels.len(),
			"texture dimensions don't match!"
		);
		Self {
			width,
			height,
			pixels,
			position: (0, 0),
		}
	}

	pub fn clone_ref(&self) -> Texture {
		Texture {
			pixels: self.pixels.as_ref().into(),
			..*self
		}
	}

	pub fn create_png(&self, palette: Option<&[u8]>) -> Vec<u8> {
		let _ = palette;
		todo!()
	}

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
		let num_frames = frames.len();
		assert_ne!(num_frames, 0, "no frames in animation!");

		let mut offset_x = 0;
		let mut offset_y = 0;
		let mut max_x = 0;
		let mut max_y = 0;
		let mut simple = true;
		for frame in frames {
			let x = frame.position.0 as isize;
			let y = frame.position.1 as isize;
			offset_x = offset_x.max(x);
			offset_y = offset_y.max(y);
			max_x = max_x.max(frame.width as isize - x);
			max_y = max_y.max(frame.height as isize - y);

			if simple
				&& (x != 0
					|| y != 0 || frame.width != frames[0].width
					|| frame.height != frames[0].height)
			{
				simple = false;
			}
		}

		let width = (max_x + offset_x) as usize;
		let height = (max_y + offset_y) as usize;

		let mut encoder = output.start_animated_png(
			name,
			width as u32,
			height as u32,
			fps,
			num_frames as u32,
			palette,
		);

		if simple {
			for frame in frames {
				encoder.write_image_data(&frame.pixels).unwrap();
			}
		} else {
			let mut buffer = vec![0; width * height];
			for frame in frames {
				buffer.fill(0);
				let offset_x = (offset_x - (frame.position.0 as isize)) as usize;
				for (dest, src) in buffer
					.chunks_exact_mut(width)
					.skip((offset_y - frame.position.1 as isize) as usize)
					.zip(frame.pixels.chunks_exact(frame.width as usize))
				{
					dest[offset_x..offset_x + src.len()].copy_from_slice(src);
				}
				encoder.write_image_data(&buffer).unwrap();
			}
		}
		encoder.finish().expect("failed to write png file");
	}
}
