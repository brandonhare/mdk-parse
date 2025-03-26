use crate::data_formats::image_formats::{parse_basic_image, parse_overlay_animation};
use crate::data_formats::{Pen, Texture};
use crate::{OutputWriter, Reader};

/// MTI files just store materials, containing both texture data
/// and giving names to Pens (flat colours or engine materials)
pub struct Mti<'a> {
	pub filename: &'a str,
	pub materials: Vec<(&'a str, Material<'a>)>,
}

pub enum Material<'a> {
	Pen(Pen),
	Texture(Texture<'a>, MaterialFlags),
	AnimatedTexture(Vec<Texture<'a>>, MaterialFlags),
}

/// Some metadata per material, no idea what most of it means
pub struct MaterialFlags {
	pub a: f32,
	pub b: f32,
	pub flags: u32,
}

impl<'a> Mti<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Mti<'a> {
		let filesize = reader.u32() as usize;
		reader.rebase(); // set this position as the new origin of the file (for offsets)
		reader.set_end(filesize);

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

			const MAT_TYPE_IMAGE: u32 = 0;
			const MAT_TYPE_AMIMATED: u32 = 1;
			const MAT_TYPE_OVERLAY_IMAGE: u32 = 2;
			let mat_type = (flags >> 16) & 3;
			let flags_rest = flags & !0x30000;

			let matflags = MaterialFlags {
				a,
				b,
				flags: flags_rest,
			};

			let mut entry_reader = reader.clone_at(start_offset);
			let result = match mat_type {
				MAT_TYPE_IMAGE => Material::Texture(parse_basic_image(&mut entry_reader), matflags),
				MAT_TYPE_AMIMATED => {
					let num_frames = entry_reader.u32();
					let width = entry_reader.u16();
					let height = entry_reader.u16();
					let frame_size = width as usize * height as usize;
					let frames = (0..num_frames)
						.map(|_| Texture::new(width, height, entry_reader.slice(frame_size)))
						.collect();
					Material::AnimatedTexture(frames, matflags)
				}
				MAT_TYPE_OVERLAY_IMAGE => {
					// this is only used for the M_COMM terminal thing that calls the bomber aircraft
					let frames = parse_overlay_animation(&mut entry_reader);
					Material::AnimatedTexture(frames, matflags)
				}
				_ => panic!("unknown mti material type on {name}"),
			};

			materials.push((name, result));
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
		for (name, material) in &self.materials {
			match material {
				Material::Pen(_) => {}
				Material::Texture(texture, _) => {
					texture.save_as(name, output, palette);
				}
				Material::AnimatedTexture(frames, _) => {
					Texture::save_animated(frames, name, 12, output, palette);
				}
			}
		}
		self.save_report(output);
	}

	pub fn save_report(&self, output: &mut OutputWriter) {
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
