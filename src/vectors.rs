use std::fmt::Write;
use std::ops::{Add, AddAssign, Deref, DerefMut, Mul, MulAssign, Sub, SubAssign};

pub type Vec2 = [f32; 2];
pub type Vec4 = [f32; 4];

#[derive(Default, Copy, Clone, PartialEq, PartialOrd)]
#[repr(C)]
pub struct Vec3 {
	pub x: f32,
	pub y: f32,
	pub z: f32,
}

impl Vec3 {
	pub const fn new(x: f32, y: f32, z: f32) -> Self {
		Self { x, y, z }
	}
	pub const fn new_splat(value: f32) -> Self {
		Self {
			x: value,
			y: value,
			z: value,
		}
	}

	pub const fn to_array(self) -> [f32; 3] {
		[self.x, self.y, self.z]
	}
	pub const fn from_array([x, y, z]: [f32; 3]) -> Self {
		Self { x, y, z }
	}

	#[must_use]
	pub fn swizzle(self) -> Self {
		Self {
			x: self.x,
			y: self.z,
			z: -self.y,
		}
	}

	pub fn swizzle_slice(points: &mut [Vec3]) {
		for p in points {
			*p = p.swizzle();
		}
	}

	pub fn swizzle_vec(mut points: Vec<Vec3>) -> Vec<Vec3> {
		Self::swizzle_slice(&mut points);
		points
	}

	pub fn calculate_bbox(points: &[Vec3]) -> [Vec3; 2] {
		let mut min = Vec3::new_splat(f32::INFINITY);
		let mut max = Vec3::new_splat(f32::NEG_INFINITY);
		for point in points {
			for i in 0..3 {
				min[i] = min[i].min(point[i]);
				max[i] = max[i].max(point[i]);
			}
		}
		[min, max]
	}

	pub fn is_zero(&self) -> bool {
		*self == Default::default()
	}
}

impl From<[f32; 3]> for Vec3 {
	fn from(array: [f32; 3]) -> Self {
		Self::from_array(array)
	}
}
impl From<&[f32; 3]> for Vec3 {
	fn from(array: &[f32; 3]) -> Self {
		Self::from_array(*array)
	}
}
impl From<Vec3> for [f32; 3] {
	fn from(value: Vec3) -> Self {
		value.to_array()
	}
}
impl From<&Vec3> for [f32; 3] {
	fn from(value: &Vec3) -> Self {
		value.to_array()
	}
}

impl AsRef<[f32; 3]> for Vec3 {
	fn as_ref(&self) -> &[f32; 3] {
		self.deref()
	}
}
impl AsRef<Vec3> for [f32; 3] {
	fn as_ref(&self) -> &Vec3 {
		unsafe { std::mem::transmute(self) }
	}
}

impl Deref for Vec3 {
	type Target = [f32; 3];
	fn deref(&self) -> &Self::Target {
		unsafe { std::mem::transmute(self) }
	}
}
impl DerefMut for Vec3 {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { std::mem::transmute(self) }
	}
}

impl std::fmt::Display for Vec3 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_char('(')?;
		std::fmt::Display::fmt(&self.x, f)?;
		f.write_str(", ")?;
		std::fmt::Display::fmt(&self.y, f)?;
		f.write_str(", ")?;
		std::fmt::Display::fmt(&self.z, f)?;
		f.write_char(')')
	}
}
impl std::fmt::Debug for Vec3 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_tuple("Vec3")
			.field(&self.x)
			.field(&self.y)
			.field(&self.z)
			.finish()
	}
}

impl AddAssign for Vec3 {
	fn add_assign(&mut self, rhs: Self) {
		self.x += rhs.x;
		self.y += rhs.y;
		self.z += rhs.z;
	}
}
impl SubAssign for Vec3 {
	fn sub_assign(&mut self, rhs: Self) {
		self.x -= rhs.x;
		self.y -= rhs.y;
		self.z -= rhs.z;
	}
}
impl Add for Vec3 {
	type Output = Vec3;
	fn add(self, rhs: Self) -> Self::Output {
		Vec3 {
			x: self.x + rhs.x,
			y: self.y + rhs.y,
			z: self.z + rhs.z,
		}
	}
}
impl Sub for Vec3 {
	type Output = Vec3;
	fn sub(self, rhs: Self) -> Self::Output {
		Vec3 {
			x: self.x - rhs.x,
			y: self.y - rhs.y,
			z: self.z - rhs.z,
		}
	}
}
impl MulAssign for Vec3 {
	fn mul_assign(&mut self, rhs: Vec3) {
		self.x *= rhs.x;
		self.y *= rhs.y;
		self.z *= rhs.z;
	}
}
impl Mul for Vec3 {
	type Output = Vec3;
	fn mul(self, rhs: Vec3) -> Self::Output {
		Vec3 {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
			z: self.z * rhs.z,
		}
	}
}
impl MulAssign<f32> for Vec3 {
	fn mul_assign(&mut self, rhs: f32) {
		self.x *= rhs;
		self.y *= rhs;
		self.z *= rhs;
	}
}
impl Mul<f32> for Vec3 {
	type Output = Vec3;
	fn mul(self, rhs: f32) -> Self::Output {
		Vec3 {
			x: self.x * rhs,
			y: self.y * rhs,
			z: self.z * rhs,
		}
	}
}

impl serde::Serialize for Vec3 {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		self.to_array().serialize(serializer)
	}
}
