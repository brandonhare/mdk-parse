use crate::{OutputWriter, Reader, Vec3};
use std::fmt::Write;

/// 3D Spline type used for CMI paths
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
		assert!(count >= 2, "found spline with invalid length");
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

	pub fn save_as(&self, name: &str, output: &mut OutputWriter) {
		// todo transform to actual bezier curves
		let mut data = String::from("time\tpos1\tpos2\tpos3\n");
		for SplinePoint {
			t,
			pos1,
			pos2,
			pos3,
		} in &self.points
		{
			writeln!(&mut data, "{t}\t{pos1}\t{pos2}\t{pos3}").unwrap();
		}
		output.write(name, "tsv", data);
	}
}
