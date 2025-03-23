use std::path::Path;
use std::process::Stdio;

use crate::data_formats::{TextureHolder, TextureResult};
use crate::file_formats::mti::Material;
use crate::file_formats::{Bni, Fti, Lbb, Mti, Sni};
use crate::output_writer::OutputWriter;
use crate::reader::Reader;

pub fn parse_misc(save_videos: bool) {
	let mut output = OutputWriter::new("assets/MISC", true);

	export_simple(&output, "FINISH.BNI", |reader, output| {
		Bni::parse(reader).save(output, true);
	});
	export_simple(&output, "OPTIONS.BNI", |reader, output| {
		Bni::parse(reader).save(output, true)
	});
	export_simple(&output, "mdkfont.fti", |reader, output| {
		Fti::parse(reader).save(output)
	});
	export_simple(&output, "UINSTALL.FTI", |reader, output| {
		Fti::parse(reader).save(output)
	});
	export_simple(&output, "MDKSOUND.SNI", |reader, output| {
		Sni::parse(reader).save(output)
	});

	export_stats(&output);

	// export LBBs
	for i in 3..=8 {
		let lbb = load_misc_file(&format!("LOAD_{i}.LBB"));
		let lbb = Lbb::parse(Reader::new(&lbb));
		lbb.texture
			.save_as(&format!("LOAD_{i}.png"), &mut output, Some(lbb.palette));
	}

	if save_videos {
		let mut video_output = output.push_dir("FLIC");
		for dirent in std::fs::read_dir("assets/MISC/FLIC").unwrap().flatten() {
			export_video(&dirent.path(), &mut video_output);
		}
	}
}

fn load_misc_file(filename: &str) -> Vec<u8> {
	let path = Path::new("assets/MISC").join(filename);
	match std::fs::read(&path) {
		Ok(data) => data,
		Err(e) => panic!("failed to read {}: {e}", path.display()),
	}
}

fn export_simple(
	output: &OutputWriter, filename: &str, func: impl FnOnce(Reader, &mut OutputWriter),
) {
	let data = load_misc_file(filename);
	func(Reader::new(&data), &mut output.push_dir(filename));
}

fn export_stats(output: &OutputWriter) {
	struct MiscTextureHolder<'a> {
		palette: &'a [u8],
		materials: &'a [(&'a str, Material<'a>)],
	}
	impl<'a> TextureHolder<'a> for MiscTextureHolder<'a> {
		fn lookup(&mut self, name: &str) -> TextureResult<'a> {
			let Some((_, mat)) = self
				.materials
				.iter()
				.find(|(mat_name, _mat)| *mat_name == name)
			else {
				return TextureResult::None;
			};
			match mat {
				Material::Pen(pen) => TextureResult::Pen(*pen),
				Material::Texture(tex, _) => TextureResult::SaveRef {
					width: tex.width,
					height: tex.height,
					path: format!("Textures/{name}.png"),
					masked: false,
				},
				Material::AnimatedTexture(..) => unreachable!(),
			}
		}
		fn get_palette(&self) -> &[u8] {
			self.palette
		}
		fn get_used_colours(
			&self, _name: &str, _colours: &mut crate::data_formats::image_formats::ColourMap,
		) {
			unimplemented!()
		}
		fn get_translucent_colours(&self) -> [[u8; 4]; 4] {
			unimplemented!()
		}
	}

	let stats_bni = load_misc_file("STATS.BNI");
	let mut stats_bni = Bni::parse(Reader::new(&stats_bni));
	let stats_mti = load_misc_file("STATS.MTI");
	let stats_mti = Mti::parse(Reader::new(&stats_mti));

	let mut stats_output = output.push_dir("STATS");

	let mut mesh_output = stats_output.push_dir("Meshes");
	let mut tex_output = mesh_output.push_dir("Textures");

	let [(_, palette)] = stats_bni.palettes.as_slice() else {
		panic!("unexpected palette count in stats bni")
	};
	let mut textures = MiscTextureHolder {
		palette,
		materials: &stats_mti.materials,
	};
	// save mesh materials
	for (mat_name, mat) in &stats_mti.materials {
		let tex = match mat {
			Material::Pen(_) => continue,
			Material::Texture(texture, _flags) => texture,
			Material::AnimatedTexture(..) => {
				panic!("unexpected animation in mti")
			}
		};
		tex.save_as(mat_name, &mut tex_output, Some(palette));
	}
	// save mesh
	for (mesh_name, mesh) in &stats_bni.meshes {
		mesh.save_textured_as(mesh_name, &mut mesh_output, &mut textures);
	}

	// save everything else
	stats_bni.meshes.clear();
	stats_bni.save(&mut stats_output, false);
}

fn export_video(input_path: &Path, output: &mut OutputWriter) {
	let Some(filename) = input_path.file_name().and_then(|s| s.to_str()) else {
		return;
	};
	let Some((file_stem, ext)) = filename.rsplit_once('.') else {
		return;
	};
	if !ext.eq_ignore_ascii_case("FLC") && !ext.eq_ignore_ascii_case("MVE") {
		return;
	}
	println!("  Converting {filename}...");
	let output_path = output.set_output_path(file_stem, "mp4");

	let result = std::process::Command::new("ffmpeg")
		.args(["-y", "-loglevel", "error", "-i"])
		.args([input_path, output_path])
		.stdin(Stdio::null())
		.status();

	match result {
		Ok(status) if status.success() => {}
		Ok(status) => {
			eprintln!("failed to convert {filename} ({status})");
		}
		Err(e) => {
			eprintln!("failed to run ffmpeg: {e}");
		}
	}
}
