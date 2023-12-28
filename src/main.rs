#![allow(dead_code)]
#![allow(unused_variables)] // todo check
#![warn(trivial_casts, trivial_numeric_casts, future_incompatible)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write;
use std::fs::{self, DirEntry};
use std::io::BufWriter;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod cmi_bytecode;
mod dti;
mod gltf;
mod reader;
use reader::Reader;

#[derive(Clone, Debug)]
struct OutputWriter {
	path: PathBuf,
}
impl OutputWriter {
	fn new(path: &Path) -> Self {
		let mut dirname = OutputWriter::get_output_path(path);
		fs::create_dir_all(&dirname).unwrap();
		dirname.push("a");
		OutputWriter { path: dirname }
	}
	fn get_output_path(path: &Path) -> PathBuf {
		Path::new("output").join(path.strip_prefix("assets").unwrap())
	}
	fn new_no_dir(path: &Path) -> Self {
		let mut dirname = OutputWriter::get_output_path(path);
		//fs::create_dir_all(&dirname).unwrap();
		dirname.push("a");
		OutputWriter { path: dirname }
	}
	#[must_use]
	fn push_dir(&self, dir: &str) -> Self {
		let mut result = self.clone();
		result.path.set_file_name(dir);
		fs::create_dir_all(&result.path).unwrap();
		result.path.push("a");
		result
	}
	fn set_output_path(&mut self, asset_name: &str, ext: &str) -> &Path {
		let ext = ext.trim_start_matches('.');
		self.path.set_file_name(asset_name);
		self.path.set_extension(ext);
		&self.path
	}
	fn write(&mut self, asset_name: &str, ext: &str, data: &[u8]) {
		fs::write(self.set_output_path(asset_name, ext), data).expect("failed to write file");
	}
	fn write_no_ext(&mut self, asset_name: &str, data: &[u8]) {
		self.path.set_file_name(asset_name);
		fs::write(&self.path, data).expect("failed to write file");
	}

	fn write_png(
		&mut self, asset_name: &str, pixels: &[u8], width: u32, height: u32, palette: PalRef,
	) {
		save_png(
			self.set_output_path(asset_name, "png"),
			pixels,
			width,
			height,
			palette,
		)
	}

	fn write_palette(&mut self, asset_name: &str, pixels: &[u8]) {
		save_pal(self.set_output_path(asset_name, "png"), pixels)
	}
}

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
			if len <= 3 {
				continue;
			}
			if first {
				println!("{}", self.0.display());
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

fn swizzle(pos: Vec3) -> Vec3 {
	[pos[0], pos[2], -pos[1]]
}
fn swizzle_slice(points: &mut [Vec3]) {
	for point in points {
		*point = swizzle(*point);
	}
}

fn get_bbox(points: &[Vec3]) -> [Vec3; 2] {
	let mut min = [f32::INFINITY; 3];
	let mut max = [f32::NEG_INFINITY; 3];
	for point in points {
		for i in 0..3 {
			min[i] = min[i].min(point[i]);
			max[i] = max[i].max(point[i]);
		}
	}
	[min, max]
}

// puts the reader back to its initial position if the wav read fails
fn try_read_wav<'a>(reader: &mut Reader<'a>) -> Option<&'a [u8]> {
	let start_pos = reader.position();
	if reader.try_slice(4) == Some(b"RIFF") {
		if let Some(length) = reader.try_u32() {
			if reader.try_slice(4) == Some(b"WAVE") {
				reader.set_position(start_pos);
				return reader.try_slice(length as usize + 8);
			}
		}
	}
	reader.set_position(start_pos);
	None
}

fn parse_sni(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let mut reader = Reader::new(&buf);

	let filesize = reader.u32() + 4;
	assert_eq!(reader.len(), filesize as usize, "filesize does not match");

	let name = reader.str(12);
	let filesize2 = reader.u32();
	assert_eq!(filesize, filesize2 + 12);

	let num_entries = reader.u32();

	let mut first_start = None;
	let mut last_end = reader.position() + num_entries as usize * 24;

	let mut output = OutputWriter::new(path);

	for i in 0..num_entries {
		let entry_name = reader.str(12);
		let entry_type = reader.i32();
		let start_offset = reader.u32() as usize + 4;
		let mut file_size = reader.u32() as usize;

		assert!(start_offset >= last_end, "overlapping files");
		assert!(start_offset - last_end < 4, "unknown bytes between files");
		first_start.get_or_insert(start_offset);

		if file_size == 0xFFFFFFFF {
			file_size = u32::from_le_bytes(buf[start_offset..start_offset + 4].try_into().unwrap())
				as usize + 4;
		}

		assert!(start_offset + file_size > last_end, "shorter end");
		last_end = start_offset + file_size;

		let mut entry_reader = reader.resized(start_offset..start_offset + file_size);
		if entry_type == -1 {
			let anims = try_parse_anim(entry_reader.clone()).expect("failed to parse sni anim");
			//assert_eq!(entry_reader.position(), file_size);
			save_anim(
				entry_name,
				&anims,
				24,
				&mut output,
				get_pal(filename, entry_name).as_deref(),
			);
		} else if entry_type == 0 {
			let bsp = try_parse_bsp(&mut entry_reader).expect("failed to parse sni bsp");
			assert_eq!(entry_reader.position(), file_size);
			save_bsp(entry_name, &bsp, &mut output);
		} else if let Some(wav) = try_read_wav(&mut entry_reader) {
			output.write(entry_name, "wav", wav);
		} else {
			// todo what else is in entry_type
			println!("unknown sni entry {entry_name}");
			output.write(entry_name, "", entry_reader.remaining_slice());
		}
	}

	assert_eq!(
		first_start.unwrap(),
		reader.position(),
		"unknown bytes at start of file"
	);
	assert!(reader.position() <= last_end);
	reader.set_position(last_end);
	reader.align(4);
	let filename2 = reader.str(12);
	assert_eq!(name, filename2);
	assert!(
		reader.remaining_len() == 0,
		"unknown bytes at end of file: {filename}"
	);
}

struct Anim {
	width: u16,
	height: u16,
	x: i16,
	y: i16,
	pixels: Vec<u8>,
}

fn try_parse_anim(mut data: Reader) -> Option<Vec<Anim>> {
	let filesize = data.try_u32()? as usize;
	data.resize(data.position()..data.len());
	if filesize > data.len() {
		return None;
	}

	let count = data.try_u32().filter(|n| *n <= 1000)?;
	let offsets = data.try_get_vec::<u32>(count as usize)?;

	let mut results = Vec::new();

	for &o in &offsets {
		let o = o as usize;
		if o >= data.len() {
			return None;
		}
		data.set_position(o);
		let width = data.try_u16()?;
		let height = data.try_u16()?;
		if width > 5000 || height > 5000 {
			return None;
		}
		let [x, y]: [i16; 2] = data.try_get()?;

		let mut pixels = vec![0; width as usize * height as usize];
		'outer: for row in pixels.chunks_exact_mut(width as usize) {
			let mut col_index = 0;
			loop {
				let count = data.try_u8()? as usize;
				if count == 0xFF {
					break 'outer;
				}
				if count == 0xFE {
					break;
				}
				if count < 0x80 {
					let count = count + 1;
					if col_index + count > row.len() {
						return None;
					}
					let pixels = data.try_slice(count)?;
					row[col_index..col_index + count].copy_from_slice(pixels);
					col_index += count;
				} else {
					let count = count - 0x7C;
					if col_index + count > row.len() {
						return None;
					}
					let value = data.try_u8()?;
					row[col_index..col_index + count].fill(value);
					col_index += count;
				}
			}
		}

		results.push(Anim {
			width,
			height,
			x,
			y,
			pixels,
		});
	}

	Some(results)
}

fn parse_mti(path: &Path) {
	let buf = read_file(path);

	let filename = path.file_name().unwrap().to_str().unwrap();

	let pal = get_pal(filename, "");

	let mut output = OutputWriter::new(path);

	parse_mti_data(&mut output, &buf, pal.as_deref())
}
fn parse_mti_data(output: &mut OutputWriter, buf: &[u8], pal: PalRef) {
	let mut reader = Reader::new(buf);
	let filesize = reader.u32() + 4;
	assert_eq!(reader.len(), filesize as usize, "filesize does not match");
	reader.resize(4..);

	let filename = reader.str(12);
	let filesize2 = reader.u32();
	assert_eq!(filesize, filesize2 + 12, "filesizes do not match");
	let num_entries = reader.u32();

	let mut pen_entries = Vec::new();

	for i in 0..num_entries {
		let name = reader.str(8);
		let flags = reader.u32();

		if flags == 0xFFFFFFFF {
			let b = reader.i32();
			let c = reader.u32();
			let d = reader.u32();
			assert_eq!(c, 0);
			assert_eq!(d, 0);
			pen_entries.push((name, b));
		} else {
			let b = reader.f32();
			let c = reader.f32();
			let start_offset = reader.u32() as usize;
			let mut data = reader.clone_at(start_offset);

			let mut num_frames = 1;
			let flags_mask = flags & 0x30000;
			let flags2 = flags & 0xFFFF;

			if flags_mask != 0 {
				num_frames = data.u32() as usize;
			}
			let width = data.u16() as u32;
			let height = data.u16() as u32;

			let frame_size = (width * height) as usize;
			if num_frames == 1 || flags_mask == 0x20000 {
				let pixels = data.slice(frame_size);
				output.write_png(name, pixels, width, height, pal);
			} else {
				let anims: Vec<Anim> = (0..num_frames)
					.map(|_| {
						let pixels = data.slice(frame_size);
						Anim {
							x: 0,
							y: 0,
							width: width as u16,
							height: height as u16,
							pixels: pixels.to_owned(),
						}
					})
					.collect();
				save_anim(name, &anims, 24, output, pal);
			}

			if flags_mask == 0x20000 {
				assert!(data.u32() == 0);
				let mut data =
					data.resized(data.position()..(data.position() + 7108).min(data.len()));
				let offsets = data.get_vec::<u32>(num_frames * 2);
				let metadata_offsets = &offsets[..num_frames];
				let pixel_offsets = &offsets[num_frames..];
				for (i, (&meta_offset, &pixel_offset)) in
					metadata_offsets.iter().zip(pixel_offsets).enumerate()
				{
					data.set_position(meta_offset as usize);
					let meta = data.slice(pixel_offset as usize - meta_offset as usize);
					let next_meta_offset = metadata_offsets
						.get(i + 1)
						.map(|n| *n as usize)
						.unwrap_or(data.len());
					debug_assert_eq!(data.position(), pixel_offset as usize);
					let pixels = data.slice(next_meta_offset - pixel_offset as usize);
					output.write(&format!("{name}_{i}_meta"), "", meta);
					output.write(&format!("{name}_{i}_pixels"), "", pixels);
				}
			}
		}
	}

	if !pen_entries.is_empty() {
		output.write(
			"pens",
			"txt",
			String::from_iter(
				pen_entries
					.iter()
					.map(|(name, id)| format!("{name:8}: {id}\n")),
			)
			.as_bytes(),
		);
	}

	reader.set_position(reader.len() - 12);
	let footer_name = reader.str(12);
	assert_eq!(
		filename, footer_name,
		"mti had mismatched header and footer names"
	);
}

thread_local! {
	static PALS : RefCell<HashMap<String, Rc<[u8]>>> = Default::default();
}

type PalRef<'a> = Option<&'a [u8]>;
fn get_pal(filename: &str, name: &str) -> Option<Rc<[u8]>> {
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

	let mut output = OutputWriter::new(path);

	let mut zooms = Vec::new();

	for (i, &BniHeader { name, data }) in headers.iter().enumerate() {
		// audio
		let mut reader = Reader::new(data);
		if let Some(wav) = try_read_wav(&mut reader) {
			output.write(name, "wav", wav);
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
				&result,
				width as u32,
				height as u32,
				Some(lut1),
			);
			output.write_png(
				&format!("{name}_2"),
				&result,
				width as u32,
				height as u32,
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
			zooms.push(parse_zoom(name, data, &mut output));
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
					reader.slice(width * height),
					width as u32,
					height as u32,
					pal.as_deref(),
				);
				continue;
			}
			reader.set_position(pos);
		}

		if let Some(anims) = try_parse_anim(reader.clone()) {
			save_anim(
				name,
				&anims,
				if name == "PICKUPS" { 2 } else { 24 },
				&mut output,
				get_pal(filename, name).as_deref(),
			);
			continue;
		}

		if let Some(multimesh) = try_parse_multimesh(&mut reader.clone()) {
			save_multimesh(name, &multimesh, &mut output);
			continue;
		}

		if let Some(mesh) = try_parse_mesh(&mut reader.clone(), true) {
			save_mesh(name, &mesh, &[], &mut output);
			continue;
		}

		if let Some(anim) = try_parse_alienanim(name, reader.clone()) {
			save_alienanim(name, &anim, &mut output);
			continue;
		}

		// raw image
		if data.len() == 640 * 480 {
			let pal = get_pal(filename, name);
			output.write_png(name, reader.remaining_slice(), 640, 480, pal.as_deref());
			continue;
		}

		// palette
		if data.len() == 16 * 16 * 3 {
			output.path.set_file_name(name);
			output.path.set_extension("png");
			save_pal(&output.path, reader.remaining_slice());
			continue;
		}

		println!("unknown {filename}/{name}");
		output.write(name, "", reader.remaining_slice());
	}

	if !zooms.is_empty() {
		save_zoom(
			"ZOOM",
			&zooms,
			&mut output,
			get_pal(filename, "ZOOM").as_deref(),
		); // todo palette
	}
}

fn parse_zoom(name: &str, data: &[u8], output: &mut OutputWriter) -> Vec<u8> {
	let mut reader = Reader::new(data);
	let filesize = reader.u32();
	reader.resize(4..4 + filesize as usize);

	let mut result = Vec::new();

	while reader.position() < reader.len() {
		let count = reader.u32();
		let data1 = reader.slice(count as usize * 4);
		result.extend_from_slice(data1);

		let a = reader.u32();
		result.resize(result.len() + a as usize * 4, 0);

		let b = reader.u32();
		if a != 0 || b != 0 {
			let data2 = reader.slice(b as usize * 4);
			result.extend_from_slice(data2);
		}
	}

	result
}

fn save_zoom(name: &str, data: &[Vec<u8>], output: &mut OutputWriter, pal: PalRef) {
	let anims: Vec<_> = data
		.iter()
		.cloned()
		.map(|pixels| Anim {
			width: 600,
			height: 180,
			x: 0,
			y: 0,
			pixels,
		})
		.collect();
	save_anim(name, &anims, 24, output, pal);
}

fn parse_overlay(name: &str, data: &[u8], output: &mut OutputWriter, pal: PalRef) {
	let mut reader = Reader::new(data);
	let filesize = reader.u32();
	reader.resize(4..4 + filesize as usize);

	let width = 600;
	let height = 360;

	let mut dest = Vec::with_capacity(width * height);

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

	output.write_png(name, &dest, width as u32, height as u32, pal);
}

fn try_parse_image(name: &str, data: &[u8], output: &mut OutputWriter) -> bool {
	if data.len() <= 0x304 {
		return false;
	}
	let lut = &data[..0x300];
	let width = u16::from_le_bytes(data[0x300..0x302].try_into().unwrap()) as usize;
	let height = u16::from_le_bytes(data[0x302..0x304].try_into().unwrap()) as usize;
	let pixel_data = &data[0x304..];
	if pixel_data.len() != width * height {
		return false;
	}

	// mark as read
	#[cfg(feature = "readranges")]
	let _ = Reader::new(data).slice(0x300 + 4 + width * height);

	output.write_png(name, pixel_data, width as _, height as _, Some(lut));
	true
}

fn parse_mto(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let mut data = Reader::new(&buf);

	let filesize = data.u32() + 4;
	assert_eq!(data.len() as u32, filesize, "filesize does not match");

	let mto_name = data.str(12);
	let filesize2 = data.u32();
	assert_eq!(filesize, filesize2 + 12, "filesizes do not match");
	let num_arenas = data.u32() as u64;

	let mut new_path = path.to_owned();
	for _ in 0..num_arenas {
		let arena_name = data.str(8);
		let arena_offset = data.u32() as usize;

		new_path.push(arena_name);
		let mut output = OutputWriter::new(&new_path);
		new_path.pop();

		let mut asset_reader = data.resized(arena_offset..);
		let asset_filesize = asset_reader.u32();
		asset_reader.resize(4..asset_filesize as usize);

		let subfile_offset = asset_reader.u32() as usize;
		let pal_offset = asset_reader.u32() as usize;
		let bsp_offset = asset_reader.u32() as usize;

		let matfile_len = asset_reader.u32() as usize;
		let matfile_name = asset_reader.str(12);
		let matfile_data = &asset_reader.buf()[12..8 + matfile_len + 8];

		{
			// parse subfile
			asset_reader.set_position(subfile_offset);
			let offset1_len = asset_reader.u32() as usize;
			let offset1_data = &asset_reader.remaining_buf()[..offset1_len];
			parse_mto_subthing(arena_name, offset1_data, &mut output);
		}

		{
			// parse pal
			asset_reader.set_position(pal_offset);
			let pal_size = bsp_offset - pal_offset;
			assert_eq!(pal_size, 336);
			let pal_data = asset_reader.slice(pal_size);
			let pal_full = set_pal(filename, arena_name, pal_data);
			output.set_output_path("PAL", "PNG");
			save_pal(&output.path, pal_data);
			if let Some(pal) = pal_full {
				output.set_output_path("PAL_full", "PNG");
				save_pal(&output.path, &pal);
			}
		}

		{
			// parse bsp
			asset_reader.set_position(bsp_offset);
			let bsp = parse_bsp(&mut asset_reader);
			save_bsp(arena_name, &bsp, &mut output);
		}

		{
			// output matfile
			parse_mti_data(
				&mut output,
				matfile_data,
				get_pal(arena_name, arena_name).as_deref(),
			);
		}
	}
}

fn parse_mto_subthing(arena_name: &str, buf: &[u8], output: &mut OutputWriter) {
	let mut data = Reader::new(buf);

	let num_animations = data.u32();
	let num_meshes = data.u32();
	let num_sounds = data.u32();

	//output.write(arena_name, "thing", data.buf());

	let animations: Vec<_> = (0..num_animations)
		.map(|_| (data.str(8), data.u32()))
		.collect();
	let mesh_headers: Vec<_> = (0..num_meshes).map(|_| (data.str(8), data.u32())).collect();
	let sound_headers: Vec<_> = (0..num_sounds)
		.map(|_| (data.str(12), data.i16(), data.i16(), data.u32(), data.u32()))
		.collect();

	// todo remove this debug thing
	let mut all_offsets: Vec<u32> = animations
		.iter()
		.chain(mesh_headers.iter())
		.map(|(_, o)| *o)
		.chain(sound_headers.iter().map(|s| s.3))
		.chain(once(buf.len() as u32))
		.collect();
	all_offsets.sort();

	// animations?
	for (i, &(name, offset)) in animations.iter().enumerate() {
		let end = all_offsets[all_offsets.iter().position(|o| *o == offset).unwrap() + 1];
		let Some(anim) = try_parse_alienanim(name, data.resized(offset as usize..end as usize))
		else {
			eprintln!("failed to parse anim {i} {arena_name}/{name}");
			output.write(name, "", &data.buf()[offset as usize..end as usize]);
			continue;
		};
		save_alienanim(name, &anim, output);
	}

	// meshes
	for &(name, offset) in &mesh_headers {
		data.set_position(offset as usize);

		let is_multimesh = data.u32();

		if is_multimesh != 0 {
			let result = try_parse_multimesh(&mut data).expect("failed to parse multimesh");
			save_multimesh(name, &result, output);
		} else {
			let mesh = parse_mesh(&mut data, true);
			save_mesh(name, &mesh, &[], output);
		};
	}

	// sounds
	for &(sound_name, looping, b, sound_offset, sound_length) in &sound_headers {
		assert_eq!(b, 0x7FFF);
		assert!(looping == 0 || looping == 1);

		let mut reader =
			Reader::new(&buf[sound_offset as usize..sound_offset as usize + sound_length as usize]);

		let data = try_read_wav(&mut reader).expect("invalid wav file!");
		output.write(sound_name, "wav", data);
	}
}

#[derive(Debug, Default)]
struct Submesh<'a> {
	mesh: Mesh<'a>,
	name: String,
	origin: Vec3,
}
#[derive(Debug, Default)]
struct Multimesh<'a> {
	textures: Vec<String>,
	meshes: Vec<Submesh<'a>>,
	bbox: [Vec3; 2],
	reference_points: Vec<Vec3>,
}

#[derive(Debug)]
struct BspPlane {
	normal: Vec3,
	dist: f32,
	plane_index_behind: i16,
	plane_index_front: i16,
	tris_front_count: u16,
	tris_front_index: u16,
	tris_back_count: u16,
	tris_back_index: u16,
	zeroes: [u32; 4],
}

struct Bsp<'a> {
	planes: Vec<BspPlane>,
	mesh: Mesh<'a>,
}

fn try_parse_bsp<'a>(data: &mut Reader<'a>) -> Option<Bsp<'a>> {
	let num_materials = data.try_u32()?;
	if num_materials > 500 {
		return None;
	}
	let textures = (0..num_materials)
		.map(|_| data.try_str(10))
		.collect::<Option<Vec<&str>>>()?;
	data.try_align(4)?;

	let num_planes = data.try_u32()? as usize;
	if num_planes > 10000 {
		return None;
	}
	let mut planes = Vec::with_capacity(num_planes);
	for _ in 0..num_planes {
		let result = BspPlane {
			normal: data.try_vec3()?,
			dist: data.try_f32()?,
			plane_index_behind: data.try_i16()?,
			plane_index_front: data.try_i16()?,
			tris_front_count: data.try_u16()?,
			tris_front_index: data.try_u16()?,
			tris_back_count: data.try_u16()?,
			tris_back_index: data.try_u16()?,
			zeroes: data.try_get()?,
		};
		if result.plane_index_behind < -1
			|| result.plane_index_behind as isize > num_planes as isize
			|| result.plane_index_front < -1
			|| result.plane_index_front as isize > num_planes as isize
		{
			return None;
		}

		if (result.normal.iter().map(|f| f * f).sum::<f32>() - 1.0).abs() > 0.0001 {
			return None;
		}
		if result.zeroes != [0; 4] {
			return None;
		}
		planes.push(result);
	}

	let num_tris = data.try_u32()? as usize;
	let tris = try_parse_mesh_tris(data, num_tris)?;

	let num_verts = data.try_u32()? as usize;
	if num_verts > 10000 {
		return None;
	}
	let mut verts = data.try_get_vec::<Vec3>(num_verts)?;
	swizzle_slice(&mut verts);

	let num_things = data.try_u32()?;
	if num_things > 10000 {
		return None;
	}
	let things = data.try_slice(num_things as usize)?;
	if things.iter().any(|c| *c != 255) {
		return None;
	}
	//assert_eq!(data.position(), data.len());

	Some(Bsp {
		planes,
		mesh: Mesh {
			textures,
			bbox: get_bbox(&verts),
			verts,
			tris,
			reference_points: Vec::new(),
		},
	})
}

fn parse_bsp<'a>(data: &mut Reader<'a>) -> Bsp<'a> {
	try_parse_bsp(data).expect("failed to parse bsp!")
}

struct AlienAnim<'a> {
	speed: f32,
	//np: u32,
	//nf: u32,
	frames: Vec<AlienAnimFrame>,
	parts: Vec<AlienAnimPart<'a>>,
}
struct AlienAnimFrame {
	vec: Vec3,
	data: Vec<Vec3>,
}
struct AlienAnimPart<'a> {
	name: &'a str,
	vecs: Vec<Vec3>,
	data: AlienAnimPartType,
}
enum AlienAnimPartType {
	Vecs(Vec<AlienAnimPartRow>),
	Transforms(Vec<[[f32; 4]; 3]>),
}
struct AlienAnimPartRow {
	index: u16,
	triples: Vec<Vec3>,
}

fn try_parse_alienanim<'a>(name: &str, mut data: Reader<'a>) -> Option<AlienAnim<'a>> {
	let speed = data.try_f32()?;
	data.resize(data.position()..);
	let num_parts = data.try_u32()? as usize;
	let num_frames = data.try_u32()? as usize;

	if num_parts > 1000 || num_frames > 1000 {
		return None;
	}

	let offsets = data.try_get_vec::<u32>(num_parts)?;
	if offsets.iter().any(|o| *o as usize >= data.len()) {
		return None;
	}

	// someAnimVector (local space)
	let frame_vectors = data.try_get_vec::<Vec3>(num_frames)?;

	let frames_data_count = data.try_u32().filter(|n| *n <= 10000)? as usize;
	// part locations ?
	let frames_data = data.try_get_vec::<Vec3>(frames_data_count * num_frames)?;

	let frames: Vec<AlienAnimFrame> = if frames_data_count * num_frames != 0 {
		frame_vectors
			.iter()
			.zip(frames_data.chunks_exact(frames_data_count))
			.map(|(&vec, data)| AlienAnimFrame {
				vec,
				data: data.to_owned(),
			})
			.collect()
	} else {
		frame_vectors
			.iter()
			.map(|&vec| AlienAnimFrame {
				vec,
				data: Vec::new(),
			})
			.collect()
	};

	let mut parts = Vec::new();

	for (i, &offset) in offsets.iter().enumerate() {
		let next = offsets.get(i + 1).copied().unwrap_or(data.len() as u32);
		let mut data = data.resized_pos(..next as usize, offset as usize);

		let name = data.try_str(12)?;
		let count = data.try_u32().filter(|c| *c < 1000)? as usize;
		let scale = data.try_f32()?;
		let part = if scale == 0.0 {
			let scale_vec = (0x8000 >> (data.try_u8()? & 0x3F)) as f32;
			let scale_pos = (0x8000 >> (data.try_u8()? & 0x3F)) as f32;
			let vecs = data.try_get_vec::<Vec3>(count)?;

			// let anim_data = data.try_get_vec::<[i16; 12]>(num_frames)?;
			let transforms = (0..num_frames)
				.map(|_| {
					let mut result = [[0.0; 4]; 3];
					for row in &mut result {
						for value in &mut row[..3] {
							*value = data.try_i16()? as f32 / scale_vec;
						}
						row[3] = data.try_i16()? as f32 / scale_pos;
					}
					Some(result)
				})
				.collect::<Option<Vec<_>>>()?;
			// todo verify

			AlienAnimPart {
				name,
				vecs,
				data: AlienAnimPartType::Transforms(transforms),
			}
		} else {
			let vecs = data.try_get_vec::<Vec3>(count)?;
			let mut rows = Vec::new();
			for j in 0..=num_frames {
				if j == num_frames {
					println!("too many rows");
					return None;
				}
				let index = data.try_u16()?;
				if index == 0xFFFF {
					break;
				}

				//let triples = data.try_get_vec::<[i8; 3]>(count)?;
				let triples = (0..count)
					.map(|_| {
						data.try_get::<[i8; 3]>()
							.map(|ns| ns.map(|n| n as f32 * scale))
					})
					.collect::<Option<Vec<Vec3>>>()?;
				// todo verify

				rows.push(AlienAnimPartRow { index, triples });
			}
			AlienAnimPart {
				name,
				vecs,
				data: AlienAnimPartType::Vecs(rows),
			}
		};

		/*/
		if data.position() + 4 < next as usize {
			println!("animation data left over");
			return None;
		}
		*/

		parts.push(part);
	}

	Some(AlienAnim {
		speed,
		frames,
		parts,
	})
}

fn save_alienanim(name: &str, anim: &AlienAnim, output: &mut OutputWriter) {
	let mut result = format!(
		"speed: {}\nframe data count: {}\n\nframes: ({}):\n",
		anim.speed,
		anim.frames
			.first()
			.map(|t| t.data.len())
			.unwrap_or_default(),
		anim.frames.len()
	);
	for (i, frame) in anim.frames.iter().enumerate() {
		writeln!(result, "\t[frame {i}] vec: {:?}", frame.vec).unwrap();
		for (j, &data) in frame.data.iter().enumerate() {
			writeln!(result, "\t\t[data {j}] {data:?}").unwrap();
		}
	}

	writeln!(result, "\nparts ({}):", anim.parts.len()).unwrap();
	for (i, part) in anim.parts.iter().enumerate() {
		writeln!(
			result,
			"\t[part {i}] name: {}\n\t\tvecs ({}):",
			part.name,
			part.vecs.len()
		)
		.unwrap();
		for (j, vec) in part.vecs.iter().enumerate() {
			writeln!(result, "\t\t\t[vec {j}]: {vec:?}").unwrap();
		}

		match &part.data {
			AlienAnimPartType::Vecs(rows) => {
				writeln!(result, "\t\tdata Vecs ({}):", rows.len()).unwrap();
				for (j, row) in rows.iter().enumerate() {
					assert!(
						(row.index as usize) < anim.frames.len(),
						"alienanim {name} row index {} out of range {}",
						row.index,
						anim.frames.len()
					);
					writeln!(
						result,
						"\t\t\t[frame {}] triples ({}):",
						row.index,
						row.triples.len(),
					)
					.unwrap();
					for (k, triple) in row.triples.iter().enumerate() {
						writeln!(result, "\t\t\t\t[triple {k:2}] {triple:3?}").unwrap();
					}
				}
			}
			AlienAnimPartType::Transforms(transforms) => {
				writeln!(result, "\t\tdata Transforms ({}):", transforms.len()).unwrap();
				for (j, transform) in transforms.iter().enumerate() {
					writeln!(result, "\t\t\t[frame {j}] transform: {transform:?}",).unwrap()
				}
			}
		}
	}

	output.write(name, "anim.txt", result.as_bytes());
}

fn parse_lbb(path: &Path) {
	let filename = path.file_name().unwrap().to_str().unwrap();
	let data = read_file(path);

	let mut output = OutputWriter::new_no_dir(path);
	output.path.pop();

	let success = try_parse_image(filename, &data, &mut output);
	assert!(success);
}

struct PathDataEntry {
	t: i32,
	pos1: Vec3,
	pos2: Vec3,
	pos3: Vec3,
}

fn read_path_data(mut reader: Reader) -> Vec<PathDataEntry> {
	let count = reader.u32();
	(0..count)
		.map(|_| {
			let t = reader.i32();
			let pos1 = reader.vec3();
			let pos2 = reader.vec3();
			let pos3 = reader.vec3();
			PathDataEntry {
				t,
				pos1,
				pos2,
				pos3,
			}
		})
		.collect()
}

fn parse_cmi(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let mut data = Reader::new(&buf);

	let filesize = data.u32() + 4;
	assert_eq!(data.len(), filesize as usize, "filesize does not match");
	data.resize(4..);

	let name = data.str(12);
	let filesize2 = data.u32();
	assert_eq!(filesize, filesize2 + 12, "filesizes do not match");

	let mut init_entries = Vec::new();
	let mut mesh_entries = Vec::new();
	let mut setup_entries = Vec::new();
	let mut arena_entries = Vec::new();

	// read offsets
	for entries in [
		&mut init_entries,
		&mut mesh_entries,
		&mut setup_entries,
		&mut arena_entries,
	] {
		let count = data.u32();
		entries.extend((0..count).map(|_| {
			let name = data.pascal_str();
			let offset = data.u32();
			if offset == 0 {
				// mesh entries
				(name, data.resized(..0))
			} else {
				let data = data.clone_at(offset as usize);
				(name, data)
			}
		}));
	}

	let mut output = OutputWriter::new(path);

	let mut cmi_offsets = cmi_bytecode::CmiOffsets::default();

	// process init entries
	{
		let mut init_output = None;
		for (name, mut data) in init_entries {
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data, &mut cmi_offsets);
			let init_output = init_output.get_or_insert_with(|| output.push_dir("init"));
			init_output.write(name, "txt", cmi.as_bytes());
		}
	}

	// process mesh entries
	{
		let mut mesh_output = output.push_dir("meshes");
		let mut empty = String::new();
		for (name, mut data) in mesh_entries {
			if data.is_empty() {
				empty.push_str(name);
				empty.push('\n');
				continue;
			}
			let mesh_type = data.i32();
			if mesh_type == 0 {
				let mesh = parse_mesh(&mut data, true);
				save_mesh(name, &mesh, &[], &mut mesh_output);
			} else if mesh_type == 1 {
				let mesh = try_parse_multimesh(&mut data).expect("failed to parse multimesh");
				save_multimesh(name, &mesh, &mut mesh_output);
			} else {
				panic!("invalid mesh type for {name} in {filename}: {mesh_type}");
			}
		}
		if !empty.is_empty() {
			mesh_output.write("others", "txt", empty.as_bytes());
		}
	}

	// process setup entries
	{
		let mut setup_output = output.push_dir("setup");
		for (name, mut data) in setup_entries {
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data, &mut cmi_offsets);
			setup_output.write(name, "txt", cmi.as_bytes());
		}
	}

	// process arena_entries
	{
		let mut arena_output = output.push_dir("arenas");
		for (name, mut data) in arena_entries {
			let str1 = data.pascal_str();
			let music = data.pascal_str();
			let offset = data.u32() as usize;

			data.set_position(offset);
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data, &mut cmi_offsets);
			arena_output.write(
				name,
				"txt",
				format!("name: {name}, music 1: \"{str1}\", music 2: \"{music}\"\n\n{cmi}")
					.as_bytes(),
			);
		}
	}

	// save anims
	{
		let mut anim_output = output.push_dir("anims");
		let anim_offsets = &mut cmi_offsets.anim_offsets;
		anim_offsets.sort_unstable();
		anim_offsets.dedup();
		for &offset in anim_offsets.iter() {
			let name = format!("{offset:06X}");
			let anim_reader = data.resized(offset as usize..);
			if let Some(anim) = try_parse_alienanim(&name, anim_reader) {
				save_alienanim(&name, &anim, &mut anim_output)
			} else {
				eprintln!("{filename}/{name} failed to parse alienanim at offset {offset:06X}");
			}
		}
	}
	// save paths
	{
		let path_offsets = &mut cmi_offsets.path_offsets;
		path_offsets.sort_unstable();
		path_offsets.dedup();
		let mut summary = String::new();
		for &offset in path_offsets.iter() {
			if offset == 0 {
				eprintln!("invalid path offset in {filename}");
				continue;
			}
			let path = read_path_data(data.clone_at(offset as usize));
			writeln!(summary, "path {offset:06X} ({})", path.len()).unwrap();
			for row in &path {
				writeln!(
					summary,
					"\t[{:3}] {:?}, {:?}, {:?}",
					row.t, row.pos1, row.pos2, row.pos3
				)
				.unwrap();
			}
			summary.push('\n');
		}
		output.write("paths", "txt", summary.as_bytes());
	}
}

struct FontLetter<'a> {
	code: u8,
	width: u8,
	height: u8,
	pixels: &'a [u8],
}
fn parse_font_letters(mut data: Reader) -> Vec<FontLetter> {
	let mut result = Vec::with_capacity(256);
	for i in 0..=255 {
		let offset = data.u32();
		if offset == 0 {
			continue;
		}
		let mut data = data.clone_at(offset as usize);

		let height_base = data.i8();
		let height_offset = data.i8();
		let height = (height_base + height_offset + 1) as u8;
		let width = data.u8();

		let pixels = data.slice(width as usize * height as usize);

		result.push(FontLetter {
			code: i,
			width,
			height,
			pixels,
		});
	}
	result
}

fn save_font_grid(name: &str, letters: &[FontLetter], output: &mut OutputWriter, pal: PalRef) {
	let (cell_width, cell_height, max_code) =
		letters.iter().fold((0, 0, 0), |(w, h, c), letter| {
			(
				w.max(letter.width as usize),
				h.max(letter.height as usize),
				c.max(letter.code),
			)
		});
	assert!(
		cell_width > 0 && cell_height > 0 && max_code > 0,
		"invalid font dimensions!"
	);

	let cells_per_row = 16;
	let num_rows = (max_code as usize).div_ceil(cells_per_row);

	let row_width = cell_width * cells_per_row;
	let row_stride = row_width * cell_height;

	let mut result = vec![0; num_rows * row_stride];

	for letter in letters {
		let col_index = letter.code as usize % cells_per_row;
		let row_index = letter.code as usize / cells_per_row;
		let result = &mut result[row_index * row_stride + col_index * cell_width..];
		for (dest, src) in result
			.chunks_mut(row_width)
			.zip(letter.pixels.chunks_exact(letter.width as usize))
		{
			dest[..letter.width as usize].copy_from_slice(src);
		}
	}

	output.write_png(
		name,
		&result,
		row_width as u32,
		(num_rows * cell_height) as u32,
		pal,
	)
}

fn parse_fti(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let mut data = Reader::new(&buf);

	let filesize = data.u32() + 4;
	assert_eq!(data.len(), filesize as usize, "filesize does not match");
	data.resize(4..);

	let mut output = OutputWriter::new(path);

	let num_things = data.u32();
	let mut offsets: Vec<_> = (0..num_things)
		.map(|_| {
			let name = data.str(8);
			let offset = data.u32();
			(name, data.clone_at(offset as usize))
		})
		.collect();

	for i in 0..offsets.len().saturating_sub(1) {
		let next_start_pos = offsets[i + 1].1.position();
		offsets[i].1.set_end(next_start_pos);
	}

	let pal = offsets
		.iter()
		.find(|(name, _)| *name == "SYS_PAL")
		.unwrap()
		.1
		.clone()
		.remaining_slice();

	let mut strings = String::new();
	for (name, mut reader) in offsets {
		match name {
			"ARROW" => {
				let anims = try_parse_anim(reader.clone());
				save_anim(name, &anims.unwrap(), 24, &mut output, Some(pal));
			}
			"SYS_PAL" => {
				let pixels = reader.slice(8 * 8 * 3);
				save_pal(output.set_output_path(name, "png"), pixels);
			}
			"SND_PUSH" => {
				output.write(
					name,
					"wav",
					try_read_wav(&mut reader).expect("expected a wav file!"),
				);
			}
			"F8" => {
				let mut letter_pixels = [[0; 8 * 8]; 128];
				let letters: Vec<FontLetter> = letter_pixels
					.iter_mut()
					.enumerate()
					.map(|(i, pixels)| {
						for row in pixels.chunks_exact_mut(8) {
							let mut b = reader.u8();
							for p in row {
								if b & 0x80 != 0 {
									*p = 1;
								}
								b <<= 1;
							}
						}
						FontLetter {
							code: i as u8,
							width: 8,
							height: 8,
							pixels,
						}
					})
					.collect();

				save_font_grid(name, &letters, &mut output, Some(pal));
			}
			"FONTBIG" | "FONTSML" => {
				let font_letters = parse_font_letters(reader.resized(reader.position()..));
				save_font_grid(name, &font_letters, &mut output, Some(pal));
			}
			_ => {
				write!(strings, "{name:8}\t").unwrap();
				loop {
					let c = reader.u8();
					match c {
						0 => break,
						b' '..=b'~' => strings.push(c as char),
						b'\t' => strings.push_str("\\t"),
						149 => strings.push('ę'),
						150 => strings.push('ń'),
						230 => strings.push('ć'),
						_ => panic!("{name}: unknown charcode {c}"),
					}
				}
				strings.push('\n');
			}
		}
	}
	output.write("strings", "txt", strings.as_bytes());
}

fn save_anim(name: &str, anims: &[Anim], fps: u16, output: &mut OutputWriter, pal: PalRef) {
	assert!(!anims.is_empty());
	output.set_output_path(name, "png");
	if anims.len() == 1 {
		let anim = &anims[0];
		save_png(
			&output.path,
			&anim.pixels,
			anim.width as u32,
			anim.height as u32,
			pal,
		);
		return;
	}

	let mut offset_x = 0;
	let mut offset_y = 0;
	let mut max_x = 0;
	let mut max_y = 0;
	for a in anims {
		offset_x = offset_x.max(a.x as isize);
		offset_y = offset_y.max(a.y as isize);
		max_x = max_x.max(a.width as isize - a.x as isize);
		max_y = max_y.max(a.height as isize - a.y as isize);
	}

	let width = (max_x + offset_x) as usize;
	let height = (max_y + offset_y) as usize;

	let mut encoder = setup_png(&output.path, width as u32, height as u32, pal);
	encoder.set_animated(anims.len() as u32, 0).unwrap();
	encoder.set_sep_def_img(false).unwrap();
	encoder.set_frame_delay(1, fps).unwrap();
	let mut encoder = encoder.write_header().expect("failed to write png header");
	let mut buffer = vec![0; width * height];
	for anim in anims {
		buffer.fill(0);
		let offset_x = (offset_x - (anim.x as isize)) as usize;
		for (a, b) in buffer
			.chunks_exact_mut(width)
			.skip((offset_y - anim.y as isize) as usize)
			.zip(anim.pixels.chunks_exact(anim.width as usize))
		{
			a[offset_x..offset_x + b.len()].copy_from_slice(b);
		}

		encoder
			.write_image_data(&buffer)
			.expect("failed to write png image data");
	}
	encoder.finish().expect("failed to write png file");
}

fn save_bsp_debug(name: &str, bsp: &Bsp, output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());

	fn recurse(
		gltf: &mut gltf::Gltf, temp_mesh: &mut Mesh, bsp: &Bsp, index: usize, node: gltf::NodeIndex,
	) {
		let plane = &bsp.planes[index];

		let front_index = plane.plane_index_front;
		if front_index >= 0 {
			let right_node = gltf.create_child_node(node, format!("front_{front_index}"), None);
			recurse(gltf, temp_mesh, bsp, front_index as usize, right_node);
		}

		temp_mesh.tris.clear();
		for i in 0..plane.tris_front_count {
			let tri = &bsp.mesh.tris[(plane.tris_front_index + i) as usize];
			if tri.indices[0] == tri.indices[1] && tri.indices[0] == tri.indices[2] {
				continue;
			}
			temp_mesh.tris.push(tri.clone());
		}
		for i in 0..plane.tris_back_count {
			let tri = &bsp.mesh.tris[(plane.tris_back_index + i) as usize];
			if tri.indices[0] == tri.indices[1] && tri.indices[0] == tri.indices[2] {
				continue;
			}
			temp_mesh.tris.push(tri.clone());
		}

		if !temp_mesh.tris.is_empty() {
			let mesh_node = gltf.create_child_node(node, format!("mesh_{index}"), None);
			add_mesh_to_gltf(gltf, format!("{index}"), temp_mesh, &[], Some(mesh_node));

			let flags_summary: Vec<_> = temp_mesh
				.tris
				.iter()
				.enumerate()
				.filter(|(_,t)| t.flags != 0)
				.map(
					|(i,t)| serde_json::json!({"index": i, "id": t.id(), "outlines": t.outlines(), "flags": t.flags & 0x008F_FFFF}),
				)
				.collect();
			gltf.set_node_extras(mesh_node, "flags", flags_summary);
		}

		let behind_index = plane.plane_index_behind;
		if behind_index >= 0 {
			let left_node = gltf.create_child_node(node, format!("behind_{behind_index}"), None);
			recurse(gltf, temp_mesh, bsp, behind_index as usize, left_node);
		}
	}

	let node = gltf.get_root_node();
	recurse(&mut gltf, &mut bsp.mesh.clone(), bsp, 0, node);

	gltf.combine_buffers();
	output.write(
		name,
		"debug.gltf",
		serde_json::to_string(&gltf).unwrap().as_bytes(),
	);
}

fn save_bsp(name: &str, bsp: &Bsp, output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());
	let root = gltf.get_root_node();
	add_mesh_to_gltf(&mut gltf, name.to_owned(), &bsp.mesh, &[], Some(root));

	gltf.combine_buffers();
	output.write(
		name,
		"gltf",
		serde_json::to_string(&gltf).unwrap().as_bytes(),
	);

	//save_bsp_debug(name, bsp, output);
}

fn setup_png<'a>(
	path: &Path, width: u32, height: u32, palette: Option<&'a [u8]>,
) -> png::Encoder<'a, impl std::io::Write> {
	let mut trns = [255; 16 * 16];
	let mut encoder = png::Encoder::new(
		BufWriter::new(fs::File::create(path).unwrap()),
		width,
		height,
	);
	if let Some(palette) = palette {
		encoder.set_color(png::ColorType::Indexed);
		for (alpha, rgb) in trns.iter_mut().zip(palette.chunks_exact(3)) {
			*alpha = if rgb == [255, 0, 255] { 0 } else { 255 };
		}
		trns[0] = 0;
		encoder.set_palette(std::borrow::Cow::Borrowed(palette));
		encoder.set_trns(trns.to_vec());
	} else {
		encoder.set_color(png::ColorType::Grayscale);
	}
	encoder
}

fn save_png(path: &Path, data: &[u8], width: u32, height: u32, palette: PalRef) {
	let mut encoder = setup_png(path, width, height, palette)
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

type Vec2 = [f32; 2];
type Vec3 = [f32; 3];

#[derive(Clone)]
struct MeshTri {
	indices: [u16; 3],
	texture: i16,
	uvs: [Vec2; 3],
	flags: u32, // bsp id and flags, 0 for normal meshes
}
impl MeshTri {
	fn id(&self) -> u8 {
		(self.flags >> 24) as u8
	}
	fn outlines(&self) -> [bool; 3] {
		[
			self.flags & 0x100000 != 0,
			self.flags & 0x200000 != 0,
			self.flags & 0x400000 != 0,
		]
	}
}

#[derive(Default, Clone)]
struct Mesh<'a> {
	textures: Vec<&'a str>,
	verts: Vec<Vec3>,
	tris: Vec<MeshTri>,
	bbox: [Vec3; 2],
	reference_points: Vec<Vec3>,
}
impl<'a> std::fmt::Debug for Mesh<'a> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Mesh")
			.field("textures", &self.textures)
			.field("verts", &self.verts.len())
			.field("tris", &self.tris.len())
			.field("bbox", &self.bbox)
			.field("reference points", &self.reference_points.len())
			.finish()
	}
}

#[derive(Debug, Clone)]
struct ImageRef {
	name: String,
	relative_path: PathBuf,
	width: usize,
	height: usize,
}

fn try_parse_mesh_tris(data: &mut Reader, count: usize) -> Option<Vec<MeshTri>> {
	if count > 10000 {
		return None;
	}
	let mut result = Vec::with_capacity(count);
	for _ in 0..count {
		let indices = data.try_get()?;
		let texture = data.try_i16()?;
		if texture > 256 {
			return None;
		}
		let uvs = data.try_get_unvalidated()?;

		let flags = data.try_u32()?;

		result.push(MeshTri {
			indices,
			texture,
			uvs,
			flags,
		});
	}
	Some(result)
}

fn parse_mesh<'a>(data: &mut Reader<'a>, read_textures: bool) -> Mesh<'a> {
	try_parse_mesh(data, read_textures).expect("failed to read mesh")
}

fn try_parse_mesh<'a>(data: &mut Reader<'a>, read_textures: bool) -> Option<Mesh<'a>> {
	let textures = if read_textures {
		let num_textures = data.try_u32()? as usize;
		if num_textures > 500 {
			return None;
		};
		let mut textures = Vec::with_capacity(num_textures);
		for i in 0..num_textures {
			textures.push(data.try_str(16)?);
		}
		textures
	} else {
		Default::default()
	};

	let num_verts = data.try_u32()? as usize;
	if num_verts > 10000 {
		return None;
	}
	let mut verts = data.try_get_vec::<Vec3>(num_verts)?;
	swizzle_slice(&mut verts);

	let num_tris = data.try_u32()? as usize;
	if num_tris > 10000 {
		return None;
	}
	let tris = try_parse_mesh_tris(data, num_tris)?;

	assert!(
		tris.iter().all(|tri| tri.flags == 0),
		"found mesh with non-zero triangle flags!"
	);

	let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = data.try_get()?;
	let bbox = [
		swizzle([min_x, min_y, min_z]),
		swizzle([max_x, max_y, max_z]),
	];

	let mut reference_points = if read_textures {
		let num_reference_points = data.try_u32()?;
		data.try_get_vec(num_reference_points as usize)?
	} else {
		Default::default()
	};
	swizzle_slice(&mut reference_points);

	Some(Mesh {
		textures,
		verts,
		tris,
		bbox,
		reference_points,
	})
}

fn try_parse_multimesh<'a>(data: &mut Reader<'a>) -> Option<Multimesh<'a>> {
	let num_textures = data.try_u32()? as usize;
	if num_textures > 500 {
		return None;
	};
	let mut textures = Vec::with_capacity(num_textures);
	for i in 0..num_textures {
		textures.push(data.try_str(16)?.to_owned());
	}

	let mut meshes = Vec::new();
	let num_submeshes = data.try_u32()?;
	if num_submeshes > 1000 {
		return None;
	}
	for i in 0..num_submeshes {
		let name = data.try_str(12)?;
		let origin = swizzle(data.try_vec3()?);
		let mut mesh = try_parse_mesh(data, false)?;
		// shift to origin
		for point in &mut mesh.verts {
			for (a, b) in point.iter_mut().zip(origin) {
				*a -= b;
			}
		}
		meshes.push(Submesh {
			mesh,
			name: name.to_owned(),
			origin,
		});
	}
	let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = data.try_get()?;
	let bbox = [
		swizzle([min_x, min_y, min_z]),
		swizzle([max_x, max_y, max_z]),
	];

	let num_reference_points = data.try_u32()?;
	let mut reference_points = data.try_get_vec::<Vec3>(num_reference_points as usize)?;
	swizzle_slice(&mut reference_points);

	Some(Multimesh {
		textures,
		meshes,
		bbox,
		reference_points,
	})
}

fn to_string(path: &OsStr) -> String {
	path.to_str().unwrap().to_owned()
}

fn add_mesh_to_gltf(
	gltf: &mut gltf::Gltf, name: String, mesh: &Mesh, textures: &[ImageRef],
	target: Option<gltf::NodeIndex>,
) -> gltf::NodeIndex {
	#[derive(Default, Debug)]
	struct SplitMesh {
		texture_id: i16,
		image: Option<ImageRef>,
		material: Option<gltf::MaterialIndex>,
		indices: Vec<u16>,
		verts: Vec<Vec3>,
		uvs: Vec<[f32; 2]>,
		vert_map: HashMap<(u16, [isize; 2]), u16>,
	}

	fn round_uvs(uvs: [f32; 2]) -> [isize; 2] {
		uvs.map(|f| (f * 1024.0) as isize)
	}

	let mut primitives: Vec<SplitMesh> = if textures.is_empty() {
		Vec::from_iter(once(Default::default()))
	} else {
		mesh.textures
			.iter()
			.enumerate()
			.map(|(i, tex)| {
				let mut result = SplitMesh::default();
				if let Some(r) = textures.iter().find(|t| &t.name == tex) {
					result.image = Some(r.clone());
					result.texture_id = i as _;
					result.material = Some(gltf.create_texture_material_ref(
						tex.to_string(),
						to_string(r.relative_path.as_os_str()),
					));
				} else {
					result.texture_id = -tex[4..].parse::<i16>().expect("expected a pen number");
					result.material =
						Some(gltf.create_colour_material(tex.to_string(), [0.0, 0.0, 0.0, 1.0]));
				}
				result
			})
			.collect()
	};

	fn get_split_mesh(meshes: &mut [SplitMesh], index: i16) -> &mut SplitMesh {
		if meshes.len() == 1 {
			return &mut meshes[0];
		}

		meshes
			.iter_mut()
			.find(|mesh| mesh.texture_id == index)
			.expect("mesh not found for index {index}")
	}

	for tri in &mesh.tris {
		let split_mesh = get_split_mesh(&mut primitives, tri.texture);

		let indices = &tri.indices;
		let uvs = &tri.uvs;

		if tri.flags & 2 != 0 {
			// start hidden
			continue;
		}
		if indices[0] == indices[1] && indices[0] == indices[2] {
			// fully degenerate
			continue;
		}
		if tri.outlines() == [false; 3]
			&& (indices[0] == indices[1] || indices[1] == indices[2] || indices[0] == indices[2])
		{
			// partially degenerate
			continue;
		} // else might be a line

		for i in (0..3).rev() {
			let index = tri.indices[i];
			let mut uv = tri.uvs[i];

			if let Some(img) = &split_mesh.image {
				uv[0] /= img.width as f32;
				uv[1] /= img.height as f32;
			}

			let new_index = *split_mesh
				.vert_map
				.entry((index, round_uvs(uv)))
				.or_insert_with(|| {
					let result = split_mesh.verts.len();
					split_mesh.verts.push(mesh.verts[index as usize]);
					split_mesh.uvs.push(uv);
					result as _
				});

			split_mesh.indices.push(new_index);
		}
	}

	primitives.retain(|prim| !prim.indices.is_empty());
	if primitives.is_empty() && mesh.reference_points.is_empty() {
		return target.unwrap_or_else(|| gltf.create_node(name.to_owned(), None));
	}

	let mesh_index = gltf.create_mesh(name.to_owned());
	for new_mesh in &primitives {
		gltf.add_mesh_primitive(
			mesh_index,
			&new_mesh.verts,
			&new_mesh.indices,
			Some(&new_mesh.uvs),
			new_mesh.material,
		);
	}

	let node = match target {
		Some(node) => {
			assert!(
				gltf.get_node_mesh(node).is_none(),
				"replacing target node mesh!"
			);
			gltf.set_node_mesh(node, mesh_index);
			node
		}
		None => gltf.create_node(name.to_owned(), Some(mesh_index)),
	};

	let reference_points = &mesh.reference_points;
	if !reference_points.is_empty() {
		gltf.create_points_nodes("Reference Points".to_owned(), reference_points, Some(node));
	}

	node
}

fn save_mesh(name: &str, mesh: &Mesh, textures: &[ImageRef], output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());

	let root = gltf.get_root_node();
	add_mesh_to_gltf(&mut gltf, name.to_owned(), mesh, textures, Some(root));

	gltf.combine_buffers();
	output.write(
		name,
		"gltf",
		serde_json::to_string(&gltf).unwrap().as_bytes(),
	);
}

fn save_multimesh(name: &str, multimesh: &Multimesh, output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());

	let base_node = gltf.get_root_node();

	for (i, submesh) in multimesh.meshes.iter().enumerate() {
		let submesh_name = if submesh.name.is_empty() {
			format!("{i}")
		} else {
			submesh.name.clone()
		};

		let subnode_index = gltf.create_child_node(base_node, submesh_name.clone(), None);
		gltf.set_node_position(subnode_index, submesh.origin);
		add_mesh_to_gltf(
			&mut gltf,
			submesh_name.clone(),
			&submesh.mesh,
			&[],
			Some(subnode_index),
		);
	}

	let reference_points = &multimesh.reference_points;
	if !reference_points.is_empty() {
		gltf.create_points_nodes(
			"Reference Points".to_owned(),
			reference_points,
			Some(base_node),
		);
	}

	gltf.combine_buffers();
	output.write(name, "gltf", &serde_json::to_vec(&gltf).unwrap());
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

fn parse_and_save_dti(path: &Path) {
	let filename = get_filename(path);
	let file = read_file(path);
	let data = dti::Dti::parse(&file.1);
	set_pal(filename, filename.split_once('.').unwrap().0, data.pal);
	data.save(&mut OutputWriter::new(path));
}

fn main() {
	#[cfg(feature = "readranges")]
	println!("Read ranges enabled");

	let start_time = std::time::Instant::now();

	for_all_ext("assets", "dti", parse_and_save_dti);
	for_all_ext("assets", "bni", parse_bni);
	for_all_ext("assets", "mto", parse_mto);
	for_all_ext("assets", "sni", parse_sni);
	for_all_ext("assets", "mti", parse_mti);
	for_all_ext("assets", "cmi", parse_cmi);

	//for_all_ext("assets", "lbb", parse_lbb);
	//for_all_ext("assets", "fti", parse_fti);
	//for_all_ext("assets", "flc", parse_video);
	//for_all_ext("assets", "mve", parse_video);

	println!("done in {:.2?}", start_time.elapsed());
}
