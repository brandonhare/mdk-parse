#![allow(dead_code)]
#![warn(trivial_casts, trivial_numeric_casts, future_incompatible)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod data_formats;
mod file_formats;
mod gltf;
mod output_writer;
mod reader;
mod vectors;
use data_formats::{Animation, Mesh, Texture, Wav};
use file_formats::{Cmi, Dti, Fti, Mti, Mto, Sni};
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

#[derive(Default)]
#[repr(transparent)]
pub struct NoDebug<T>(T);
impl<T> From<T> for NoDebug<T> {
	fn from(value: T) -> Self {
		Self(value)
	}
}
impl<T> std::fmt::Debug for NoDebug<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("...")
	}
}
impl<T> std::ops::Deref for NoDebug<T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl<T> std::ops::DerefMut for NoDebug<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

fn try_parse_anim(reader: &mut Reader) -> Option<Vec<Texture<'static>>> {
	let mut data = reader.clone();
	let filesize = data.try_u32()? as usize;
	if filesize > data.remaining_len() {
		return None;
	}
	data.rebase_length(filesize);

	let num_frames = data.try_u32()? as usize;
	if num_frames > 1000 {
		return None;
	}
	let mut results = Vec::with_capacity(num_frames);

	for _ in 0..num_frames {
		let offset = data.try_u32()? as usize;
		if offset >= data.remaining_len() {
			return None;
		}
		let mut data = data.clone_at(offset);

		let width = data.try_u16()?;
		let height = data.try_u16()?;
		if width > 5000 || height > 5000 {
			return None;
		}
		let x = data.try_i16()?;
		let y = data.try_i16()?;

		let mut pixels = vec![0; width as usize * height as usize];
		'row_loop: for row in pixels.chunks_exact_mut(width as usize) {
			let mut col_index = 0;
			loop {
				let count = data.try_u8()?;
				match count {
					0..=0x7F => {
						let count = count as usize + 1;
						if col_index + count > row.len() {
							return None;
						}
						let pixels = data.try_slice(count)?;
						row[col_index..col_index + count].copy_from_slice(pixels);
						col_index += count;
					}
					0x80..=0xFD => {
						let count = count as usize - 0x7C;
						if col_index + count > row.len() {
							return None;
						}
						let value = data.try_u8()?;
						row[col_index..col_index + count].fill(value);
						col_index += count;
					}
					0xFE => continue 'row_loop,
					0xFF => break 'row_loop,
				}
			}
		}

		results.push(Texture {
			width,
			height,
			pixels: pixels.into(),
			position: (x, y),
		});
	}

	// mark source reader as read
	reader.skip(filesize + 4);

	Some(results)
}

thread_local! {
	static PALS : RefCell<HashMap<String, Rc<[u8]>>> = Default::default();
}

type PalRef<'a> = Option<&'a [u8]>;
fn get_pal(filename: &str, _name: &str) -> Option<Rc<[u8]>> {
	let temp: String;
	let asset_name = match filename {
		"STREAM.BNI" | "STREAM.MTI" => "STREAM",
		"FALL3D.BNI" => "SPACEPAL",
		filename if filename.starts_with("FALL3D_") => {
			temp = format!("FALLP{}", filename.as_bytes()[7] as char);
			&temp
		}
		"STATS.MTI" => "STATS",
		"TRAVSPRT.BNI" => "LEVEL3",
		filename if filename.starts_with("LEVEL") => &filename[..6],

		filename => filename,
	};
	let result = PALS.with(|pals| pals.borrow().get(asset_name).cloned());
	if result.is_none() {
		//println!("missing {filename}/{name} ({asset_name})");
	}
	result
}
fn set_pal(filename: &str, asset_name: &str, pal: &[u8]) -> Option<Rc<[u8]>> {
	let asset_name = match (asset_name, filename) {
		("PAL", "STREAM.BNI") => "STREAM",
		("PAL", "STATS.BNI") => "STATS",
		_ => asset_name,
	};

	let level_name = if pal.len() < 0x300 {
		filename
			.split_once("O.MTO")
			.map(|(a, _)| a)
			.or_else(|| panic!("failed to get level name from {filename}/{asset_name}"))
	} else {
		None
	};

	PALS.with(|pals| {
		let pals = &mut *pals.borrow_mut();

		let result = if let Some(level_name) = level_name {
			if let Some(level_pal) = pals.get(level_name) {
				let mut result: Rc<[u8]> = Rc::from(level_pal.as_ref());
				Rc::get_mut(&mut result).unwrap()[4 * 16 * 3..4 * 16 * 3 + pal.len()]
					.copy_from_slice(pal);
				result
			} else {
				/*let prev_count = 4 * 16 * 3;
				Rc::from_iter(
					std::iter::repeat(0)
						.take(prev_count)
						.chain(pal.iter().copied())
						.chain(std::iter::repeat(0))
						.take(256 * 3),
				)*/
				//println!("level pal not found for arena pal {level_name}/{asset_name}");
				return None;
			}
		} else {
			Rc::from(pal)
		};

		pals.entry(asset_name.to_owned())
			.and_modify(|_| panic!("duplicate pal {asset_name}!"))
			.or_insert(result.clone());

		Some(result)
	})
}

fn parse_bni(path: &Path) {
	let buf = read_file(path);
	let filename = path.file_name().unwrap().to_str().unwrap();
	let mut reader = Reader::new(&buf);

	let filesize = reader.u32() + 4;
	assert_eq!(reader.len(), filesize as usize, "filesize does not match");
	reader.resize(4..);

	let num_entries = reader.u32();
	#[derive(Debug)]
	struct BniHeader<'a> {
		name: &'a str,
		data: &'a [u8],
	}

	let mut headers = Vec::with_capacity(num_entries as usize);

	for i in 0..num_entries {
		let name = reader.str(12);

		let start_offset = reader.u32() as usize;
		let next_offset = if i + 1 >= num_entries {
			reader.len()
		} else {
			let pos = reader.position();
			reader.skip(12);
			let next_offset = reader.u32() as usize;
			reader.set_position(pos);
			next_offset
		};

		let data = &reader.buf()[start_offset..next_offset];

		if data.len() == 16 * 16 * 3 {
			set_pal(filename, name, data);
		}

		headers.push(BniHeader { name, data });
	}

	let mut output = OutputWriter::new(path, true);

	let mut zooms = Vec::new();

	for &BniHeader { name, data } in headers.iter() {
		// audio
		let mut reader = Reader::new(data);
		if let Some(wav) = Wav::try_parse(&mut reader) {
			output.write(name, "wav", wav.file_data.0);
			continue;
		}

		if name == "INTRO1A" {
			if data.len() < 0x600 {
				println!("intro1a unexpected size {}", data.len());
				continue;
			}
			let lut1 = reader.slice(0x300);
			let lut2 = reader.slice(0x300);

			let width = 600;
			let height = 360;

			let mut result = Vec::with_capacity(width * height);

			loop {
				let count = reader.i8();
				if count <= -1 {
					for _ in count..0 {
						result.push(reader.u8());
					}
					continue;
				}
				if count < 1 {
					break;
				}
				result.resize(result.len() + count as usize, reader.u8());
			}
			assert!(reader.remaining_buf().is_empty());

			output.write_png(
				&format!("{name}_1"),
				width as u32,
				height as u32,
				&result,
				Some(lut1),
			);
			output.write_png(
				&format!("{name}_2"),
				width as u32,
				height as u32,
				&result,
				Some(lut2),
			);
			continue;
		}
		// overlay
		if name == "SNIPERS2" {
			parse_overlay(name, data, &mut output, get_pal(filename, name).as_deref());
			continue;
		}

		if name.starts_with("FALLPU_") {
			let mut names = Vec::new();
			loop {
				let pos = reader.position();
				if reader.u32() == 0 {
					break;
				}
				reader.set_position(pos);
				names.push(reader.str(12));
			}
			output.write(name, "txt", names.join("\n").as_bytes());
			continue;
		}
		if name.starts_with("ZOOM00") {
			zooms.push(parse_zoom(&mut reader));
			continue;
		}

		// image with palette
		if try_parse_image(name, data, &mut output) {
			continue;
		}

		// image without palette
		if data.len() >= 4 {
			let pos = reader.position();
			let width = reader.u16() as usize;
			let height = reader.u16() as usize;

			if width * height == reader.remaining_len() {
				let pal = get_pal(filename, name);
				output.write_png(
					name,
					width as u32,
					height as u32,
					reader.slice(width * height),
					pal.as_deref(),
				);
				continue;
			}
			reader.set_position(pos);
		}

		if let Some(anims) = try_parse_anim(&mut reader) {
			Texture::save_animated(
				&anims,
				name,
				if name == "PICKUPS" { 2 } else { 24 },
				&mut output,
				get_pal(filename, name).as_deref(),
			);
			continue;
		}

		if let Some(multimesh) = Mesh::try_parse(&mut reader.clone(), true) {
			multimesh.save_as(name, &mut output);
			continue;
		}

		if let Some(mesh) = Mesh::try_parse(&mut reader.clone(), false) {
			mesh.save_as(name, &mut output);
			continue;
		}

		if let Some(anim) = Animation::try_parse(&mut reader.clone()) {
			anim.save_as(name, &mut output);
			continue;
		}

		// raw image
		if data.len() == 640 * 480 {
			let pal = get_pal(filename, name);
			output.write_png(name, 640, 480, reader.remaining_slice(), pal.as_deref());
			continue;
		}

		// palette
		if data.len() == 16 * 16 * 3 {
			let pal = reader.remaining_slice();
			output.write_palette(name, pal);
			continue;
		}

		println!("unknown {filename}/{name}");
		output.write(name, "", reader.remaining_slice());
	}

	if !zooms.is_empty() {
		Texture::save_animated(
			&zooms,
			"ZOOM",
			24,
			&mut output,
			get_pal(filename, "ZOOM").as_deref(),
		); // todo palette
	}
}

fn parse_zoom(reader: &mut Reader) -> Texture<'static> {
	let filesize = reader.u32() as usize;
	let end_pos = reader.position() + filesize;
	let mut pixels = Vec::with_capacity(600 * 180);

	while reader.position() < end_pos {
		let num_pixels1 = reader.u32() as usize;
		let pixels1 = reader.slice(num_pixels1 * 4);
		pixels.extend_from_slice(pixels1);

		let num_zeroes = reader.u32() as usize;
		pixels.resize(pixels.len() + num_zeroes * 4, 0);

		let num_pixels2 = reader.u32() as usize;
		let pixels2 = reader.slice(num_pixels2 * 4);
		pixels.extend_from_slice(pixels2);
	}
	assert_eq!(reader.position(), end_pos);

	Texture::new(600, 180, pixels)
}

fn parse_overlay(name: &str, data: &[u8], output: &mut OutputWriter, pal: PalRef) {
	let mut reader = Reader::new(data);
	let filesize = reader.u32();
	reader.resize(4..4 + filesize as usize);

	let width: u32 = 600;
	let height: u32 = 360;

	let mut dest = Vec::with_capacity(width as usize * height as usize);

	loop {
		let index = reader.u16();
		if index & 0x8000 != 0x8000 {
			for _ in 0..4 * index {
				dest.push(reader.u8());
			}
			continue;
		}
		if index & 0xFF00 != 0xFF00 {
			dest.resize_with(dest.len() + (index as usize & 0xFFF), Default::default);
			continue;
		}
		let index = index & 0xFF;
		if index == 0 {
			break;
		}
		for _ in 0..index {
			dest.push(reader.u8());
		}
	}

	output.write_png(name, width, height, &dest, pal);
}

fn try_parse_image(name: &str, buffer: &[u8], output: &mut OutputWriter) -> bool {
	if buffer.len() <= 0x304 {
		return false;
	}
	let mut reader = Reader::new(buffer);
	let palette = reader.slice(0x300);
	let width = reader.u16();
	let height = reader.u16();
	let num_pixels = width as usize * height as usize;
	if reader.remaining_len() != num_pixels {
		return false;
	}
	let pixels = reader.slice(num_pixels);
	output.write_png(name, width as u32, height as u32, pixels, Some(palette));
	true
}

fn parse_lbb(path: &Path) {
	let filename = path.file_name().unwrap().to_str().unwrap();
	let data = read_file(path);

	let mut output = OutputWriter::new(path.parent().unwrap(), false);

	let success = try_parse_image(filename, &data, &mut output);
	assert!(success);
}

#[derive(Debug, Clone)]
struct ImageRef {
	name: String,
	relative_path: PathBuf,
	width: usize,
	height: usize,
}
fn to_string(path: &OsStr) -> String {
	path.to_str().unwrap().to_owned()
}

fn debug_scan_data_for_float_runs(data: &mut Reader) {
	let mut i = data.position();
	let mut floats = Vec::new();
	let mut float_start = 0;
	let mut all_floats = Vec::new();

	while data.position() < data.len() {
		let word32 = data.u32();
		let word16_1 = i16::from_le_bytes(word32.to_le_bytes()[..2].try_into().unwrap());
		let word16_2 = i16::from_le_bytes(word32.to_le_bytes()[2..].try_into().unwrap());
		let float = f32::from_le_bytes(word32.to_le_bytes());
		if float > -1000.0 && float < 1000.0 && !(-0.0001..=0.0001).contains(&float) {
			if floats.is_empty() {
				float_start = i;
			}
			floats.push(float);
			all_floats.push(float);
		//println!("[{i}] {float}");
		} else {
			if !floats.is_empty() {
				println!("[{float_start}..{i}] {} {floats:?}", floats.len());
				floats.clear();
			}
			if word32 < 10000 {
				println!("[{i}] {word32}");
			} else {
				println!("[{i}] {word16_1}, {word16_2}");
			}
		}
		i += 4;
	}
}

fn get_filename(path: &Path) -> &str {
	path.file_name().and_then(OsStr::to_str).unwrap()
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

fn for_all_ext(path: impl AsRef<Path>, ext: &str, func: fn(&Path)) {
	let mut entries: Vec<_> = fs::read_dir(path).unwrap().flatten().collect();
	entries.sort_by(|a, b| {
		fn is_stream(dir: &DirEntry) -> bool {
			let path = dir.path();
			let stem = path.file_stem();
			!stem
				.unwrap_or_default()
				.to_str()
				.unwrap_or_default()
				.contains("STREAM")
		}
		is_stream(a)
			.cmp(&is_stream(b))
			.then(a.path().cmp(&b.path()))
	});
	for entry in entries {
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
	//for_all_ext("assets", "flc", parse_video);
	//for_all_ext("assets", "mve", parse_video);

	println!("done in {:.2?}", start_time.elapsed());
}
