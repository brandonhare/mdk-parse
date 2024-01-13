use crate::data_formats::Texture;
use crate::{OutputWriter, Reader};

pub enum Material<'a> {
	Pen(Pen),
	Texture(Texture<'a>, MaterialFlags),
	AnimatedTexture(Vec<Texture<'a>>, MaterialFlags),
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Pen {
	Texture(u8),     // index into mesh material array
	Colour(u8),      // index into palette
	Translucent(u8), // index into dti translucent_colours
	Shiny(u8),       // todo
	Unknown(i32),    // todo
}
impl Pen {
	pub fn new(index: i32) -> Pen {
		match index {
			0..=255 => Pen::Texture(index as u8),
			-255..=-1 => Pen::Colour(-index as u8),
			-1010..=-990 => Pen::Shiny((-990 - index) as u8),
			-1027..=-1024 => Pen::Translucent((-1024 - index) as u8),
			..=-1028 => Pen::Unknown(index), // todo
			_ => Pen::Unknown(index),        // todo
		}
	}
}

pub struct MaterialFlags {
	// todo what are these
	pub a: f32,
	pub b: f32,
	pub flags: u32,
}

pub struct Mti<'a> {
	pub filename: &'a str,
	pub materials: Vec<(&'a str, Material<'a>)>,
}

impl<'a> Mti<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Mti<'a> {
		let filesize = reader.u32() as usize;
		let mut reader = {
			let mut new_reader = reader.rebased();
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
				let pen_value = match reader.i32() {
					0 => Pen::Colour(0),
					n @ 1.. => Pen::new(-n), // negate to match mesh tri values
					n => Pen::Unknown(n),    // todo negative?
				};

				let padding1 = reader.u32(); // padding
				let padding2 = reader.u32();
				assert!(padding1 == 0 && padding2 == 0);
				materials.push((name, Material::Pen(pen_value)));
				continue;
			}

			// texture
			let a = reader.f32(); // todo what is this
			let b = reader.f32(); // todo what is this
			let start_offset = reader.u32() as usize;

			let mut entry_reader = reader.clone_at(start_offset);

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
				entry_reader.u32() as usize
			} else {
				1
			};
			let width = entry_reader.u16();
			let height = entry_reader.u16();
			let frame_size = width as usize * height as usize;

			if num_frames == 1 {
				let pixels = entry_reader.slice(frame_size);
				materials.push((
					name,
					Material::Texture(Texture::new(width, height, pixels), matflags),
				));
			} else if flags_mask == 0x10000 {
				// animated sequence
				let frames = (0..num_frames)
					.map(|_| Texture::new(width, height, entry_reader.slice(frame_size)))
					.collect();
				materials.push((name, Material::AnimatedTexture(frames, matflags)));
			} else {
				// overlay animation
				let frames = parse_overlay_animation(&mut entry_reader, width, height, num_frames);
				materials.push((name, Material::AnimatedTexture(frames, matflags)));
			}
		}

		reader.set_position(reader.len() - 12);
		let footer = reader.str(12);
		assert_eq!(filename, footer, "mti footer does not match");

		Mti {
			filename,
			materials,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.materials.is_empty()
	}

	pub fn save(&self, output: &mut OutputWriter, palette: Option<&[u8]>) {
		// todo more granular palette
		use std::fmt::Write;
		let mut pens_summary = String::from("name    \tvalue\n");
		let mut flags_summary = String::from("name    \ta    \tb  \tflags\n");
		let mut has_pens = false;
		let mut has_flags = false;
		for (name, material) in &self.materials {
			match material {
				Material::Pen(pen) => {
					has_pens = true;
					writeln!(pens_summary, "{name:8}\t{pen:?}").unwrap()
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
							"{name:8}\t{:5}\t{:3}\t{:x}",
							flags.a, flags.b, flags.flags
						)
						.unwrap();
					}
				}
			}
		}

		if has_pens {
			output.write("pens", "txt", &pens_summary);
		}
		if has_flags {
			output.write("texture_flags", "txt", &flags_summary);
		}
	}
}

fn parse_overlay_animation<'a>(
	data: &mut Reader<'a>, width: u16, height: u16, num_frames: usize,
) -> Vec<Texture<'a>> {
	let base_pixels = data.slice(width as usize * height as usize);

	let mut frames: Vec<Texture> = Vec::with_capacity(num_frames + 1);
	frames.push(Texture::new(width, height, base_pixels));

	let _runtime_anim_time = data.u32();

	let mut data = data.rebased(); // offsets relative to here
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

		frames.push(Texture::new(width, height, dest_pixels));
	}

	if frames.first() == frames.last() {
		frames.pop();
	} else {
		eprintln!("texture doesn't loop properly!");
	}

	frames
}
