use crate::data_formats::Texture;
use crate::Reader;

pub fn try_parse_animation(reader: &mut Reader) -> Option<Vec<Texture<'static>>> {
	let mut data = reader.clone();
	let filesize = data.try_u32()? as usize;
	if filesize > data.remaining_len() {
		return None;
	}
	data.rebase_length(filesize);

	let num_frames = data.try_u32()? as usize;
	if num_frames == 0 || num_frames > 1000 {
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
		let x = data.try_i16()?;
		let y = data.try_i16()?;

		let mut pixels = vec![0; width as usize * height as usize];
		'row_loop: for row in pixels.chunks_exact_mut(width as usize) {
			let mut col_index = 0;
			loop {
				let count = data.try_u8()?;
				match count {
					0..=0x7F => {
						let count = count as usize + 1;
						if col_index + count > row.len() {
							return None;
						}
						let pixels = data.try_slice(count)?;
						row[col_index..col_index + count].copy_from_slice(pixels);
						col_index += count;
					}
					0x80..=0xFD => {
						let count = count as usize - 0x7C;
						if col_index + count > row.len() {
							return None;
						}
						let value = data.try_u8()?;
						row[col_index..col_index + count].fill(value);
						col_index += count;
					}
					0xFE => continue 'row_loop,
					0xFF => break 'row_loop,
				}
			}
		}

		results.push(Texture {
			width,
			height,
			pixels: pixels.into(),
			position: (x, y),
		});
	}

	// mark source reader as read
	reader.skip(filesize + 4);

	Some(results)
}

pub fn try_parse_basic_image<'a>(reader: &mut Reader<'a>) -> Option<Texture<'a>> {
	let width = reader.try_u16()?;
	let height = reader.try_u16()?;
	let num_pixels = width as usize * height as usize;
	if reader.remaining_len() != num_pixels {
		return None;
	}
	let pixels = reader.slice(num_pixels);
	Some(Texture::new(width, height, pixels))
}

pub fn try_parse_palette_image<'a>(reader: &mut Reader<'a>) -> Option<(&'a [u8], Texture<'a>)> {
	let palette = reader.try_slice(0x300)?;
	let image = try_parse_basic_image(reader)?;
	Some((palette, image))
}

pub fn try_parse_overlay_image(reader: &mut Reader) -> Option<Texture<'static>> {
	let filesize = reader.try_u32()? as usize;
	if reader.remaining_len() != filesize.next_multiple_of(4) {
		return None;
	};

	let width = 600;
	let height = 360;
	let num_pixels = width as usize * height as usize;

	let mut pixels = Vec::with_capacity(num_pixels);
	loop {
		let index = reader.try_u16()?;
		if index & 0x8000 != 0x8000 {
			for _ in 0..4 * index {
				pixels.push(reader.try_u8()?);
			}
			continue;
		}
		if index & 0xFF00 != 0xFF00 {
			pixels.resize_with(pixels.len() + (index as usize & 0xFFF), Default::default);
			continue;
		}
		let index = index & 0xFF;
		if index == 0 {
			break;
		}
		for _ in 0..index {
			pixels.push(reader.try_u8()?);
		}

		if pixels.len() > num_pixels {
			return None;
		}
	}
	reader.align(4);

	if !reader.is_empty() || pixels.len() != num_pixels {
		return None;
	}

	Some(Texture::new(width, height, pixels))
}

pub fn try_parse_rle_image(reader: &mut Reader) -> Option<Texture<'static>> {
	let filesize = reader.try_u32()? as usize;
	if reader.remaining_len() != filesize {
		return None;
	}

	let width = 600;
	let height = 180;
	let num_pixels = width as usize * height as usize;

	let mut pixels = Vec::with_capacity(num_pixels);

	while !reader.is_empty() {
		let num_pixels1 = reader.try_u32().filter(|n| *n <= 10000)? as usize;
		let pixels1 = reader.try_slice(num_pixels1 * 4)?;
		pixels.extend_from_slice(pixels1);

		let num_zeroes = reader.try_u32().filter(|n| *n <= 10000)? as usize;
		pixels.resize(pixels.len() + num_zeroes * 4, 0);

		let num_pixels2 = reader.try_u32().filter(|n| *n <= 10000)? as usize;
		let pixels2 = reader.try_slice(num_pixels2 * 4)?;
		pixels.extend_from_slice(pixels2);

		if pixels.len() > num_pixels {
			return None;
		}
	}

	if pixels.len() != num_pixels {
		return None;
	}

	Some(Texture::new(width, height, pixels))
}

pub fn try_parse_crossfade_image<'a>(
	reader: &mut Reader<'a>,
) -> Option<([&'a [u8]; 2], Texture<'static>)> {
	if reader.len() < 0x600 {
		return None;
	}

	let lut1 = reader.slice(0x300);
	let lut2 = reader.slice(0x300);

	let width = 600;
	let height = 360;
	let num_pixels = width as usize * height as usize;

	let mut pixels = Vec::with_capacity(num_pixels);
	loop {
		let count = reader.try_i8()?;
		if count == 0 {
			break;
		}
		if count < 0 {
			let span = reader.try_slice(count.unsigned_abs() as usize)?;
			pixels.extend_from_slice(span);
		} else {
			let pixel = reader.try_u8()?;
			pixels.resize(pixels.len() + count as usize, pixel);
		}
		if pixels.len() > num_pixels {
			return None;
		}
	}
	if pixels.len() != num_pixels || !reader.is_empty() {
		return None;
	}

	Some(([lut1, lut2], Texture::new(width, height, pixels)))
}
