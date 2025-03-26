use crate::data_formats::{Texture, image_formats};
use crate::reader::Reader;

/// LBB files are the loading images for each level.
pub struct Lbb<'a> {
	pub palette: &'a [u8],
	pub texture: Texture<'a>,
}
impl<'a> Lbb<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Self {
		// no actual file structure here, just a raw palette image
		let (palette, texture) = image_formats::try_parse_palette_image(&mut reader).unwrap();
		Self { palette, texture }
	}
}
