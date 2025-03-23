#![allow(dead_code)]
#![warn(trivial_casts, trivial_numeric_casts, future_incompatible)]

mod data_formats;
mod fall3d;
mod file_formats;
mod gltf;
mod output_writer;
mod reader;
mod stream;
mod traverse;
mod vectors;

use data_formats::image_formats;
use file_formats::{Bni, Cmi, Dti, Fti, Mti, Mto, Sni};
use output_writer::OutputWriter;
use reader::Reader;
use std::path::Path;
use vectors::{Vec2, Vec3};

fn read_file(path: &Path) -> Vec<u8> {
	std::fs::read(path).unwrap()
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
	let mti = Mti::parse(Reader::new(&file));
	mti.save(&mut OutputWriter::new(path, true), None);
}

fn parse_cmi(path: &Path) {
	let file = read_file(path);
	let cmi = Cmi::parse(Reader::new(&file));
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
	println!("  Converting {}...", path.display());
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
			eprintln!("failed to run ffmpeg: {e}");
		}
	}
}

fn main() {
	let start_time = std::time::Instant::now();

	let save_sounds = true;
	let save_textures = true;
	let save_meshes = true;

	println!("Parsing traverse data...");
	traverse::parse_traverse(save_sounds, save_textures, save_meshes);

	println!("Parsing stream data...");
	stream::parse_stream(save_sounds, save_textures, save_meshes);

	println!("Parsing fall3d data...");
	fall3d::parse_fall3d(save_sounds, save_textures, save_meshes);

	println!("Parsing misc data...");
	// todo export properly
	for_all_ext("assets/MISC", "bni", parse_bni);
	for_all_ext("assets/MISC", "sni", parse_sni);
	for_all_ext("assets/MISC", "lbb", parse_lbb);
	for_all_ext("assets/MISC", "mti", parse_mti); // todo export with palette from bni
	for_all_ext("assets/MISC", "fti", parse_fti);

	println!("Converting videos with ffmpeg...");
	for_all_ext("assets", "flc", parse_video);
	for_all_ext("assets", "mve", parse_video);

	println!("Done in {:.2?}", start_time.elapsed());
}
