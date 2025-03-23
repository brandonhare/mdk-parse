/// Exports the assets from FALL3D, which is the skydiving section at the start of each level.
use crate::Reader;
use crate::data_formats::image_formats::ColourMap;
use crate::data_formats::{Texture, TextureHolder, TextureResult};
use crate::file_formats::mti::Material;
use crate::file_formats::{Bni, Mti, Sni};
use crate::output_writer::OutputWriter;
use std::fmt::Write;

// combine flare and zoom images into an animation
fn combine_animation_frames(bni: &mut Bni) {
	let mut flare = Vec::new();
	let mut zoom = Vec::new();
	bni.textures.retain(|(name, tex)| {
		if let Some(flare_num) = name.strip_prefix("FLARE") {
			let flare_index: usize = flare_num
				.parse::<usize>()
				.expect("bad flare suffix")
				.checked_sub(1)
				.expect("bad flare index");
			if flare_index <= flare.len() {
				flare.resize_with(flare_index + 1, Default::default);
			}
			flare[flare_index] = tex.clone();
			false
		} else if let Some(zoom_num) = name.strip_prefix("ZOOM") {
			let zoom_index: usize = zoom_num.parse::<usize>().expect("bad flare suffix");
			if zoom_index <= zoom.len() {
				zoom.resize_with(zoom_index + 1, Default::default);
			}
			zoom[zoom_index] = tex.clone();
			false
		} else {
			true
		}
	});

	assert!(flare.iter().all(|tex| tex != &Default::default()));
	assert!(zoom.iter().all(|tex| tex != &Default::default()));

	// center flare anim
	let (flare_width, flare_height) = flare
		.iter()
		.fold((0, 0), |(w, h), tex| (w.max(tex.width), h.max(tex.height)));
	for tex in &mut flare {
		tex.position = (
			(flare_width - tex.width) as i16 / -2,
			(flare_height - tex.height) as i16 / -2,
		);
	}

	bni.animations_2d.push(("FLARE", flare));
	bni.animations_2d.push(("ZOOM", zoom));
}

/// FLARE and ZOOM animations are not normal images, but transparent overlays.
/// This is a special colour palette just for them.
/// It might not be 100% accurate (especially since transparency in the engine is just picking other colours from the existing palette), but it's probably close enough.
static ZOOM_PAL: [u8; NUM_ZOOM_PAL_ENTRIES * 4] = const {
	// these values are hard-coded into the engine.
	static ZOOM_TRANSPARENCIES: [u8; NUM_ZOOM_PAL_ENTRIES] = [
		0, 0x3, 0x6, 0xC, 0x12, 0x18, 0x30, 0x48, 0x60, 0x78, 0x90, 0xA8, 0xC0, 0xD7, 0xE6, 0xF5,
		0xFF,
	];

	let mut result = [255; NUM_ZOOM_PAL_ENTRIES * 4];
	let mut i = 0;
	while i < NUM_ZOOM_PAL_ENTRIES {
		result[NUM_ZOOM_PAL_ENTRIES * 3 + i] = ZOOM_TRANSPARENCIES[i];
		i += 1;
	}
	result
};
const NUM_ZOOM_PAL_ENTRIES: usize = 17;

pub fn parse_fall3d(save_sounds: bool, save_textures: bool, save_meshes: bool) {
	let output = OutputWriter::new("assets/FALL3D", true);
	let shared_output = output.push_dir("Shared");

	if save_sounds {
		let sni = std::fs::read("assets/FALL3D/FALL3D.SNI").unwrap();
		let sni = Sni::parse(Reader::new(&sni));
		let mut output = shared_output.push_dir("Sounds");
		for (name, sound) in &sni.sounds {
			sound.save_as(name, &mut output);
		}
		assert!(sni.anims.is_empty());
		assert!(sni.bsps.is_empty());
	}

	let bni = std::fs::read("assets/FALL3D/FALL3D.BNI").unwrap();
	let mut bni = Bni::parse(Reader::new(&bni));

	if save_textures {
		combine_animation_frames(&mut bni);

		let mut tex_output = shared_output.push_dir("Textures");
		let mut anim_output = shared_output.push_dir("Animations");
		let spacepal = bni
			.palettes
			.iter()
			.find(|(name, _)| *name == "SPACEPAL")
			.unwrap()
			.1;
		tex_output.write_palette("SPACEPAL", spacepal);

		for (name, frames) in &bni.animations_2d {
			let pal = if *name == "ZOOM" || *name == "FLARE" {
				// todo check if flare palette is different
				&ZOOM_PAL
			} else {
				spacepal
			};

			Texture::save_animated(frames, name, 24, &mut anim_output, Some(pal));
		}
		for (name, tex) in &bni.textures {
			tex.save_as(name, &mut tex_output, Some(spacepal));
		}
	}

	// todo move most of this to shared
	let mut temp_filename = String::new();
	for level_index in 1..=5 {
		temp_filename.clear();
		write!(temp_filename, "assets/FALL3D/FALL3D_{level_index}.MTI").unwrap();
		let mti = std::fs::read(&temp_filename).unwrap();
		let mti = Mti::parse(Reader::new(&mti));

		temp_filename.clear();
		write!(temp_filename, "LEVEL{level_index}").unwrap();
		let mut output = output.push_dir(&temp_filename);

		temp_filename.clear();
		write!(temp_filename, "FALLPU_{level_index}").unwrap();
		for (name, str) in &bni.strings {
			if *name == temp_filename {
				temp_filename.clear();
				for line in str {
					temp_filename.push_str(line);
					temp_filename.push('\n');
				}
				output.write(name, "txt", &temp_filename);
				break;
			}
		}

		// get palette
		temp_filename.clear();
		write!(temp_filename, "FALLP{level_index}").unwrap();
		let palette = bni
			.palettes
			.iter()
			.find(|(name, _)| *name == temp_filename)
			.unwrap()
			.1;

		if save_textures {
			output.write_palette(&temp_filename, palette);
		}

		struct Textures<'a> {
			palette: &'a [u8],
			materials: &'a [(&'a str, Material<'a>)],
		}
		impl<'a> TextureHolder<'a> for Textures<'a> {
			fn lookup(&mut self, name: &str) -> TextureResult<'a> {
				for (mat_name, mat) in self.materials {
					if *mat_name == name {
						return match mat {
							Material::Pen(pen) => TextureResult::Pen(*pen),
							Material::Texture(tex, _) => TextureResult::SaveRef {
								width: tex.width,
								height: tex.height,
								path: format!("Textures/{name}.png"),
								masked: tex.pixels.iter().any(|p| *p == 0),
							},
							Material::AnimatedTexture(frames, _) => TextureResult::SaveRef {
								width: frames[0].width,
								height: frames[0].height,
								path: format!("Textures/{name}.png"),
								masked: frames
									.iter()
									.any(|frame| frame.pixels.iter().any(|p| *p == 0)),
							},
						};
					}
				}
				//eprintln!("failed to find fall3d material {name}"); // todo
				TextureResult::None
			}
			fn get_used_colours(&self, _name: &str, _colours: &mut ColourMap) {
				eprintln!("not getting used colours in fall3d");
			}
			fn get_palette(&self) -> &[u8] {
				self.palette
			}
			fn get_translucent_colours(&self) -> [[u8; 4]; 4] {
				eprintln!("getting unknown stream translucent colours!");
				[[0; 4]; 4]
			}
		}
		let mut textures = if save_textures {
			Textures {
				palette,
				materials: &mti.materials,
			}
		} else {
			Textures {
				palette,
				materials: &[],
			}
		};

		if save_meshes {
			let mut output = output.push_dir("Meshes");
			for (name, mesh) in &bni.meshes {
				mesh.save_textured_as(name, &mut output, &mut textures);
			}

			if !bni.animations_3d.is_empty() {
				let mut output = output.push_dir("Animations");
				for (name, anim) in &bni.animations_3d {
					anim.save_as(name, &mut output);
				}
			}
		}

		if save_textures {
			let mut output = output.push_dir("Meshes/Textures");
			let mut temp_anim = Vec::new();
			temp_filename.clear();
			write!(temp_filename, "L{level_index}_C").unwrap();
			for (name, material) in &mti.materials {
				match material {
					Material::Pen(_pen) => (),
					Material::Texture(tex, _flags) if name.starts_with(&temp_filename) => {
						temp_anim.push(tex.clone_ref());
					}
					Material::Texture(tex, _flags) => tex.save_as(name, &mut output, Some(palette)),
					Material::AnimatedTexture(frames, _flags) => {
						Texture::save_animated(frames, name, 24, &mut output, Some(palette))
					}
				}
			}
			Texture::save_animated(&temp_anim, &temp_filename, 12, &mut output, Some(palette));
		}
	}
}
