use std::borrow::Cow;

use crate::data_formats::{try_parse_animation, Texture, Wav};
use crate::{OutputWriter, Reader};

pub struct FontLetter<Pixels: AsRef<[u8]>> {
	pub code: u8,
	pub width: u8,
	pub height: u8,
	pub pixels: Pixels,
}

pub struct Fti<'a> {
	pub arrow: Texture<'a>,
	pub palette: &'a [u8],
	pub snd_push: Option<Wav<'a>>,
	pub font_big: Vec<FontLetter<&'a [u8]>>,
	pub font_sml: Vec<FontLetter<&'a [u8]>>,
	pub font_8: Vec<FontLetter<Vec<u8>>>,
	pub strings: Vec<(&'a str, Cow<'a, str>)>,
}

impl<'a> Fti<'a> {
	pub fn parse(mut data: Reader<'a>) -> Fti<'a> {
		let filesize = data.u32() + 4;
		assert_eq!(data.len(), filesize as usize, "filesize does not match");
		data.resize(4..);

		let mut arrow = None;
		let mut palette = None;
		let mut snd_push = None;
		let mut font_big = None;
		let mut font_sml = None;
		let mut font_8 = None;
		let mut strings = Vec::new();

		let num_items = data.u32();
		let mut prev_end = data.position();
		for _ in 0..num_items {
			let name = data.str(8);
			let offset = data.u32() as usize;
			assert!(offset >= prev_end, "overlapped items!");

			let mut reader = data.clone_at(offset);

			match name {
				"ARROW" => {
					let frames = try_parse_animation(&mut reader).unwrap();
					assert!(frames.len() == 1);
					arrow = Some(frames.into_iter().next().unwrap());
				}
				"SND_PUSH" => {
					snd_push = Some(Wav::parse(&mut reader));
				}
				"SYS_PAL" => {
					palette = Some(reader.slice(64 * 3));
				}
				"F8" => {
					font_8 = Some(parse_small_font(&mut reader));
				}
				"FONTBIG" => {
					font_big = Some(parse_font_letters(&mut reader));
				}
				"FONTSML" => {
					font_sml = Some(parse_font_letters(&mut reader));
				}
				_ => {
					strings.push((name, parse_string(&mut reader)));
				}
			}

			prev_end = reader.position();
		}

		Fti {
			arrow: arrow.unwrap(),
			palette: palette.unwrap(),
			snd_push,
			font_big: font_big.unwrap(),
			font_sml: font_sml.unwrap(),
			font_8: font_8.unwrap(),
			strings,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		output.write_png(
			"ARROW",
			self.arrow.width as u32,
			self.arrow.height as u32,
			&self.arrow.pixels,
			Some(self.palette),
		); // todo use a save_as function
		output.write_palette("SYS_PAL", self.palette);
		if let Some(ref snd_push) = self.snd_push {
			snd_push.save_as("SND_PUSH", output);
		}
		save_font_as("FONTBIG", &self.font_big, output, self.palette);
		save_font_as("FONTSML", &self.font_sml, output, self.palette);
		save_font_as("F8", &self.font_8, output, self.palette);

		let mut strings = String::from("name\tvalue\n");
		for (name, string) in &self.strings {
			use std::fmt::Write;
			writeln!(strings, "{name}\t{string}").unwrap();
		}
		output.write("strings", "tsv", &strings);
	}
}

fn parse_string<'a>(reader: &mut Reader<'a>) -> Cow<'a, str> {
	let buf = reader.remaining_buf();
	for (i, c) in buf.iter().enumerate() {
		match *c {
			0 => return Cow::Borrowed(std::str::from_utf8(reader.slice(i)).unwrap()),
			b'\\' if !matches!(buf[i + 1], b'n' | b't' | b'c') => {}
			b' '..=b'~' => continue,
			_ => {}
		}
		// non-simple char, start building a new string then return
		let (prev, next) = buf.split_at(i);
		let mut result: String = std::str::from_utf8(prev).unwrap().to_owned();
		for (i, &c) in (i..).zip(next) {
			match c {
				0 => {
					let _ = reader.slice(i + 1); // mark as read
					return Cow::Owned(result);
				}
				b'\n' => result.push_str("\\n"),
				b'\t' => result.push_str("\\t"),
				b'\r' => result.push_str("\\r"),
				b' '..=b'~' => result.push(c as char),
				149 => result.push('ę'),
				150 => result.push('ń'),
				230 => result.push('ć'),
				c => panic!("unknown char {c}"),
			}
		}
		break;
	}
	panic!("string had no nul terminator!");
}

fn parse_font_letters<'a>(data: &mut Reader<'a>) -> Vec<FontLetter<&'a [u8]>> {
	let mut result = Vec::with_capacity(256);
	let start_pos = data.position();
	let mut last_pos = 0;
	for code in 0..=255 {
		let offset = data.u32() as usize;
		if offset == 0 {
			continue;
		}
		let mut data = data.clone_at(start_pos + offset);

		let height_base = data.i8();
		let height_offset = data.i8();
		let height = (height_base + height_offset + 1) as u8;
		let width = data.u8();

		let pixels = data.slice(width as usize * height as usize);

		last_pos = last_pos.max(data.position());

		result.push(FontLetter {
			code,
			width,
			height,
			pixels,
		});
	}
	data.set_position(last_pos);
	result
}

fn parse_small_font(reader: &mut Reader) -> Vec<FontLetter<Vec<u8>>> {
	(0..16 * 8)
		.map(|code| {
			let mut pixels = vec![0; 8 * 8];

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
				code,
				width: 8,
				height: 8,
				pixels,
			}
		})
		.collect()
}

fn save_font_as<Pixels: AsRef<[u8]>>(
	name: &str, font: &[FontLetter<Pixels>], output: &mut OutputWriter, pal: &[u8],
) {
	let (cell_width, cell_height, max_code) =
		font.iter().fold((0, 0, 0), |(width, height, max), letter| {
			(
				width.max(letter.width as usize),
				height.max(letter.height as usize),
				max.max(letter.code),
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

	for letter in font {
		let col_index = letter.code as usize % cells_per_row;
		let row_index = letter.code as usize / cells_per_row;
		let result = &mut result[row_index * row_stride + col_index * cell_width..];
		for (dest, src) in result
			.chunks_mut(row_width)
			.zip(letter.pixels.as_ref().chunks_exact(letter.width as usize))
		{
			dest[..letter.width as usize].copy_from_slice(src);
		}
	}

	output.write_png(
		name,
		row_width as u32,
		(num_rows * cell_height) as u32,
		&result,
		Some(pal),
	)
}
