use crate::{OutputWriter, Reader, Vec3};

pub struct Spline {
	pub points: Vec<SplinePoint>,
}

pub struct SplinePoint {
	pub t: i32,
	pub pos1: Vec3,
	pub pos2: Vec3,
	pub pos3: Vec3,
}

impl Spline {
	pub fn parse(reader: &mut Reader) -> Self {
		let count = reader.u32() as usize;
		let mut points = Vec::with_capacity(count);
		for _ in 0..count {
			let t = reader.i32();
			let pos1 = reader.vec3();
			let pos2 = reader.vec3();
			let pos3 = reader.vec3();
			points.push(SplinePoint {
				t,
				pos1,
				pos2,
				pos3,
			});
		}
		// todo transform
		Spline { points }
	}

	pub fn save_as(&self, _name: &str, _output: &mut OutputWriter) {
		// todo
	}
}
