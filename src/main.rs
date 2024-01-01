#![allow(dead_code)]
#![allow(unused_variables)] // todo check
#![warn(trivial_casts, trivial_numeric_casts, future_incompatible)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write;
use std::fs::{self, DirEntry};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod cmi_bytecode;
mod data_formats;
mod file_formats;
mod gltf;
mod named_vec;
mod output_writer;
mod reader;
use data_formats::{Bsp, Wav};
use file_formats::{Dti, Sni};
use named_vec::NamedVec;
use output_writer::OutputWriter;
use reader::Reader;

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

type Vec2 = [f32; 2];
type Vec3 = [f32; 3];
type Vec4 = [f32; 4];

fn swizzle(pos: Vec3) -> Vec3 {
	[pos[0], pos[2], -pos[1]]
}
fn swizzle_slice(points: &mut [Vec3]) {
	for point in points {
		*point = swizzle(*point);
	}
}
fn swizzle_vec(mut vec: Vec<Vec3>) -> Vec<Vec3> {
	swizzle_slice(&mut vec);
	vec
}

fn add_vec<T: std::ops::Add<Output = T>, const N: usize>(lhs: [T; N], rhs: [T; N]) -> [T; N] {
	let mut iter = lhs.into_iter().zip(rhs).map(|(a, b)| a + b);
	std::array::from_fn(|_| iter.next().unwrap())
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

#[derive(Debug)]
struct Anim {
	width: u16,
	height: u16,
	x: i16,
	y: i16,
	pixels: Vec<u8>,
}

fn try_parse_anim(reader: &mut Reader) -> Option<Vec<Anim>> {
	let mut data = reader.clone();
	let filesize = data.try_u32()? as usize;
	if filesize > data.remaining_len() {
		return None;
	}
	data.rebase_start();

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

	// mark source reader as read
	reader.skip(filesize + 4);

	Some(results)
}

fn parse_mti(path: &Path) {
	let buf = read_file(path);

	let filename = path.file_name().unwrap().to_str().unwrap();

	let pal = get_pal(filename, "");

	let mut output = OutputWriter::new(path, true);
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

	for _ in 0..num_entries {
		let name = reader.str(8);
		let flags = reader.u32();

		if flags == 0xFFFFFFFF {
			// pen
			let pen_value = reader.i32();
			let _ = reader.u32(); // padding
			let _ = reader.u32();
			pen_entries.push((name, pen_value));
		} else {
			// texture
			let a = reader.f32(); // todo what is this
			let b = reader.f32(); // todo what is this
			let start_offset = reader.u32() as usize;
			let mut data = reader.clone_at(start_offset);

			let flags_mask = flags & 0x30000; // if the texture is animated or not
			let flags_rest = flags & !0x30000; // todo what are these

			assert_ne!(
				flags_mask, 0x30000,
				"unknown mti flags combination ({flags:X}) on {name}"
			);

			let num_frames = if flags_mask != 0 {
				data.u32() as usize
			} else {
				1
			};
			let width = data.u16();
			let height = data.u16();
			let frame_size = width as usize * height as usize;

			if num_frames == 1 {
				let pixels = data.slice(frame_size);
				output.write_png(name, width as u32, height as u32, pixels, pal);
			} else if flags_mask == 0x10000 {
				// animated sequence
				let frames: Vec<Anim> = (0..num_frames)
					.map(|_| {
						let pixels = data.slice(frame_size);
						Anim {
							x: 0,
							y: 0,
							width,
							height,
							pixels: pixels.to_vec(),
						}
					})
					.collect();
				save_anim(name, &frames, 30, output, pal);
			} else {
				// compressed animation
				let base_pixels = data.slice(frame_size);

				let mut frames: Vec<Anim> = Vec::with_capacity(num_frames + 1);
				frames.push(Anim {
					x: 0,
					y: 0,
					width,
					height,
					pixels: base_pixels.to_vec(),
				});

				let _runtime_anim_time = data.u32();

				let mut data = data.resized(data.position()..); // offsets relative to here
				let offsets = data.get_vec::<u32>(num_frames * 2); // run of meta offsets then run of pixels offsets
				for (&metadata_offset, &pixel_offset) in
					offsets[..num_frames].iter().zip(&offsets[num_frames..])
				{
					let mut meta = data.clone_at(metadata_offset as usize);
					let mut src_pixels = data.clone_at(pixel_offset as usize);

					let mut dest_pixels = frames.last().unwrap().pixels.clone();

					let mut dest_pixel_offset = meta.u16() as usize * 4;
					let num_chunks = meta.u16();

					for _ in 0..num_chunks {
						let chunk_size = meta.u8() as usize * 4;
						let output_offset = meta.u8() as usize * 4;
						dest_pixels[dest_pixel_offset..dest_pixel_offset + chunk_size]
							.clone_from_slice(src_pixels.slice(chunk_size));
						dest_pixel_offset += chunk_size + output_offset;
					}

					frames.push(Anim {
						x: 0,
						y: 0,
						width,
						height,
						pixels: dest_pixels,
					});
				}

				let last_frame = frames.pop().unwrap();
				assert_eq!(
					frames.first().unwrap().pixels,
					last_frame.pixels,
					"last frame didn't reset to first frame"
				);

				save_anim(name, &frames, 12, output, pal);
			}

			// print summary
			let mut summary = String::new();
			if a != 0.0 {
				writeln!(summary, "a: {a}").unwrap();
			}
			if b != 3.5 {
				writeln!(summary, "b: {b}").unwrap();
			}
			if flags_rest != 0 {
				writeln!(summary, "flags: {flags_rest:X}").unwrap();
			}
			if !summary.is_empty() {
				output.write(name, "txt", summary.as_bytes());
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

	let mut output = OutputWriter::new(path, true);

	let mut zooms = Vec::new();

	for (i, &BniHeader { name, data }) in headers.iter().enumerate() {
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

	let output = OutputWriter::new(path, true);
	for _ in 0..num_arenas {
		let arena_name = data.str(8);
		let arena_offset = data.u32() as usize;

		let mut output = output.push_dir(arena_name);

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
			output.write_palette("PAL", pal_data);
			if let Some(pal_full) = pal_full {
				output.write_palette("PAL_full", &pal_full);
			}
		}

		{
			// parse bsp
			asset_reader.set_position(bsp_offset);
			let bsp = Bsp::parse(&mut asset_reader);
			bsp.save_as(arena_name, &mut output);
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

		let wav = Wav::parse(&mut reader);
		output.write(sound_name, "wav", wav.file_data.0);
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

struct AlienAnim<'a> {
	speed: f32,
	target_vectors: Vec<Vec3>, // todo what exactly are these?
	reference_points: Vec<Vec<Vec3>>,
	parts: Vec<AlienAnimPart<'a>>,
}
struct AlienAnimPart<'a> {
	name: &'a str,
	point_paths: Vec<Vec<Vec3>>,
}
impl<'a> AlienAnim<'a> {
	fn num_frames(&self) -> usize {
		self.target_vectors.len()
	}
}

fn try_parse_alienanim<'a>(name: &str, mut data: Reader<'a>) -> Option<AlienAnim<'a>> {
	let speed = data.try_f32()?;

	data.resize(data.position()..);

	let num_parts = data.try_u32()? as usize;
	let num_frames = data.try_u32()? as usize;
	if num_parts > 1000 || num_frames > 1000 {
		return None;
	}

	let part_offsets = data.try_get_vec::<u32>(num_parts)?;
	if part_offsets.iter().any(|o| *o as usize >= data.len()) {
		return None;
	}

	let mut target_vectors = swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
	for i in 1..target_vectors.len() {
		// todo added in gameplay
		target_vectors[i] = add_vec(target_vectors[i], target_vectors[i - 1]);
	}

	let num_reference_points = data.try_u32()? as usize;
	if num_reference_points > 8 || num_reference_points * num_frames * 12 > data.remaining_len() {
		return None;
	}
	let mut reference_points: Vec<Vec<Vec3>> = Vec::with_capacity(num_reference_points);
	for _ in 0..num_reference_points {
		let points_path = swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
		reference_points.push(points_path);
	}

	let mut parts = Vec::with_capacity(num_parts);
	for &offset in &part_offsets {
		let mut data = data.clone_at(offset as usize);
		let part_name = data.try_str(12)?;
		let num_points = data.try_u32()? as usize;
		if num_points > 1000 {
			return None;
		}
		let scale = data.try_f32()?;
		let mut point_paths: Vec<Vec<Vec3>> = Vec::new();
		point_paths.resize_with(num_points, || Vec::with_capacity(num_frames));

		if scale == 0.0 {
			let scale_vec = 1.0 / (0x8000u32 >> (data.try_u8()? & 0x3F)) as f32;
			let scale_pos = 1.0 / (0x8000u32 >> (data.try_u8()? & 0x3F)) as f32;
			// origin points
			let origin_points = data.try_get_vec::<Vec3>(num_points)?;
			// don't swizzle until after processing

			// transforms
			for frame_index in 0..num_frames {
				let transform = data.try_get::<[[i16; 4]; 3]>()?;
				let [r1, r2, r3] = transform.map(|[x, y, z, w]| {
					[
						x as f32 * scale_vec,
						y as f32 * scale_vec,
						z as f32 * scale_vec,
						w as f32 * scale_pos,
					]
				});
				for (path, &[x, y, z]) in point_paths.iter_mut().zip(&origin_points) {
					path.push(swizzle([
						r1[0] * x + r1[1] * y + r1[2] * z + r1[3],
						r2[0] * x + r2[1] * y + r2[2] * z + r2[3],
						r3[0] * x + r3[1] * y + r3[2] * z + r3[3],
					]))
				}
			}
		} else {
			// origin points
			for path in &mut point_paths {
				path.push(swizzle(data.try_vec3()?));
			}
			// frames
			for _ in 0..num_frames {
				let frame_index = data.try_u16()? as usize;
				if frame_index > num_frames {
					break;
				}

				for path in &mut point_paths {
					if frame_index < path.len() {
						return None; // frames out of order
					}
					let prev = *path.last().unwrap();
					path.resize(frame_index, prev); // duplicate potential gaps so our timeline is full

					let pos = data.try_get::<[i8; 3]>()?;
					let pos = swizzle(pos.map(|i| i as f32 * scale));
					path.push(add_vec(prev, pos));
				}

				if frame_index == num_frames {
					assert_eq!(data.u16(), 0xFFFF);
					break;
				}
			}
		}

		for path in &mut point_paths {
			assert!(path.len() <= num_frames);
			path.resize(num_frames, *path.last().unwrap()); // duplicate until the end of the timeline
		}

		parts.push(AlienAnimPart {
			name: part_name,
			point_paths,
		});
	}

	Some(AlienAnim {
		speed,
		target_vectors,
		reference_points,
		parts,
	})
}

fn save_alienanim_text(name: &str, anim: &AlienAnim, output: &mut OutputWriter) {
	let mut result = format!(
		"name: {name}\nspeed: {}\nnum frames: {}\n",
		anim.speed,
		anim.num_frames(),
	);

	writeln!(
		result,
		"\nreference points ({}):",
		anim.reference_points.len()
	)
	.unwrap();
	for (i, path) in anim.reference_points.iter().enumerate() {
		writeln!(result, "\tpoint {i} ({} frames):", path.len()).unwrap();
		for (j, point) in path.iter().enumerate() {
			writeln!(result, "\t\t[frame {j:2}] {point:?}").unwrap();
		}
	}

	writeln!(result, "\ntarget vectors ({}):", anim.target_vectors.len()).unwrap();
	for (i, target) in anim
		.target_vectors
		.iter()
		.scan([0.0; 3], |acc, item| {
			*acc = add_vec(*acc, *item);
			Some(*acc)
		})
		.enumerate()
	{
		writeln!(result, "\t[{i}] {target:?}").unwrap();
	}

	writeln!(result, "\nparts ({}):", anim.parts.len()).unwrap();
	for (part_index, part) in anim.parts.iter().enumerate() {
		writeln!(
			result,
			"\t[part {part_index}] {} ({} points):",
			part.name,
			part.point_paths.len()
		)
		.unwrap();
		for (point_index, path) in part.point_paths.iter().enumerate() {
			writeln!(result, "\t\t[point {point_index}]").unwrap();
			for (i, pos) in path.iter().enumerate() {
				writeln!(result, "\t\t\t[frame {i:2}] {pos:?}").unwrap();
			}
		}
	}

	output.write(name, "anim.txt", result.as_bytes());
}

fn save_alienanim(name: &str, anim: &AlienAnim, output: &mut OutputWriter) {
	let num_frames = anim.num_frames();

	let fps = 30.0;
	let time_period = anim.speed / fps;

	let mut gltf = gltf::Gltf::new(name.into());
	let cube_mesh = Some(gltf.get_cube_mesh());
	let animation = gltf.create_animation(name.into());
	let root_node = gltf.get_root_node();
	let base_timestamps = gltf.create_animation_timestamps(num_frames, fps / anim.speed);
	let interpolation = Some(gltf::AnimationInterpolationMode::Step);

	if anim.target_vectors.iter().any(|p| *p != [0.0; 3]) {
		let node = gltf.create_child_node(root_node, "Target Vectors".into(), cube_mesh);
		gltf.add_animation_translation(
			animation,
			node,
			base_timestamps,
			&anim.target_vectors,
			interpolation,
		);
	}

	if anim
		.reference_points
		.iter()
		.any(|p| p.iter().any(|p| *p != [0.0; 3]))
	{
		let ref_node = gltf.create_child_node(root_node, "Reference Points".into(), None);
		for (i, path) in anim.reference_points.iter().enumerate() {
			let node = gltf.create_child_node(ref_node, i.to_string(), cube_mesh);
			gltf.add_animation_translation(animation, node, base_timestamps, path, interpolation);
		}
	}

	for part in &anim.parts {
		let part_node = gltf.create_child_node(root_node, part.name.into(), None);
		for (i, path) in part.point_paths.iter().enumerate() {
			let point_node = gltf.create_child_node(part_node, i.to_string(), cube_mesh);
			gltf.add_animation_translation(
				animation,
				point_node,
				base_timestamps,
				path,
				interpolation,
			);
		}
	}

	gltf.combine_buffers();
	output.write(
		name,
		"anim.gltf",
		serde_json::to_string(&gltf).unwrap().as_bytes(),
	);

	//save_alienanim_text(name, anim, output);
}

fn parse_lbb(path: &Path) {
	let filename = path.file_name().unwrap().to_str().unwrap();
	let data = read_file(path);

	let mut output = OutputWriter::new(path.parent().unwrap(), false);

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

	let mut output = OutputWriter::new(path, true);

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
		row_width as u32,
		(num_rows * cell_height) as u32,
		&result,
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

	let mut output = OutputWriter::new(path, true);

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
				let anims = try_parse_anim(&mut reader).unwrap();
				save_anim(name, &anims, 24, &mut output, Some(pal));
			}
			"SYS_PAL" => {
				let pixels = reader.slice(8 * 8 * 3);
				output.write_palette(name, pixels);
			}
			"SND_PUSH" => {
				output.write(name, "wav", Wav::parse(&mut reader).file_data.0);
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

fn save_anim(name: &str, frames: &[Anim], fps: u16, output: &mut OutputWriter, palette: PalRef) {
	let num_frames = frames.len();
	if frames.len() == 1 {
		let image = &frames[0];
		output.write_png(
			name,
			image.width as u32,
			image.height as u32,
			&image.pixels,
			palette,
		);
		return;
	}
	assert_ne!(num_frames, 0);

	let mut offset_x = 0;
	let mut offset_y = 0;
	let mut max_x = 0;
	let mut max_y = 0;
	for frame in frames {
		offset_x = offset_x.max(frame.x as isize);
		offset_y = offset_y.max(frame.y as isize);
		max_x = max_x.max(frame.width as isize - frame.x as isize);
		max_y = max_y.max(frame.height as isize - frame.y as isize);
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

	let mut buffer = vec![0; width * height];
	for anim in frames {
		buffer.fill(0);
		let offset_x = (offset_x - (anim.x as isize)) as usize;
		for (dest, src) in buffer
			.chunks_exact_mut(width)
			.skip((offset_y - anim.y as isize) as usize)
			.zip(anim.pixels.chunks_exact(anim.width as usize))
		{
			dest[offset_x..offset_x + src.len()].copy_from_slice(src);
		}
		encoder
			.write_image_data(&buffer)
			.expect("failed to write png image data");
	}
	encoder.finish().expect("failed to write png file");
}

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
	let verts = swizzle_vec(data.try_get_vec::<Vec3>(num_verts)?);

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

	let reference_points = if read_textures {
		let num_reference_points = data.try_u32()?;
		swizzle_vec(data.try_get_vec(num_reference_points as usize)?)
	} else {
		Default::default()
	};

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
	let reference_points = swizzle_vec(data.try_get_vec::<Vec3>(num_reference_points as usize)?);

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

fn parse_dti(path: &Path) {
	let filename = get_filename(path);
	let file = read_file(path);
	let data = Dti::parse(Reader::new(&file.1));
	set_pal(filename, filename.split_once('.').unwrap().0, data.pal);
	data.save(&mut OutputWriter::new(path, true));
}
fn parse_sni(path: &Path) {
	let buf = read_file(path);
	let filename = get_filename(path);
	let sni = Sni::parse(Reader::new(&buf));
	sni.save(&mut OutputWriter::new(path, true));
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
