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
mod output_writer;
mod reader;
mod vectors;
use data_formats::{Bsp, Mesh, Wav};
use file_formats::{Dti, Fti, Mti, Sni};
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

	for &BniHeader { name, data } in headers.iter() {
		// audio
		let mut reader = Reader::new(data);
		if let Some(wav) = Wav::try_parse(&mut reader) {
			output.write(name, "wav", wav.samples.0);
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

		if let Some(multimesh) = Mesh::try_parse(&mut reader.clone(), true) {
			multimesh.save_as(name, &mut output);
			continue;
		}

		if let Some(mesh) = Mesh::try_parse(&mut reader.clone(), false) {
			mesh.save_as(name, &mut output);
			continue;
		}

		if let Some(anim) = try_parse_alienanim(reader.clone()) {
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

		let matfile_offset = asset_reader.position();

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
			asset_reader.set_position(matfile_offset);
			let mti = Mti::parse(&mut asset_reader);
			let pal = get_pal(arena_name, arena_name);
			mti.save(&mut output, pal.as_deref());
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
		let Some(anim) = try_parse_alienanim(data.resized(offset as usize..end as usize)) else {
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
		assert!(is_multimesh <= 1, "invalid multimesh type {is_multimesh}");
		let mesh = Mesh::parse(&mut data, is_multimesh != 0);
		mesh.save_as(name, output);
	}

	// sounds
	for &(sound_name, looping, b, sound_offset, sound_length) in &sound_headers {
		assert_eq!(b, 0x7FFF);
		assert!(looping == 0 || looping == 1);

		let mut reader =
			Reader::new(&buf[sound_offset as usize..sound_offset as usize + sound_length as usize]);

		let wav = Wav::parse(&mut reader);
		output.write(sound_name, "wav", wav.samples.0);
	}
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

fn try_parse_alienanim(mut data: Reader<'_>) -> Option<AlienAnim<'_>> {
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

	let mut target_vectors = Vec3::swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
	for i in 1..target_vectors.len() {
		// todo added in gameplay
		target_vectors[i] = target_vectors[i] + target_vectors[i - 1];
	}

	let num_reference_points = data.try_u32()? as usize;
	if num_reference_points > 8 || num_reference_points * num_frames * 12 > data.remaining_len() {
		return None;
	}
	let mut reference_points: Vec<Vec<Vec3>> = Vec::with_capacity(num_reference_points);
	for _ in 0..num_reference_points {
		let points_path = Vec3::swizzle_vec(data.try_get_vec::<Vec3>(num_frames)?);
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
				for (path, &Vec3 { x, y, z }) in point_paths.iter_mut().zip(&origin_points) {
					path.push(
						Vec3::from([
							r1[0] * x + r1[1] * y + r1[2] * z + r1[3],
							r2[0] * x + r2[1] * y + r2[2] * z + r2[3],
							r3[0] * x + r3[1] * y + r3[2] * z + r3[3],
						])
						.swizzle(),
					)
				}
			}
		} else {
			// origin points
			for path in &mut point_paths {
				path.push(data.try_vec3()?.swizzle());
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
					let pos = Vec3::from(pos.map(|i| i as f32 * scale)).swizzle();
					path.push(prev + pos);
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
		.scan(Vec3::default(), |acc, item| {
			*acc += *item;
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

	if anim.target_vectors.iter().any(|p| *p != Vec3::default()) {
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
		.any(|p| p.iter().any(|p| *p != Vec3::default()))
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

	output.write(name, "anim.gltf", gltf.render_json().as_bytes());

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
	let mut data = Reader::new(&buf);

	let filesize = data.u32() + 4;
	assert_eq!(data.len(), filesize as usize, "filesize does not match");
	data.resize(4..);

	let filename = data.str(12);
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
			let is_multimesh = data.u32();
			assert!(is_multimesh <= 1, "invalid multimesh type {is_multimesh}");
			let mesh = Mesh::parse(&mut data, is_multimesh != 0);
			mesh.save_as(name, &mut mesh_output);
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
			if let Some(anim) = try_parse_alienanim(anim_reader) {
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
	let filename = get_filename(path);
	let file = read_file(path);
	let dti = Dti::parse(Reader::new(&file));
	set_pal(filename, filename.split_once('.').unwrap().0, dti.pal);
	dti.save(&mut OutputWriter::new(path, true));
}
fn parse_sni(path: &Path) {
	let file = read_file(path);
	let sni = Sni::parse(Reader::new(&file));
	sni.save(&mut OutputWriter::new(path, true));
}

fn parse_mti(path: &Path) {
	let filename = get_filename(path);
	let file = read_file(path);
	let mti = Mti::parse(&mut Reader::new(&file));
	let pal = get_pal(filename, "");
	mti.save(&mut OutputWriter::new(path, true), pal.as_deref());
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
