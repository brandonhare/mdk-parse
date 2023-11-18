use std::io;
use std::io::Read;

#[cfg(not(target_endian = "little"))]
compile_error!("big endian not supported!");

#[cfg(feature = "readranges")]
thread_local! {
	pub static READ_RANGE : std::rc::Rc<std::cell::RefCell<ranges::Ranges<usize>>> = Default::default();
}

#[derive(Clone)]
pub struct Reader<'buf> {
	reader: io::Cursor<&'buf [u8]>,
}

pub trait Readable {
	type Buffer: std::fmt::Debug;
	fn new_buffer() -> Self::Buffer;
	fn buffer_as_mut(buf: &mut Self::Buffer) -> &mut [u8] {
		let ptr: *mut Self::Buffer = buf;
		unsafe {
			std::slice::from_raw_parts_mut(ptr as *mut u8, std::mem::size_of::<Self::Buffer>())
		}
	}

	fn convert_big(buf: Self::Buffer) -> Self;
	fn convert_little(buf: Self::Buffer) -> Self;
	#[must_use]
	fn validate(&self) -> bool;
}

#[allow(dead_code)]
impl<'buf> Reader<'buf> {
	pub fn new(buf: &'buf [u8]) -> Reader<'buf> {
		Reader {
			reader: io::Cursor::new(buf),
		}
	}

	fn mark_read(&self, range: std::ops::Range<usize>) {
		#[cfg(feature = "readranges")]
		{
			let origin = self.buf().as_ptr() as usize;
			let range = range.start + origin..range.end + origin;
			READ_RANGE.with(|ranges| ranges.borrow_mut().insert(range));
		}
	}

	pub fn resize(&mut self, range: impl std::ops::RangeBounds<usize>) {
		*self = self.resized(range);
	}
	pub fn resize_pos(&mut self, range: impl std::ops::RangeBounds<usize>, new_pos: usize) {
		*self = self.resized_pos(range, new_pos);
	}
	#[must_use]
	pub fn resized(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
		let start = match range.start_bound() {
			std::ops::Bound::Included(&n) => n,
			std::ops::Bound::Excluded(&n) => n + 1,
			std::ops::Bound::Unbounded => 0,
		};
		let end = match range.end_bound() {
			std::ops::Bound::Included(&n) => n + 1,
			std::ops::Bound::Excluded(&n) => n,
			std::ops::Bound::Unbounded => self.len(),
		};
		Reader::new(&self.buf()[start..end])
	}
	#[must_use]
	pub fn resized_pos(&self, range: impl std::ops::RangeBounds<usize>, new_pos: usize) -> Self {
		let mut result = self.resized(range);
		result.set_position(new_pos);
		result
	}

	#[must_use]
	pub fn clone_at(&self, new_pos: usize) -> Self {
		let mut result = self.clone();
		result.set_position(new_pos);
		result
	}

	pub fn set_end(&mut self, end_pos: usize) {
		self.resize_pos(0..end_pos, self.position());
	}

	pub fn buf(&self) -> &'buf [u8] {
		self.reader.get_ref()
	}
	pub fn remaining_buf(&self) -> &'buf [u8] {
		&self.buf()[self.position()..]
	}

	pub fn len(&self) -> usize {
		self.buf().len()
	}
	pub fn remaining_len(&self) -> usize {
		self.len() - self.position()
	}
	pub fn is_empty(&self) -> bool {
		self.remaining_len() == 0
	}

	pub fn position(&self) -> usize {
		self.reader.position() as usize
	}
	pub fn set_position(&mut self, pos: usize) {
		self.reader.set_position(pos as u64)
	}

	pub fn try_get<T: Readable>(&mut self) -> Option<T> {
		self.try_get_unvalidated().filter(T::validate)
	}
	pub fn try_get_unvalidated<T: Readable>(&mut self) -> Option<T> {
		let mut buffer = T::new_buffer();
		let pos = self.position();
		let buffer_bytes = T::buffer_as_mut(&mut buffer);
		self.reader.read_exact(buffer_bytes).ok()?;
		self.mark_read(pos..pos + buffer_bytes.len());
		let result = if cfg!(target_endian = "little") {
			T::convert_little(buffer)
		} else {
			T::convert_big(buffer)
		};
		Some(result)
	}
	pub fn get<T: Readable + std::fmt::Debug>(&mut self) -> T {
		let start = self.position();
		let end = start + std::mem::size_of::<T::Buffer>();
		let Some(result) = self.try_get_unvalidated::<T>() else {
			panic!(
				"failed to read bytes {start}..{end} (buffer size {})",
				self.len()
			);
		};
		if !result.validate() {
			panic!("invalid value '{result:?}' at {start}..{end}");
		}
		result
	}
	pub fn get_unvalidated<T: Readable + std::fmt::Debug>(&mut self) -> T {
		let start = self.position();
		let end = start + std::mem::size_of::<T::Buffer>();
		let Some(result) = self.try_get_unvalidated::<T>() else {
			panic!(
				"failed to read bytes {start}..{end} (buffer size {})",
				self.len()
			);
		};
		result
	}

	#[must_use]
	pub fn try_skip(&mut self, len: isize) -> Option<()> {
		let end_pos = self.position().checked_add_signed(len)?;
		if (0..=self.len()).contains(&end_pos) {
			self.set_position(end_pos);
			Some(())
		} else {
			None
		}
	}
	pub fn skip(&mut self, len: isize) {
		let start_pos = self.position();
		let ok = self.try_skip(len).is_some();
		assert!(
			ok,
			"failed to skip {len} bytes from {start_pos} (out of range 0..{})",
			self.len()
		);
	}

	pub fn try_align(&mut self, alignment: usize) -> Option<()> {
		debug_assert!(alignment.is_power_of_two() && alignment > 0);
		let pos = self.position();
		let next_position = pos.next_multiple_of(alignment);
		let remainder = next_position - pos;
		if remainder > self.remaining_len() {
			return None;
		}
		if remainder != 0 {
			self.mark_read(pos..pos + next_position);
			self.try_skip(remainder as isize)
		} else {
			Some(())
		}
	}
	pub fn align(&mut self, alignment: usize) {
		self.try_align(alignment).expect("failed to align");
	}

	pub fn slice(&mut self, size: usize) -> &'buf [u8] {
		self.try_slice(size).expect("slice out of range")
	}
	pub fn try_slice(&mut self, size: usize) -> Option<&'buf [u8]> {
		let pos = self.position();
		self.try_skip(size as isize)?;
		self.mark_read(pos..pos + size);
		Some(&self.buf()[pos..pos + size])
	}
	pub fn remaining_slice(&mut self) -> &'buf [u8] {
		self.slice(self.remaining_len())
	}

	pub fn pascal_str(&mut self) -> &'buf str {
		self.try_pascal_str().expect("invalid string")
	}
	pub fn try_pascal_str(&mut self) -> Option<&'buf str> {
		let length = self.try_u8()?;
		self.try_str(length as usize)
	}

	pub fn str(&mut self, size: usize) -> &'buf str {
		self.try_str(size).expect("invalid string")
	}
	pub fn try_str(&mut self, size: usize) -> Option<&'buf str> {
		if size > 100 {
			return None;
		}

		let buf = self.try_slice(size)?;

		let buf = if let Some(local_end_pos) = buf.iter().position(|c| *c == 0) {
			if buf[local_end_pos..].iter().any(|&c| c != 0) {
				return None;
			}
			&buf[..local_end_pos]
		} else {
			buf
		};

		if !buf.iter().all(|&c| {
			matches!(c, b' '..=b'~')
			/*matches!(c,
				b' ' | b'.' | b'-' | b'$' | b'0'..=b'9' | b'?' | b'A'..=b'Z' | b'_' | b'a'..=b'z'
			)*/
		}) {
			return None;
		}

		std::str::from_utf8(buf).ok()
	}

	pub fn u8(&mut self) -> u8 {
		self.get()
	}
	pub fn i8(&mut self) -> i8 {
		self.get()
	}
	pub fn u16(&mut self) -> u16 {
		self.get()
	}
	pub fn i16(&mut self) -> i16 {
		self.get()
	}
	pub fn u32(&mut self) -> u32 {
		self.get()
	}
	pub fn i32(&mut self) -> i32 {
		self.get()
	}
	pub fn u64(&mut self) -> u64 {
		self.get()
	}
	pub fn i64(&mut self) -> i64 {
		self.get()
	}
	pub fn f32(&mut self) -> f32 {
		self.get()
	}
	pub fn f64(&mut self) -> f64 {
		self.get()
	}

	pub fn try_u8(&mut self) -> Option<u8> {
		self.try_get()
	}
	pub fn try_i8(&mut self) -> Option<i8> {
		self.try_get()
	}
	pub fn try_u16(&mut self) -> Option<u16> {
		self.try_get()
	}
	pub fn try_i16(&mut self) -> Option<i16> {
		self.try_get()
	}
	pub fn try_u32(&mut self) -> Option<u32> {
		self.try_get()
	}
	pub fn try_i32(&mut self) -> Option<i32> {
		self.try_get()
	}
	pub fn try_u64(&mut self) -> Option<u64> {
		self.try_get()
	}
	pub fn try_i64(&mut self) -> Option<i64> {
		self.try_get()
	}
	pub fn try_f32(&mut self) -> Option<f32> {
		self.try_get()
	}
	pub fn try_f64(&mut self) -> Option<f64> {
		self.try_get()
	}

	/*
	pub fn arr<T: Readable, const N: usize>(&mut self) -> [T; N] {
		std::array::from_fn(|_| self.get())
	}
	*/

	pub fn get_vec<T: Readable + std::fmt::Debug>(&mut self, count: usize) -> Vec<T> {
		(0..count).map(|_| self.get()).collect()
	}
	pub fn try_get_vec<T: Readable + std::fmt::Debug>(&mut self, count: usize) -> Option<Vec<T>> {
		let mut result = Vec::with_capacity(count);
		for _ in 0..count {
			result.push(self.try_get()?);
		}
		Some(result)
	}
	pub fn try_get_vec_unvalidated<T: Readable + std::fmt::Debug>(
		&mut self, count: usize,
	) -> Option<Vec<T>> {
		let mut result = Vec::with_capacity(count);
		for _ in 0..count {
			result.push(self.try_get_unvalidated()?);
		}
		Some(result)
	}

	pub fn vec2(&mut self) -> [f32; 2] {
		self.get()
	}
	pub fn vec3(&mut self) -> [f32; 3] {
		self.get()
	}
	pub fn vec4(&mut self) -> [f32; 4] {
		self.get()
	}

	pub fn try_vec2(&mut self) -> Option<[f32; 2]> {
		self.try_get()
	}
	pub fn try_vec3(&mut self) -> Option<[f32; 3]> {
		self.try_get()
	}
	pub fn try_vec4(&mut self) -> Option<[f32; 4]> {
		self.try_get()
	}
}

impl<T: Readable, const N: usize> Readable for [T; N] {
	type Buffer = [T::Buffer; N];

	fn new_buffer() -> Self::Buffer {
		assert_eq!(
			std::mem::size_of::<Self::Buffer>(),
			N * std::mem::size_of::<T::Buffer>()
		);
		std::array::from_fn(|_| T::new_buffer())
	}
	fn convert_big(buf: Self::Buffer) -> Self {
		buf.map(T::convert_big)
	}
	fn convert_little(buf: Self::Buffer) -> Self {
		buf.map(T::convert_little)
	}
	fn validate(&self) -> bool {
		self.iter().all(T::validate)
	}
}

macro_rules! make_readable {
	($name:ident, $size:expr, $validate_func:tt) => {
		impl Readable for $name {
			type Buffer = [u8; $size];
			fn new_buffer() -> Self::Buffer {
				Default::default()
			}
			fn convert_big(bytes: Self::Buffer) -> Self {
				$name::from_be_bytes(bytes)
			}
			fn convert_little(bytes: Self::Buffer) -> Self {
				$name::from_le_bytes(bytes)
			}

			fn validate(&self) -> bool {
				($validate_func)(*self)
			}
		}
	};
}

fn validate_int<T>(_: T) -> bool {
	true
}
fn validate_float32(f: f32) -> bool {
	f.is_finite() && (-10000000.0..=10000000.0).contains(&f)
}
fn validate_float64(f: f64) -> bool {
	f.is_finite() && (-10000000.0..=10000000.0).contains(&f)
}

macro_rules! allNums {
	($func:ident) => {
		$func!(i8, 1, validate_int);
		$func!(u8, 1, validate_int);
		$func!(i16, 2, validate_int);
		$func!(u16, 2, validate_int);
		$func!(i32, 4, validate_int);
		$func!(u32, 4, validate_int);
		$func!(i64, 8, validate_int);
		$func!(u64, 8, validate_int);
		$func!(f32, 4, validate_float32);
		$func!(f64, 8, validate_float64);
	};
}
allNums!(make_readable);
