use std::{
	fs,
	io::BufWriter,
	path::{Path, PathBuf},
};

/// Helper struct to wrangle filenames, folder structures, and PNG stuff
#[derive(Clone)]
pub struct OutputWriter {
	path: PathBuf,
}
impl OutputWriter {
	/// Creates an output writer that points to the corresponding path in the Output folder
	///
	/// e.g. `assets/MISC` becomes `output/MISC`
	pub fn new(path: impl AsRef<Path>, create_output_dir: bool) -> Self {
		let mut output_path =
			Path::new("output").join(path.as_ref().strip_prefix("assets").unwrap());
		if create_output_dir {
			fs::create_dir_all(&output_path).unwrap();
		}
		output_path.push("_");
		OutputWriter { path: output_path }
	}

	#[must_use]
	pub fn push_dir(&self, dir: &str) -> Self {
		let mut result = self.clone();
		result.path.set_file_name(dir);
		fs::create_dir_all(&result.path).unwrap();
		result.path.push("a");
		result
	}

	pub fn set_output_path(&mut self, asset_name: &str, ext: &str) -> &Path {
		let ext = ext.trim_start_matches('.');
		self.path.set_file_name(asset_name);
		self.path.set_extension(ext);
		&self.path
	}

	pub fn write(&mut self, asset_name: &str, ext: &str, data: impl AsRef<[u8]>) {
		let path = self.set_output_path(asset_name, ext);

		if let Err(e) = fs::write(path, data) {
			panic!("failed to write file {}: {e}", path.display());
		};
	}

	pub fn write_png(
		&mut self, asset_name: &str, width: u32, height: u32, pixels: impl AsRef<[u8]>,
		palette: Option<&[u8]>,
	) {
		save_png(
			self.set_output_path(asset_name, "png"),
			pixels.as_ref(),
			width,
			height,
			palette,
			false,
		)
	}
	pub fn write_png_rgba(
		&mut self, asset_name: &str, width: u32, height: u32, pixels: impl AsRef<[u8]>,
		palette: &[u8],
	) {
		save_png(
			self.set_output_path(asset_name, "png"),
			pixels.as_ref(),
			width,
			height,
			Some(palette),
			true,
		)
	}

	pub fn write_palette(&mut self, asset_name: &str, pixels: impl AsRef<[u8]>) {
		save_pal(self.set_output_path(asset_name, "png"), pixels.as_ref())
	}

	#[must_use]
	pub fn start_animated_png(
		&mut self, asset_name: &str, width: u32, height: u32, fps: u16, num_frames: u32,
		palette: Option<&[u8]>,
	) -> png::Writer<impl std::io::Write> {
		self.start_animated_png_inner(asset_name, width, height, fps, num_frames, palette, false)
	}
	#[must_use]
	pub fn start_animated_png_rgba(
		&mut self, asset_name: &str, width: u32, height: u32, fps: u16, num_frames: u32,
		palette: &[u8],
	) -> png::Writer<impl std::io::Write> {
		self.start_animated_png_inner(
			asset_name,
			width,
			height,
			fps,
			num_frames,
			Some(palette),
			true,
		)
	}

	#[allow(clippy::too_many_arguments)]
	pub fn start_animated_png_inner(
		&mut self, asset_name: &str, width: u32, height: u32, fps: u16, num_frames: u32,
		palette: Option<&[u8]>, palette_rgba: bool,
	) -> png::Writer<impl std::io::Write> {
		let path = self.set_output_path(asset_name, "png");
		let mut encoder = setup_png(path, width, height, palette, palette_rgba);
		if num_frames > 1 {
			encoder.set_animated(num_frames, 0).unwrap();
			encoder.set_sep_def_img(false).unwrap();
			encoder.set_frame_delay(1, fps).unwrap();
		}
		encoder.write_header().unwrap()
	}
}

fn save_png(
	path: &Path, data: &[u8], width: u32, height: u32, palette: Option<&[u8]>, palette_rgba: bool,
) {
	debug_assert_eq!(
		width as usize * height as usize,
		data.len(),
		"mismatched image dimensions"
	);

	let palette = match palette {
		Some(pal) if !palette_rgba => {
			// truncate unused colours for no reason
			let max_index = data.iter().copied().max().unwrap() as usize;
			let trimmed_pal = pal.get(..(max_index + 1) * 3);
			debug_assert!(
				trimmed_pal.is_some(),
				"indexed pixel out of range in {}",
				path.display()
			);
			trimmed_pal
		}
		_ => palette,
	};

	let mut encoder = setup_png(path, width, height, palette, palette_rgba)
		.write_header()
		.unwrap();
	encoder.write_image_data(data).unwrap();
	encoder.finish().unwrap();
}
fn save_pal(path: &Path, data: &[u8]) {
	let width: u32 = 16;
	assert!(data.len() % 24 == 0);
	let height = data.len() as u32 / (3 * width);
	let mut encoder = png::Encoder::new(
		BufWriter::new(fs::File::create(path).unwrap()),
		width,
		height,
	);
	encoder.set_color(png::ColorType::Rgb);
	let mut encoder = encoder.write_header().unwrap();
	encoder.write_image_data(data).unwrap();
	encoder.finish().unwrap();
}

fn setup_png<'a>(
	path: &Path, width: u32, height: u32, palette: Option<&'a [u8]>, palette_rgba: bool,
) -> png::Encoder<'a, impl std::io::Write> {
	let mut encoder = png::Encoder::new(
		BufWriter::new(fs::File::create(path).unwrap()),
		width,
		height,
	);
	if let Some(palette) = palette {
		encoder.set_color(png::ColorType::Indexed);
		if !palette_rgba {
			// rgb palette
			assert_eq!(palette.len() % 3, 0);
			encoder.set_palette(palette);
			encoder.set_trns([0].as_slice());
		} else {
			// rgba palette, sorted as rgbrgbrgb...aaa
			assert_eq!(palette.len() % 4, 0);
			let num_entries = palette.len() / 4;
			let (rgb, a) = palette.split_at(num_entries * 3);
			encoder.set_palette(rgb);
			encoder.set_trns(a);
		}
	} else {
		encoder.set_color(png::ColorType::Grayscale);
	}

	encoder
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_writer() {
		assert!(
			!Path::new("output/test_no_dir").exists(),
			"output test directory already exists before starting test"
		);

		let mut writer = OutputWriter::new("assets/test_no_dir/input_file.txt", false);
		assert_eq!(
			writer.path,
			Path::new("output/test_no_dir/input_file.txt/_"),
			"output not created properly"
		);

		assert_eq!(
			writer.set_output_path("output_file", "cool"),
			Path::new("output/test_no_dir/input_file.txt/output_file.cool"),
			"output path not set properly"
		);
		assert_eq!(
			writer.path,
			Path::new("output/test_no_dir/input_file.txt/output_file.cool"),
			"set_output_path did not motify internal path"
		);

		assert_eq!(
			writer.set_output_path("output_no_ext", ""),
			Path::new("output/test_no_dir/input_file.txt/output_no_ext"),
			"output path not set without extension properly"
		);

		assert!(
			!Path::new("output/test_no_dir").exists(),
			"should not have created a directory"
		);
	}
}
