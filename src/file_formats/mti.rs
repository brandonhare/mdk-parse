use crate::{data_formats::Texture, output_writer::OutputWriter, Reader};

#[derive(Debug)]
pub enum Material<'a> {
	Pen(i32), // todo split
	Texture(Texture<'a>, MaterialFlags),
	AnimatedTexture(Vec<Texture<'a>>, MaterialFlags),
}

#[derive(Debug)]
pub struct MaterialFlags {
	// todo what are these
	pub a: f32,
	pub b: f32,
	pub flags: u32,
}

#[derive(Debug)]
pub struct Mti<'a> {
	pub materials: Vec<(&'a str, Material<'a>)>,
}

impl<'a> Mti<'a> {
	pub fn parse(reader: &mut Reader<'a>) -> Mti<'a> {
		let filesize = reader.u32() as usize;
		let mut reader = {
			let mut new_reader = reader.rebased_start();
			reader.skip(filesize);
			new_reader.set_end(filesize);
			new_reader
		};

		let filename = reader.str(12);
		let filesize2 = reader.u32() as usize;
		assert_eq!(filesize, filesize2 + 8, "filesizes do not match");
		let num_entries = reader.u32() as usize;

		let mut materials: Vec<(&str, Material)> = Vec::with_capacity(num_entries);

		for _ in 0..num_entries {
			let name = reader.str(8);
			let flags = reader.u32();

			if flags == 0xFFFFFFFF {
				// pen
				let pen_value = reader.i32();
				let _ = reader.u32(); // padding
				let _ = reader.u32();
				materials.push((name, Material::Pen(pen_value)));
				continue;
			}

			// texture
			let a = reader.f32(); // todo what is this
			let b = reader.f32(); // todo what is this
			let start_offset = reader.u32() as usize;
			let mut data = reader.clone_at(start_offset);

			let flags_mask = flags & 0x30000; // if the texture is animated or not
			let flags_rest = flags & !0x30000; // todo what are these

			let matflags = MaterialFlags {
				a,
				b,
				flags: flags_rest,
			};

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
				materials.push((
					name,
					Material::Texture(
						Texture {
							width,
							height,
							pixels: data.slice(frame_size).into(),
						},
						matflags,
					),
				));
			} else if flags_mask == 0x10000 {
				// animated sequence
				let frames = (0..num_frames)
					.map(|_| Texture {
						width,
						height,
						pixels: data.slice(frame_size).into(),
					})
					.collect();
				materials.push((name, Material::AnimatedTexture(frames, matflags)));
			} else {
				// compressed animation
				let base_pixels = data.slice(frame_size);

				let mut frames: Vec<Texture> = Vec::with_capacity(num_frames + 1);
				frames.push(Texture {
					width,
					height,
					pixels: base_pixels.into(),
				});

				let _runtime_anim_time = data.u32();

				let mut data = data.resized(data.position()..); // offsets relative to here
				let offsets = data.get_vec::<u32>(num_frames * 2); // run of meta offsets then run of pixels offsets
				for (&metadata_offset, &pixel_offset) in
					offsets[..num_frames].iter().zip(&offsets[num_frames..])
				{
					let mut meta = data.clone_at(metadata_offset as usize);
					let mut src_pixels = data.clone_at(pixel_offset as usize);

					let mut dest_pixels = frames.last().unwrap().pixels.clone().into_owned();

					let mut dest_pixel_offset = meta.u16() as usize * 4;
					let num_chunks = meta.u16();

					for _ in 0..num_chunks {
						let chunk_size = meta.u8() as usize * 4;
						let output_offset = meta.u8() as usize * 4;
						dest_pixels[dest_pixel_offset..dest_pixel_offset + chunk_size]
							.clone_from_slice(src_pixels.slice(chunk_size));
						dest_pixel_offset += chunk_size + output_offset;
					}

					frames.push(Texture {
						width,
						height,
						pixels: dest_pixels.into(),
					});
				}

				if frames.first() == frames.last() {
					frames.pop();
				} else {
					eprintln!("texture {name} doesn't loop properly!");
				}

				materials.push((name, Material::AnimatedTexture(frames, matflags)));
			}
		}

		reader.set_position(reader.len() - 12);
		let footer = reader.str(12);
		assert_eq!(filename, footer, "mti footer does not match");

		Mti { materials }
	}

	pub fn save(&self, output: &mut OutputWriter, palette: Option<&[u8]>) {
		// todo more granular palette
		use std::fmt::Write;
		let mut pens_summary = String::from("name\tvalue\n");
		let mut flags_summary = String::from("name\ta\tb\tflags\n");
		let mut has_pens = false;
		let mut has_flags = false;
		for (name, material) in &self.materials {
			match material {
				Material::Pen(n) => {
					has_pens = true;
					writeln!(pens_summary, "{name}\t{n}").unwrap()
				}
				Material::Texture(texture, _) => {
					texture.save_as(name, output, palette);
				}
				Material::AnimatedTexture(frames, _) => {
					Texture::save_animated(frames, name, 12, output, palette);
				}
			}
			match material {
				Material::Pen(_) => {}
				Material::Texture(_, flags) | Material::AnimatedTexture(_, flags) => {
					if flags.a != 0.0 || flags.b != 3.5 || flags.flags != 0 {
						has_flags = true;
						writeln!(
							flags_summary,
							"{name}\t{}\t{}\t{}",
							flags.a, flags.b, flags.flags
						)
						.unwrap();
					}
				}
			}
		}

		if has_pens {
			output.write("pens", "tsv", &pens_summary);
		}
		if has_flags {
			output.write("texture_flags", "tsv", &flags_summary);
		}
	}
}
