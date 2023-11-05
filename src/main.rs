#![allow(dead_code)]
#![allow(unused_variables, unused_assignments, unused_mut)] // todo check
#![warn(trivial_casts, trivial_numeric_casts)]
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
		assert!(
			!ext.starts_with('.'),
			"extension should not start with a dot"
		);
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
	}
}
fn read_file(path: &Path) -> DataFile {
	DataFile(path, std::fs::read(path).unwrap())
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
			let anims = try_parse_anim(&mut entry_reader).expect("failed to parse sni anim");
			//assert_eq!(entry_reader.position(), file_size);
			save_anim(
				entry_name,
				&anims,
				&mut output,
				get_pal(filename, entry_name),
			);
		} else if entry_type == 0 {
			let bsp = try_parse_bsp(&mut entry_reader).expect("failed to parse sni bsp");
			assert_eq!(entry_reader.position(), file_size);
			save_bsp(entry_name, &bsp, &mut output);
		} else {
			// todo what else is in entry_type
			let data = entry_reader.remaining_slice();
			if data.starts_with(b"RIFF") {
				output.write(entry_name, "wav", data);
			} else {
				println!("unknown sni entry {entry_name}");
				output.write(entry_name, "", data);
			}
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

fn try_parse_anim(data: &mut Reader) -> Option<Vec<Anim>> {
	let filesize = data.try_u32()? as usize;
	data.resize(4..data.len());
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
		let [a, b]: [_; 2] = data.try_get()?;

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
			x: a,
			y: b,
			pixels,
		});
	}

	Some(results)
}

#[repr(C)]
#[derive(Debug)]
struct PathDataEntry {
	t: i32,
	pos1: Vec3,
	pos2: Vec3,
	pos3: Vec3,
}

fn parse_mti(path: &Path) {
	let buf = read_file(path);

	let filename = path.file_name().unwrap().to_str().unwrap();

	let pal = get_pal(filename, "");

	let mut output = OutputWriter::new(path);

	parse_mti_data(&mut output, &buf, pal)
}
fn parse_mti_data(output: &mut OutputWriter, buf: &[u8], pal: PalRef) {
	let mut reader = Reader::new(buf);
	let filesize = reader.u32() + 4;
	assert_eq!(reader.len(), filesize as usize, "filesize does not match");
	reader.resize(4..);

	let name = reader.str(12);
	let filesize2 = reader.u32();
	assert_eq!(filesize, filesize2 + 12, "filesizes do not match");
	let num_entries = reader.u32();

	let mut pen_entries = Vec::new();

	for i in 0..num_entries {
		let name = reader.str(8);
		let a = reader.u32();

		if a == 0xFFFFFFFF {
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
			let mut data = reader.resized(start_offset..);

			let width: u32;
			let height: u32;
			let new_a;

			if a & 0x30000 == 0 {
				width = data.u16() as u32;
				height = data.u16() as u32;
				new_a = a;
			} else {
				new_a = (a & 0xFFFF) | ((data.u16() as u32) << 0x10);
				let ignored = data.u16();
				width = data.u16() as u32;
				height = data.u16() as u32;
			}

			let bit_depth = get_bits(width);

			let bit_depth_2 = (1 << (bit_depth & 0x3F)) - 1;

			let data_start_pos = start_offset + data.position();
			let data_end_pos = data_start_pos + width as usize * height as usize;

			//println!("{i}: {filename} a:{a:x} a2:{new_a:x} b:{b} c:{c} len:{maybe_length} value:{some_value}, bits:{bit_depth}, bits2:{bit_depth_2}, startPos:{start_offset:x}, dataStartPos:{data_start_pos:x}, dataEndPos:{data_end_pos:x}");

			output.set_output_path(name, "png");

			//println!("{i}: {dirname:?}");

			let data = data.remaining_slice();
			save_png(
				&output.path,
				&data[..width as usize * height as usize],
				width,
				height,
				pal.clone(),
			);
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
}

fn get_bits(n: u32) -> u32 {
	let mut i = 0xC;
	let mut result = 0;
	while i != 0 {
		if n <= 1 << result & 0x3F {
			return result;
		};
		result += 1;
		i -= 1;
	}
	result
}

//static PALS: std::sync::OnceLock<Mutex<HashMap<String, Box<[u8]>>>> = OnceLock::new();

thread_local! {
	static PALS : RefCell<HashMap<String, Rc<[u8]>>> = Default::default();
}

type PalRef = Option<Rc<[u8]>>;
fn get_pal(filename: &str, name: &str) -> PalRef {
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
fn set_pal(filename: &str, asset_name: &str, pal: &[u8]) -> PalRef {
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
				println!("level pal not found for arena pal {level_name}/{asset_name}");
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
		if matches!(&data[..4], b"RIFF") {
			output.write(name, "wav", reader.remaining_slice());
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

			output.set_output_path(&format!("{name}_1"), "png");
			save_png(
				&output.path,
				&result,
				width as u32,
				height as u32,
				Some(lut1.into()),
			);
			output.set_output_path(&format!("{name}_2"), "png");
			save_png(
				&output.path,
				&result,
				width as u32,
				height as u32,
				Some(lut2.into()),
			);
			continue;
		}
		// overlay
		if name == "SNIPERS2" {
			parse_overlay(name, data, &mut output, get_pal(filename, name));
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
		if try_parse_image(data, name, &mut output) {
			continue;
		}

		// image without palette
		if data.len() >= 4 {
			let pos = reader.position();
			let width = reader.u16() as usize;
			let height = reader.u16() as usize;

			if width * height == reader.remaining_len() {
				output.set_output_path(name, "png");
				let pal = get_pal(filename, name);
				save_png(
					&output.path,
					reader.slice(width * height),
					width as u32,
					height as u32,
					pal,
				);
				continue;
			}
			reader.set_position(pos);
		}

		if let Some(anims) = try_parse_anim(&mut reader.clone()) {
			save_anim(name, &anims, &mut output, get_pal(filename, name));
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

		if let Some(anim) = try_parse_alienanim(&mut reader.clone()) {
			output.write(name, "anim.txt", format!("{anim:?}").as_bytes());
			continue;
		}

		// raw image
		if data.len() == 640 * 480 {
			output.set_output_path(name, "png");
			let pal = get_pal(filename, name);
			save_png(&output.path, reader.remaining_slice(), 640, 480, pal);
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
		save_zoom("ZOOM", &zooms, &mut output, get_pal(filename, "ZOOM")); // todo palette
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
	save_anim(name, &anims, output, pal);
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

	output.set_output_path(name, "png");
	save_png(&output.path, &dest, width as u32, height as u32, pal);
}

fn try_parse_image(data: &[u8], asset_name: &str, output: &mut OutputWriter) -> bool {
	if data.len() <= 0x304 {
		return false;
	}
	let lut = &data[..0x300];
	let width = u16::from_le_bytes(data[0x300..0x302].try_into().unwrap()) as usize;
	let height = u16::from_le_bytes(data[0x302..0x304].try_into().unwrap()) as usize;
	let data = &data[0x304..];
	if data.len() != width * height {
		return false;
	}
	output.set_output_path(asset_name, "png");
	save_png(
		&output.path,
		data,
		width as _,
		height as _,
		Some(lut.into()),
	);
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
		let asset_offset = data.u32() as usize;

		new_path.push(arena_name);
		let mut output = OutputWriter::new(&new_path);
		new_path.pop();

		let mut asset_reader = data.resized(asset_offset..);
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
			let offset1_data =
				&asset_reader.buf()[subfile_offset + 4..subfile_offset + 4 + offset1_len];
			parse_mto_subthing(arena_name, offset1_data, &mut output);
		}

		{
			// parse pal
			asset_reader.set_position(pal_offset);
			let pal_size = bsp_offset - pal_offset;
			assert_eq!(pal_size, 336);
			let pal_data = asset_reader.slice(pal_size);
			set_pal(filename, arena_name, pal_data);
			output.set_output_path("PAL", "PNG");
			save_pal(&output.path, pal_data);
		}

		{
			// parse bsp
			asset_reader.set_position(bsp_offset);
			let bsp = parse_bsp(&mut asset_reader);
			save_bsp(arena_name, &bsp, &mut output);
		}

		{
			// output matfile
			parse_mti_data(&mut output, matfile_data, get_pal(arena_name, arena_name));
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
		let Some(anim) = try_parse_alienanim(&mut data.resized(offset as usize..end as usize))
		else {
			eprintln!("failed to parse anim {i} {arena_name}/{name}");
			output.write(name, "", &data.buf()[offset as usize..end as usize]);
			continue;
		};
		output.write(name, "anim.txt", format!("{anim:?}").as_bytes());
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
		let mut reader =
			Reader::new(&buf[sound_offset as usize..sound_offset as usize + sound_length as usize]);
		let data = reader.buf();
		assert!(data.starts_with(b"RIFF"));
		assert_eq!(b, 0x7FFF);
		assert!(looping == 0 || looping == 1);
		output.write(sound_name, "wav", reader.remaining_slice());
	}
}

#[derive(Debug, Default)]
struct Submesh {
	mesh: Mesh,
	name: String,
	origin: Vec3,
}
#[derive(Debug, Default)]
struct Multimesh {
	textures: Vec<String>,
	meshes: Vec<Submesh>,
	bbox: [Vec3; 2],
	extras: Vec<Vec3>, // todo what are these
}

#[derive(Debug)]
struct BspPlane {
	normal: Vec3,
	dist: f32,
	plane_index_a: i16,
	plane_index_b: i16,
	a: u16,
	tri_index: u16,
	b: i32,
	zeroes: [u32; 4],
}

struct Bsp {
	planes: Vec<BspPlane>,
	triangles: Vec<MeshTri>,
	points: Vec<Vec3>,
	num_last: u32,
}

fn try_parse_bsp(data: &mut Reader) -> Option<Bsp> {
	let num_materials = data.try_u32()?;
	if num_materials > 500 {
		return None;
	}
	let material_names = (0..num_materials)
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
			normal: data.try_get()?,
			dist: data.try_get()?,
			plane_index_a: data.try_get()?,
			plane_index_b: data.try_get()?,
			a: data.try_get()?,
			tri_index: data.try_get()?,
			b: data.try_get()?,
			zeroes: data.try_get()?,
		};
		if result.plane_index_a < -1
			|| result.plane_index_a as isize > num_planes as isize
			|| result.plane_index_b < -1
			|| result.plane_index_b as isize > num_planes as isize
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

	let num_triangles = data.try_u32()? as usize;
	let triangles = try_parse_mesh_tris(data, num_triangles)?;

	let num_points = data.try_u32()? as usize;
	if num_points > 10000 {
		return None;
	}
	let points = data.try_get_vec::<Vec3>(num_points)?;

	let num_last = data.try_u32()?;
	if num_last > 10000 {
		return None;
	}
	let last_things = data.try_slice(num_last as usize)?;
	if last_things.iter().any(|c| *c != 255) {
		return None;
	}

	//assert_eq!(data.position(), data.len());

	Some(Bsp {
		planes,
		triangles,
		points,
		num_last,
	})
}

fn parse_bsp(data: &mut Reader) -> Bsp {
	try_parse_bsp(data).expect("failed to parse bsp!")
}

struct AlienAnim {
	speed: f32,
	np: u32,
	nf: u32,
	vectors: Vec<Vec3>,
	parts: Vec<AlienAnimPart>,
}
struct AlienAnimPart {
	vecs: Vec<Vec3>,
	count: u32,
	data: AlienAnimPartType,
}
enum AlienAnimPartType {
	A(f32, Vec<AlienAnimPartRow>),
	B(u16, Vec<[u8; 24]>),
}
struct AlienAnimPartRow {
	index: u16,
	triples: Vec<[u8; 3]>,
}
impl std::fmt::Debug for AlienAnim {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AlienAnim")
			.field("speed", &self.speed)
			.field("np", &self.np)
			.field("nf", &self.nf)
			.field("vectors", &self.vectors.len())
			.field("parts", &self.parts)
			.finish()
	}
}
impl std::fmt::Debug for AlienAnimPart {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AlienAnimPart")
			.field("vecs", &self.vecs.len())
			.field("count", &self.count)
			.field("data", &self.data)
			.finish()
	}
}
impl std::fmt::Debug for AlienAnimPartType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::A(a, rows) => f
				.debug_struct("A")
				.field("value", a)
				.field("rows", &rows.len())
				.finish(),
			Self::B(b, rows) => f
				.debug_struct("B")
				.field("value", b)
				.field("rows", &rows.len())
				.finish(),
		}
	}
}

fn try_parse_alienanim(data: &mut Reader) -> Option<AlienAnim> {
	let speed = data.try_f32()?;
	data.resize(4..);
	let np = data.try_u32()?;
	let nf = data.try_u32()?;

	if np > 1000 || nf > 1000 {
		return None;
	}

	let offsets = data.try_get_vec::<u32>(np as usize)?;
	if offsets.iter().any(|o| *o as usize >= data.len()) {
		return None;
	}
	let vectors = data.try_get_vec::<Vec3>(nf as usize)?;
	// if vectors.iter().any(|v| *v != [0.0; 3]) {
	// 	return None;
	// }

	let num_things = data.try_u32().filter(|n| *n <= 10000)?;
	let things = data.try_get_vec::<Vec3>((num_things * nf) as usize)?;

	let mut parts = Vec::new();

	for (i, &offset) in offsets.iter().enumerate() {
		let next = offsets.get(i + 1).copied().unwrap_or(data.len() as u32);
		let mut data = data.resized_pos(..next as usize, offset as usize);

		let name = data.try_str(12)?;
		let count = data.try_u32().filter(|c| *c < 1000)?;
		let a = data.try_f32()?;
		let part = if a == 0.0 {
			let b = data.try_u16()?;
			let vecs = data.try_get_vec::<Vec3>(count as usize)?;
			let anim_data = data.try_get_vec::<[u8; 24]>(nf as usize)?;
			AlienAnimPart {
				vecs,
				count,
				data: AlienAnimPartType::B(b, anim_data),
			}
		} else {
			let vecs = data.try_get_vec::<Vec3>(count as usize)?;
			let num_shorts = (count * 3 + 2) * (nf - 1);
			let mut rows = Vec::new();
			for j in 0..=nf {
				if j == nf {
					println!("too many rows");
					return None;
				}
				let index = data.try_u16()?;
				if index == 0xFFFF {
					break;
				}
				let triples = data.try_get_vec::<[u8; 3]>(count as usize)?;
				rows.push(AlienAnimPartRow { index, triples });
			}
			AlienAnimPart {
				vecs,
				count,
				data: AlienAnimPartType::A(a, rows),
			}
		};

		if data.position() + 4 < next as usize {
			println!("animation data left over");
			return None;
		}
		parts.push(part);
	}

	Some(AlienAnim {
		speed,
		np,
		nf,
		vectors,
		parts,
	})
}

fn parse_lbb(path: &Path) {
	let filename = path.file_name().unwrap().to_str().unwrap();
	let data = read_file(path);

	let mut output = OutputWriter::new_no_dir(path);
	output.path.pop();

	let success = try_parse_image(&data, filename, &mut output);
	assert!(success);
}

fn parse_dti(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let mut data = Reader::new(&buf);
	let filesize = data.u32() + 4;
	assert_eq!(data.len() as u32, filesize, "filesize does not match");
	data.resize(4..);

	let data_file_name = data.str(12);
	let filesize2 = data.u32();
	assert_eq!(filesize, filesize2 + 12);

	let mut output = OutputWriter::new(path);

	let offsets = data.get::<[u32; 5]>();

	let get_range = |i: usize| {
		let next = offsets
			.get(i + 1)
			.map(|o| *o as usize)
			.unwrap_or(data.len());
		data.resized_pos(..next, offsets[i] as usize)
	};

	// data1 (arena and skybox data?)
	let mut info_str;
	let sky_info: [i32; 8];
	{
		let mut data = get_range(0);
		let arena_index = data.u32();
		assert_eq!(arena_index, 0);
		let player_start_pos = data.vec3();
		let player_start_angle = data.f32();
		sky_info = data.get::<[i32; 8]>();
		let translucent_colours = data
			.get::<[[i32; 4]; 4]>()
			.map(|c| u32::from_be_bytes(c.map(|n| n as u8)));
		assert!(data.remaining_len() == 0);

		info_str = format!(
			"start pos: {player_start_pos:?}, start angle: {player_start_angle}\n\
skybox: {sky_info:3?}\n\
translucent colours: {translucent_colours:08x?}\n"
		);
	}

	// data2  (teleport locations?)
	{
		let mut data = get_range(1);
		let count = data.u32();
		let mut things = Vec::with_capacity(count as usize);
		for i in 0..count {
			things.push((data.get::<[i32; 2]>(), data.vec3(), data.f32()));
			assert_eq!(things.last().unwrap().0[0], (i as i32 + 1) % 10);
		}
		assert_eq!(data.remaining_len(), 0);

		// todo what are these

		info_str.push_str("\nTeleport locations?:\n");
		info_str.extend(things.iter().map(|([a, arena_index], pos, angle)| {
			format!("{a} {arena_index:2}, {pos:7?} {angle}\n")
		}));
		output.write("info", "txt", info_str.as_bytes());
	}

	// entities (connnects? arena locations?)
	let mut bsp_entities_str = String::new();
	{
		let mut entities_data = get_range(2);

		let num_arenas = entities_data.u32();
		let mut arena_offsets = Vec::with_capacity(num_arenas as usize);
		for _ in 0..num_arenas {
			let name = entities_data.str(8);
			let offset = entities_data.u32();
			let num = entities_data.f32();
			arena_offsets.push((name, offset, num));
		}

		for (arena_index, &(name, offset, num)) in arena_offsets.iter().enumerate() {
			let mut data = entities_data.resized(
				offset as usize
					..(arena_offsets
						.get(arena_index + 1)
						.map(|(_, next, _)| *next as usize)
						.unwrap_or(entities_data.len())),
			);

			writeln!(bsp_entities_str, "{name}, {num}").unwrap();

			let num_entities = data.u32();
			for _ in 0..num_entities {
				let entity_type = data.i32();
				let arena_index = data.i32();
				let c = data.i32();
				let pos = data.vec3();
				write!(
					bsp_entities_str,
					"{entity_type},{arena_index:4},{c}, {pos:7?}, "
				)
				.unwrap();

				if entity_type == 2 || entity_type == 4 {
					let rest = data.str(12);
					writeln!(bsp_entities_str, "{rest}").unwrap();
				} else {
					let rest = data.vec3();
					writeln!(bsp_entities_str, "{rest:7?}").unwrap();
				}
			}
			assert_eq!(data.remaining_len(), 0);

			bsp_entities_str.push('\n');
		}
	}
	output.write("bsp_entities", "txt", bsp_entities_str.as_bytes());

	// pal
	let pal = {
		let mut pal_data = get_range(3);
		let pal_free_rows = pal_data.u32();
		let pixels = pal_data.slice(0x300);
		assert_eq!(pal_data.remaining_len(), 0);
		assert!(pal_free_rows % 16 == 0);
		output.set_output_path("PAL", "png");
		save_pal(&output.path, pixels);
		set_pal(filename, filename.split_once('.').unwrap().0, pixels)
	};

	// skybox
	{
		let mut skybox_data = get_range(4);

		let [sky_top_colour, sky_floor_colour, sky_y, sky_x, sky_width, sky_height, sky_reflected_top_colour, sky_reflected_bottom_colour] =
			sky_info;

		let has_reflection = sky_reflected_top_colour >= 0;

		let src_height = sky_height as usize;
		let dest_width = sky_width as usize + 4;
		let (dest_height, src_width) = if has_reflection {
			assert!(src_height & 1 == 0);
			(src_height / 2, dest_width * 2)
		} else {
			(src_height, dest_width)
		};

		let skybox_pixels = skybox_data.slice(src_width * src_height);

		let filename_footer = skybox_data.slice(12);
		assert_eq!(skybox_data.remaining_len(), 0);

		let mut full_height = dest_height;

		let mut pixels = Vec::new();
		if !has_reflection {
			pixels.extend(std::iter::repeat(sky_top_colour as u8).take(src_width * 64));
			pixels.extend_from_slice(skybox_pixels);
			pixels.extend(std::iter::repeat(sky_floor_colour as u8).take(src_width * 64));
			full_height += 128;
		} else {
			let size = dest_width * dest_height;
			assert_eq!(size * 4, skybox_pixels.len());

			let (top, bottom) = skybox_pixels.split_at(src_width * dest_height);

			pixels.extend(std::iter::repeat(sky_top_colour as u8).take(dest_width * 64));
			pixels.extend(
				top.chunks(dest_width)
					.step_by(2)
					.take(dest_height)
					.flatten(),
			);
			pixels.extend(std::iter::repeat(sky_floor_colour as u8).take(dest_width * 64));

			pixels.extend(std::iter::repeat(sky_top_colour as u8).take(dest_width * 64));
			pixels.extend(
				top.chunks(dest_width)
					.skip(1)
					.step_by(2)
					.take(dest_height)
					.flatten(),
			);
			pixels.extend(std::iter::repeat(sky_floor_colour as u8).take(dest_width * 64));

			pixels.extend(std::iter::repeat(sky_reflected_top_colour as u8).take(dest_width * 64));
			pixels.extend(
				bottom
					.chunks(dest_width)
					.skip(1)
					.step_by(2)
					.take(dest_height)
					.flatten(),
			);
			pixels
				.extend(std::iter::repeat(sky_reflected_bottom_colour as u8).take(dest_width * 64));
			pixels.extend(std::iter::repeat(sky_reflected_top_colour as u8).take(dest_width * 64));
			pixels.extend(
				bottom
					.chunks(dest_width)
					.skip(1)
					.step_by(2)
					.take(dest_height)
					.flatten(),
			);
			pixels
				.extend(std::iter::repeat(sky_reflected_bottom_colour as u8).take(dest_width * 64));

			full_height *= 4;
			full_height += 64 * 8;
		}

		println!("{filename} {sky_x} {sky_y} {sky_width} {sky_height}");

		output.set_output_path("skybox", "png");
		save_png(
			&output.path,
			&pixels,
			dest_width as u32,
			full_height as u32,
			pal,
		);
	}
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
			let data = data.resized_pos(.., offset as usize);
			(name, data)
		}));
		entries.retain(|e| e.1.position() != 0);
	}

	let output = OutputWriter::new(path);

	// process init entries
	{
		let mut init_output = None;
		for (name, mut data) in init_entries {
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data);
			let init_output = init_output.get_or_insert_with(|| output.push_dir("init"));
			init_output.write(name, "txt", cmi.as_bytes());
		}
	}

	// process mesh entries
	{
		let mut mesh_output = output.push_dir("meshes");
		for (name, mut data) in mesh_entries {
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
	}

	// process setup entries
	{
		let mut setup_output = output.push_dir("setup");
		for (name, mut data) in setup_entries {
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data);
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
			let cmi = cmi_bytecode::parse_cmi(filename, name, &mut data);

			arena_output.write(
				name,
				"txt",
				format!("name: {name}, thing: \"{str1}\", music: \"{music}\"\n\n{cmi}").as_bytes(),
			);
		}
	}
}

fn save_anim(name: &str, anims: &[Anim], output: &mut OutputWriter, pal: PalRef) {
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

	let mut encoder = setup_png(&output.path, width as u32, height as u32, &pal);
	encoder.set_animated(anims.len() as u32, 0).unwrap();
	encoder.set_sep_def_img(false).unwrap();
	encoder.set_frame_delay(1, 12).unwrap();
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

fn save_bsp(name: &str, bsp: &Bsp, output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());
	let pos_index = gltf.add_positions(&bsp.points);
	let indices_index = gltf.add_indices(
		&bsp.triangles
			.iter()
			.flat_map(|t| t.indices)
			.collect::<Vec<_>>(),
	);
	gltf.add_mesh_simple(name.to_owned(), &[pos_index, indices_index], None);
	gltf.combine_buffers();
	output.write(
		name,
		"gltf",
		serde_json::to_string(&gltf).unwrap().as_bytes(),
	);
}

fn setup_png<'a>(
	path: &Path, width: u32, height: u32, palette: &'a PalRef,
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
		encoder.set_palette(std::borrow::Cow::Borrowed(palette.as_ref()));
		encoder.set_trns(trns.to_vec());
	} else {
		encoder.set_color(png::ColorType::Grayscale);
	}
	encoder
}

fn save_png(path: &Path, data: &[u8], width: u32, height: u32, palette: PalRef) {
	let mut encoder = setup_png(path, width, height, &palette)
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
#[repr(C)]
struct MeshTri {
	indices: [u16; 3],
	texture: i16,
	uvs: [Vec2; 3],
	flags: u32, // todo what are these
}

#[derive(Default, Clone)]
struct Mesh {
	textures: Vec<String>,
	verts: Vec<Vec3>,
	tris: Vec<MeshTri>,
	bbox: [Vec3; 2],
	extras: Vec<Vec3>, // todo what are these
}
impl std::fmt::Debug for Mesh {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Mesh")
			.field("textures", &self.textures)
			.field("verts", &self.verts.len())
			.field("tris", &self.tris.len())
			.field("bbox", &self.bbox)
			.field("extras", &self.extras.len())
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
		if !(-5000..=200).contains(&texture) {
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

fn parse_mesh(data: &mut Reader, read_textures: bool) -> Mesh {
	try_parse_mesh(data, read_textures).expect("failed to read mesh")
}

fn try_parse_mesh(data: &mut Reader, read_textures: bool) -> Option<Mesh> {
	let textures = if read_textures {
		let num_textures = data.try_u32()? as usize;
		if num_textures > 500 {
			return None;
		};
		let mut textures = Vec::with_capacity(num_textures);
		for i in 0..num_textures {
			textures.push(data.try_str(16)?.to_owned());
		}
		textures
	} else {
		Default::default()
	};

	let num_verts = data.try_u32()? as usize;
	if num_verts > 10000 {
		return None;
	}
	let mut verts = Vec::with_capacity(num_verts);
	for _ in 0..num_verts {
		let [x, y, z] = data.try_vec3()?;
		verts.push([x, y, z]);
	}

	let num_tris = data.try_u32()? as usize;
	if num_tris > 10000 {
		return None;
	}
	let tris = try_parse_mesh_tris(data, num_tris)?;

	let [min_x, max_x, min_y, max_y, min_z, max_z]: [f32; 6] = data.try_get()?;
	let bbox = [[min_x, min_y, min_z], [max_x, max_y, max_z]];

	let extras = if read_textures {
		let num_extras = data.try_u32()?;
		data.try_get_vec(num_extras as usize)?
	} else {
		Default::default()
	};

	Some(Mesh {
		textures,
		verts,
		tris,
		bbox,
		extras,
	})
}

fn try_parse_multimesh(data: &mut Reader) -> Option<Multimesh> {
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
		let origin = data.try_vec3()?;
		let mesh = try_parse_mesh(data, false)?;
		meshes.push(Submesh {
			mesh,
			name: name.to_owned(),
			origin,
		});
	}
	let bbox = data.try_get()?;

	let num_extras = data.u32();
	let extras = data.try_get_vec::<Vec3>(num_extras as usize)?;

	Some(Multimesh {
		textures,
		meshes,
		bbox,
		extras,
	})
}

fn to_string(path: &OsStr) -> String {
	path.to_str().unwrap().to_owned()
}

fn save_mesh(name: &str, mesh: &Mesh, textures: &[ImageRef], output: &mut OutputWriter) {
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

	let mut gltf = gltf::Gltf::new(name.to_owned());

	let mut meshes: Vec<SplitMesh> = if textures.is_empty() {
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
					result.material =
						Some(gltf.add_texture(tex.clone(), to_string(r.relative_path.as_os_str())));
				} else {
					result.texture_id = -tex[4..].parse::<i16>().expect("expected a pen number");
					result.material = Some(gltf.add_colour(tex.clone(), [0.0, 0.0, 0.0, 1.0]));
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
		let split_mesh = get_split_mesh(&mut meshes, tri.texture);

		let indices = &tri.indices;
		let uvs = &tri.uvs;

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

	for new_mesh in &meshes {
		let primitives = [
			gltf.add_positions(&new_mesh.verts),
			gltf.add_uvs(&new_mesh.uvs),
			gltf.add_indices(&new_mesh.indices),
		];
		gltf.add_mesh_simple(name.to_owned(), &primitives, new_mesh.material);
	}

	gltf.combine_buffers();

	let result = serde_json::to_string(&gltf).unwrap();
	output.write(name, "gltf", result.as_bytes());
}

fn save_multimesh(name: &str, multimesh: &Multimesh, output: &mut OutputWriter) {
	let mut gltf = gltf::Gltf::new(name.to_owned());
	for mesh in &multimesh.meshes {
		let mut verts = mesh.mesh.verts.clone();
		for p in &mut verts {
			for (a, b) in p.iter_mut().zip(mesh.origin.iter()) {
				*a -= b;
			}
		}
		let verts = gltf.add_positions(&verts);
		let indices = gltf.add_indices(
			&mesh
				.mesh
				.tris
				.iter()
				.flat_map(|tri|[tri.indices[0], tri.indices[2], tri.indices[1]]) // todo hack why are multimeshes flipped
				.collect::<Vec<_>>(),
		);
		let mut submesh_name: &str = &mesh.name;
		if submesh_name.is_empty() {
			submesh_name = name;
		}
		let mesh_index = gltf.add_mesh_simple(submesh_name.to_owned(), &[verts, indices], None);
		gltf.set_mesh_position(mesh_index, mesh.origin)
	}
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
	println!("converting {}", path.display());
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

fn main() {
	#[cfg(feature = "readranges")]
	println!("Read ranges enabled");

	let start_time = std::time::Instant::now();

	//for_all_ext("assets", "dti", parse_dti);
	//for_all_ext("assets", "bni", parse_bni);
	//for_all_ext("assets", "mto", parse_mto);
	//for_all_ext("assets", "sni", parse_sni);
	//for_all_ext("assets", "mti", parse_mti);
	for_all_ext("assets", "cmi", parse_cmi);

	//for_all_ext("assets", "lbb", parse_lbb);
	//for_all_ext("assets", "flc", parse_video);
	//for_all_ext("assets", "mve", parse_video);

	//for_all_ext("assets", "fti", parse_fti);

	println!("done in {:.2?}", start_time.elapsed());
}
