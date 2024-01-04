#![allow(dead_code)]
#![warn(trivial_casts, trivial_numeric_casts, future_incompatible)]
use std::path::Path;

mod data_formats;
mod file_formats;
mod gltf;
mod output_writer;
mod reader;
mod vectors;
use data_formats::image_formats;
use file_formats::{Bni, Cmi, Dti, Fti, Mti, Mto, Sni};
use output_writer::OutputWriter;
use reader::Reader;
use vectors::{Vec2, Vec3};

struct DataFile<'a>(&'a Path, Vec<u8>);
impl<'a> std::ops::Deref for DataFile<'a> {
	type Target = [u8];
	fn deref(&self) -> &Self::Target {
		&self.1
	}
}
#[cfg(feature = "readranges")]
impl<'a> Drop for DataFile<'a> {
	fn drop(&mut self) {
		let buf_range = self.1.as_ptr_range();
		let buf_range = buf_range.start as usize..buf_range.end as usize;
		let read_range = reader::READ_RANGE
			.with(|read_range| read_range.borrow().clone())
			.invert()
			.intersect(buf_range.clone());

		if read_range.is_empty() {
			return;
		}

		let map_bound = |span: &ranges::GenericRange<usize>| -> (usize, usize) {
			use std::ops::Bound as B;
			use std::ops::RangeBounds;
			(
				match span.start_bound() {
					B::Included(start) => start - buf_range.start,
					B::Excluded(start) => start - buf_range.start + 1,
					B::Unbounded => 0,
				},
				match span.end_bound() {
					B::Included(end) => end - buf_range.start + 1,
					B::Excluded(end) => end - buf_range.start,
					B::Unbounded => buf_range.len(),
				},
			)
		};

		let mut first = true;
		for span in read_range.as_slice() {
			let (start, end) = map_bound(span);
			let len = end - start;
			if len <= 3 && self.1[start..end].iter().all(|&b| b == 0) {
				// padding
				continue;
			}
			if first {
				println!("{} ({:06X}..{:06X})", self.0.display(), 0, self.1.len());
				first = false;
			}
			println!("  {start:06X}-{end:06X} ({len:7})");
		}
		if !first {
			println!();
		}
	}
}
fn read_file(path: &Path) -> DataFile {
	let data = std::fs::read(path).unwrap();

	#[cfg(feature = "readranges")]
	{
		let origin = data.as_ptr() as usize;
		reader::READ_RANGE.with(|range| range.borrow_mut().remove(origin..origin + data.len()));
	}

	DataFile(path, data)
}

fn for_all_ext(path: impl AsRef<Path>, ext: &str, func: impl Fn(&Path) + Copy) {
	for entry in std::fs::read_dir(path).unwrap() {
		let entry = entry.unwrap();
		let path = entry.path();
		if entry.file_type().unwrap().is_dir() {
			for_all_ext(path, ext, func);
		} else if path
			.extension()
			.is_some_and(|path| path.eq_ignore_ascii_case(ext))
		{
			func(&path);
		}
	}
}

fn parse_dti(path: &Path) {
	let file = read_file(path);
	let dti = Dti::parse(Reader::new(&file));
	dti.save(&mut OutputWriter::new(path, true));
}

fn parse_bni(path: &Path) {
	let file = read_file(path);
	let bni = Bni::parse(Reader::new(&file));
	bni.save(&mut OutputWriter::new(path, true));
}

fn parse_mto(path: &Path) {
	let file = read_file(path);
	let mto = Mto::parse(Reader::new(&file));
	mto.save(&mut OutputWriter::new(path, true));
}

fn parse_sni(path: &Path) {
	let file = read_file(path);
	let sni = Sni::parse(Reader::new(&file));
	sni.save(&mut OutputWriter::new(path, true));
}

fn parse_mti(path: &Path) {
	let file = read_file(path);
	let mti = Mti::parse(&mut Reader::new(&file));
	mti.save(&mut OutputWriter::new(path, true), None);
}

fn parse_cmi(path: &Path) {
	let file = read_file(path);
	let cmi = Cmi::parse(&mut Reader::new(&file));
	cmi.save(&mut OutputWriter::new(path, true));
}

fn parse_fti(path: &Path) {
	let file = read_file(path);
	let fti = Fti::parse(Reader::new(&file));
	fti.save(&mut OutputWriter::new(path, true));
}

fn parse_lbb(path: &Path) {
	let file = read_file(path);
	let (pal, image) = image_formats::try_parse_palette_image(&mut Reader::new(&file)).unwrap();
	let mut output = OutputWriter::new(path.parent().unwrap(), false);
	let name = path.file_name().and_then(|s| s.to_str()).unwrap();
	image.save_as(name, &mut output, Some(pal));
}

fn parse_video(path: &Path) {
	let mut output_path = OutputWriter::get_output_path(path);
	output_path.set_extension("mp4");
	let _ = std::fs::create_dir_all(output_path.with_file_name(""));
	let result = std::process::Command::new("ffmpeg")
		.args(["-y", "-loglevel", "error", "-i"])
		.args([path, &output_path])
		.output();

	match result {
		Ok(output) if output.status.success() => {}
		Ok(output) => {
			eprintln!("failed to convert {}:", path.display());
			if !output.stdout.is_empty() {
				if let Ok(str) = std::str::from_utf8(&output.stdout) {
					eprintln!("{str}");
				} else {
					eprintln!("{:?}", output.stdout);
				}
			}
			if !output.stderr.is_empty() {
				if let Ok(str) = std::str::from_utf8(&output.stderr) {
					eprintln!("{str}");
				} else {
					eprintln!("{:?}", output.stderr);
				}
			}
		}
		Err(e) => {
			eprintln!("failed to convert {path:?}: {e}");
		}
	}
}

fn main() {
	#[cfg(feature = "readranges")]
	println!("Read ranges enabled");

	let start_time = std::time::Instant::now();

	for_all_ext("assets", "dti", parse_dti);
	for_all_ext("assets", "bni", parse_bni);
	for_all_ext("assets", "mto", parse_mto);
	for_all_ext("assets", "sni", parse_sni);
	for_all_ext("assets", "mti", parse_mti);
	for_all_ext("assets", "cmi", parse_cmi);

	for_all_ext("assets", "fti", parse_fti);
	for_all_ext("assets", "lbb", parse_lbb);
	for_all_ext("assets", "flc", parse_video);
	for_all_ext("assets", "mve", parse_video);

	println!("done in {:.2?}", start_time.elapsed());
}
