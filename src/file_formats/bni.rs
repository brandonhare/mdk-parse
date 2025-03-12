use crate::data_formats::{Animation, Mesh, Texture, Wav, image_formats};
use crate::{OutputWriter, Reader};

pub struct Bni<'a> {
	pub sounds: Vec<(&'a str, Wav<'a>)>,
	pub textures: Vec<(&'a str, Texture<'a>)>,
	pub coloured_textures: Vec<(&'a str, (&'a [u8], Texture<'a>))>,
	pub animations_2d: Vec<(&'a str, Vec<Texture<'a>>)>,
	pub animations_3d: Vec<(&'a str, Animation<'a>)>,
	pub meshes: Vec<(&'a str, Mesh<'a>)>,
	pub palettes: Vec<(&'a str, &'a [u8])>,
	pub strings: Vec<(&'a str, Vec<&'a str>)>,

	pub unknowns: Vec<(&'a str, &'a [u8])>,
}

impl std::fmt::Debug for Bni<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		struct Named<'a, T>(&'a [(&'a str, T)]);
		impl<T> std::fmt::Debug for Named<'_, T> {
			fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
				f.debug_list()
					.entries(self.0.iter().map(|(name, _)| *name))
					.finish()
			}
		}

		f.debug_struct("Bni")
			.field("sounds", &Named(&self.sounds))
			.field("textures", &Named(&self.textures))
			.field("coloured_textures", &Named(&self.coloured_textures))
			.field("animations_2d", &Named(&self.animations_2d))
			.field("animations_3d", &Named(&self.animations_3d))
			.field("meshes", &Named(&self.meshes))
			.field("palettes", &Named(&self.palettes))
			.field("strings", &Named(&self.strings))
			.field("unknowns", &Named(&self.unknowns))
			.finish()
	}
}

impl<'a> Bni<'a> {
	pub fn parse(mut file_reader: Reader<'a>) -> Self {
		let filesize = file_reader.u32() + 4;
		assert_eq!(
			file_reader.len(),
			filesize as usize,
			"filesize does not match"
		);
		file_reader.rebase();

		let mut sounds = Vec::new();
		let mut textures = Vec::new();
		let mut coloured_textures = Vec::new();
		let mut animations_2d = Vec::new();
		let mut animations_3d = Vec::new();
		let mut meshes = Vec::new();
		let mut palettes = Vec::new();
		let mut strings = Vec::new();
		let mut unknowns = Vec::new();

		let num_entries = file_reader.u32();
		for entry_index in 0..num_entries {
			let name = file_reader.str(12);
			let offset = file_reader.u32() as usize;

			let next_offset = if entry_index + 1 == num_entries {
				file_reader.len()
			} else {
				file_reader.clone_at(file_reader.position() + 12).u32() as usize
			};

			let reader = file_reader.resized(offset..next_offset);
			// guess asset types

			// wav
			if let Some(wav) = Wav::try_parse(&mut reader.clone()) {
				sounds.push((name, wav));
				continue;
			}

			// image
			if let Some(texture) = image_formats::try_parse_basic_image(&mut reader.clone()) {
				textures.push((name, texture));
				continue;
			}
			if let Some(texture) = image_formats::try_parse_overlay_image(&mut reader.clone()) {
				textures.push((name, texture));
				continue;
			}
			if let Some(texture) = image_formats::try_parse_rle_image(&mut reader.clone()) {
				textures.push((name, texture));
				continue;
			}
			if let Some((pal, texture)) =
				image_formats::try_parse_palette_image(&mut reader.clone())
			{
				coloured_textures.push((name, (pal, texture)));
				continue;
			}
			if let Some(([lut1, lut2], texture)) =
				image_formats::try_parse_crossfade_image(&mut reader.clone())
			{
				coloured_textures.push((name, (lut1, texture.clone())));
				coloured_textures.push((name, (lut2, texture)));
				continue;
			}

			// 2d animation
			if let Some(mut anim) = image_formats::try_parse_animation(&mut reader.clone()) {
				if anim.len() == 1 {
					textures.push((name, anim.pop().unwrap()));
				} else {
					animations_2d.push((name, anim));
				}
				continue;
			}

			// 3d animation
			if let Some(anim) = Animation::try_parse(&mut reader.clone()) {
				animations_3d.push((name, anim));
				continue;
			}

			// meshes
			if let Some(mesh) = Mesh::try_parse(&mut reader.clone(), false) {
				meshes.push((name, mesh));
				continue;
			}
			if let Some(mesh) = Mesh::try_parse(&mut reader.clone(), true) {
				meshes.push((name, mesh));
				continue;
			}

			// strings
			if let Some(text) = try_parse_strings(&mut reader.clone()) {
				strings.push((name, text));
				continue;
			}

			// palette
			if reader.len() == 0x300 {
				palettes.push((name, reader.clone().slice(0x300)));
				continue;
			}

			// raw image
			if reader.len() == 640 * 480 {
				textures.push((
					name,
					Texture::new(640, 480, reader.clone().remaining_slice()),
				));
				continue;
			}

			eprintln!("unknown asset {name}");
			unknowns.push((name, reader.clone().remaining_slice()));
		}

		Bni {
			sounds,
			textures,
			coloured_textures,
			animations_2d,
			animations_3d,
			meshes,
			palettes,
			strings,
			unknowns,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		fn save_items<T>(
			folder_name: &str, output: &mut OutputWriter, items: &[(&str, T)],
			mut save_func: impl FnMut(&str, &T, &mut OutputWriter),
		) {
			if items.is_empty() {
				return;
			}
			let mut output = output.push_dir(folder_name);

			let mut prev_name = "";
			let mut name_run = 0;
			let mut buffer = String::new();
			for (i, (name, item)) in items.iter().enumerate() {
				// deduplicate names (for crossfade image)
				let save_name = if *name == prev_name
					|| items
						.get(i + 1)
						.is_some_and(|(next_name, _)| name == next_name)
				{
					use std::fmt::Write;
					name_run += 1;
					buffer.clear();
					write!(buffer, "{name} ({name_run})").unwrap();
					&buffer
				} else {
					name_run = 0;
					*name
				};
				prev_name = name;

				save_func(save_name, item, &mut output);
			}
		}

		let palette = self
			.palettes
			.first()
			.map(|(_, pal)| *pal)
			.filter(|_| self.palettes.len() == 1);

		save_items("sounds", output, &self.sounds, |name, sound, output| {
			sound.save_as(name, output)
		});

		save_items(
			"textures",
			output,
			&self.textures,
			|name, texture, output| texture.save_as(name, output, palette),
		);

		save_items(
			"textures",
			output,
			&self.coloured_textures,
			|name, (pal, texture), output| texture.save_as(name, output, Some(pal)),
		);

		save_items(
			"2d animations",
			output,
			&self.animations_2d,
			|name, frames, output| {
				let fps = if name == "PICKUPS" { 2 } else { 30 }; // todo fps
				Texture::save_animated(frames, name, fps, output, palette)
			},
		);

		save_items(
			"3d animations",
			output,
			&self.animations_3d,
			|name, anim, output| anim.save_as(name, output),
		);

		save_items("meshes", output, &self.meshes, |name, mesh, output| {
			mesh.save_as(name, output)
		});

		save_items("palettes", output, &self.palettes, |name, pal, output| {
			output.write_palette(name, pal)
		});

		save_items("strings", output, &self.strings, |name, strings, output| {
			output.write(name, "txt", strings.join("\n"));
		});

		save_items("unknown", output, &self.unknowns, |name, item, output| {
			output.write(name, "data", item)
		});
	}
}

fn try_parse_strings<'a>(reader: &mut Reader<'a>) -> Option<Vec<&'a str>> {
	if let Some(str) = reader.clone().try_str(reader.remaining_len()) {
		return Some(vec![str]);
	}

	let mut result = Vec::with_capacity(reader.len() / 12);
	while reader.remaining_len() >= 12 {
		result.push(reader.try_str(12)?);
	}

	if !reader.is_empty() {
		if reader.try_u32() != Some(0) {
			return None;
		}
		if !reader.is_empty() {
			return None;
		}
	}

	Some(result)
}
