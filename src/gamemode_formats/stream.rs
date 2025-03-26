//! Exports the assets from STREAM (the end-of-level space tube section).
use crate::data_formats::mesh::ColourMap;
use crate::data_formats::{Pen, Texture, TextureHolder, TextureResult};
use crate::file_formats::{
	Bni,
	mti::{Material, Mti},
};
use crate::{OutputWriter, Reader};
use std::fmt::Write;

pub fn parse_stream(save_sounds: bool, save_textures: bool, save_meshes: bool) {
	let bni = std::fs::read("assets/STREAM/STREAM.BNI").unwrap();
	let bni = Bni::parse(Reader::new(&bni));
	let mti = std::fs::read("assets/STREAM/STREAM.MTI").unwrap();
	let mti = Mti::parse(Reader::new(&mti));

	assert!(bni.animations_2d.is_empty());
	assert!(bni.coloured_textures.is_empty());
	assert!(bni.strings.is_empty());
	assert_eq!(bni.palettes.len(), 1);

	let palette = bni.palettes[0].1;

	let mut output = OutputWriter::new("assets/STREAM", true);

	if save_sounds {
		let mut output = output.push_dir("Sounds");
		for (name, sound) in &bni.sounds {
			sound.save_as(name, &mut output);
		}
	}

	struct StreamTextures<'a> {
		bni_textures: &'a [(&'a str, Texture<'a>)],
		mti_materials: &'a [(&'a str, Material<'a>)],
		palette: &'a [u8],
		used_textures: Vec<&'a str>,
	}
	impl<'a> TextureHolder<'a> for StreamTextures<'a> {
		fn lookup(&mut self, name: &str) -> TextureResult<'a> {
			let name = &name[..name.len().min(8)]; // truncated material names
			let mut result_name: Option<&'a str> = None;
			let mut width = 0;
			let mut height = 0;
			let mut masked = false;
			for (mat_name, mat) in self.mti_materials {
				if name != *mat_name {
					continue;
				}
				match mat {
					Material::Pen(pen) => return TextureResult::Pen(*pen),
					Material::Texture(tex, _) => {
						width = tex.width;
						height = tex.height;
						masked = tex.pixels.iter().any(|p| *p == 0);
						result_name = Some(mat_name);
					}
					Material::AnimatedTexture(tex, _) => {
						self.used_textures.push(mat_name);
						width = tex[0].width;
						height = tex[0].height;
						assert!(
							tex[1..]
								.iter()
								.all(|t| t.width == width && t.height == height),
							"mismatched texture dimensions!"
						);
						masked = tex.iter().any(|frame| frame.pixels.iter().any(|p| *p == 0));
						result_name = Some(mat_name);
					}
				}
				break;
			}
			if result_name.is_none() {
				for (tex_name, tex) in self.bni_textures {
					if *tex_name != name {
						continue;
					}
					result_name = Some(tex_name);
					width = tex.width;
					height = tex.height;
					masked = tex.pixels.iter().any(|p| *p == 0);
					break;
				}
			}

			let Some(name) = result_name else {
				// todo
				//println!("failed to find stream material {name}");
				return TextureResult::None;
			};

			self.used_textures.push(name);
			TextureResult::SaveRef {
				width,
				height,
				path: format!("Textures/{name}.png"),
				masked,
			}
		}
		fn get_used_colours(&self, name: &str, colours: &mut ColourMap) {
			for (mat_name, mat) in self.mti_materials {
				if name == *mat_name {
					match mat {
						Material::Pen(Pen::Colour(p)) => colours.push(*p),
						Material::Texture(tex, _) => colours.extend(tex.pixels.iter()),
						Material::AnimatedTexture(frames, _) => {
							for frame in frames {
								colours.extend(frame.pixels.iter());
							}
						}
						_ => {}
					}
					break;
				}
			}
			for (tex_name, tex) in self.bni_textures {
				if name == *tex_name {
					colours.extend(tex.pixels.iter());
					break;
				}
			}
		}
		fn get_palette(&self) -> &[u8] {
			self.palette
		}
		fn get_translucent_colours(&self) -> [[u8; 4]; 4] {
			eprintln!("getting unknown stream translucent colours!");
			[[0; 4]; 4]
		}
	}
	let mut textures = StreamTextures {
		bni_textures: &bni.textures,
		mti_materials: &mti.materials,
		palette,
		used_textures: Vec::new(),
	};

	if save_meshes {
		let mut output = output.push_dir("Meshes");
		for (name, mesh) in &bni.meshes {
			mesh.save_textured_as(name, &mut output, &mut textures);
		}
		let mut output = output.push_dir("Animations");
		for (name, anim) in &bni.animations_3d {
			anim.save_as(name, &mut output);
		}
	} else {
		// gather textures to save
		for (_, mesh) in &bni.meshes {
			for name in &mesh.materials {
				textures.lookup(name);
			}
		}
	}

	if save_textures {
		output.write_palette("Palette", palette);

		let mut meshes_output = output.push_dir("Meshes/Textures");
		let mut other_output = output.push_dir("Textures");
		let mut pens = String::from("Name    \tValue\n");

		for (name, mat) in &mti.materials {
			let output = if textures.used_textures.contains(name) {
				&mut meshes_output
			} else {
				&mut other_output
			};
			match mat {
				Material::Pen(pen) => writeln!(pens, "{name:8}\t{pen:?}").unwrap(),
				Material::Texture(tex, _) => tex.save_as(name, output, Some(palette)),
				Material::AnimatedTexture(frames, _) => {
					Texture::save_animated(frames, name, 24, output, Some(palette))
				}
			}
		}
		for (name, tex) in &bni.textures {
			let output = if textures.used_textures.contains(name) {
				&mut meshes_output
			} else {
				&mut other_output
			};
			tex.save_as(name, output, Some(palette));
		}

		other_output.write("Pens", "txt", &pens);
	}
}
