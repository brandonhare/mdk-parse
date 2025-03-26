#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Pen {
	Texture(u8),     // index into mesh material array
	Colour(u8),      // index into palette
	Translucent(u8), // index into dti translucent_colours
	Shiny(u8), // value contains the 'angle' of the shiny material (y-offset of the reflected texture)
	Unknown(i32), // todo
}
impl Pen {
	pub fn new(index: i32) -> Pen {
		match index {
			0..=255 => Pen::Texture(index as u8),
			-255..=-1 => Pen::Colour(-index as u8),
			-1010..=-990 => Pen::Shiny((-990 - index) as u8),
			-1027..=-1024 => Pen::Translucent((-1024 - index) as u8),
			_ => Pen::Unknown(index), // todo
		}
	}
}
