#![allow(unused)]
use std::convert;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::mem::size_of;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;

#[derive(Clone)]
pub struct Reader<'buf> {
	reader: io::Cursor<&'buf [u8]>,
	big_endian: bool,
}

impl<'buf> Deref for Reader<'buf> {
	type Target = io::Cursor<&'buf [u8]>;
	fn deref(&self) -> &Self::Target {
		&self.reader
	}
}
impl<'buf> DerefMut for Reader<'buf> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.reader
	}
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

impl<'buf> Reader<'buf> {
	pub fn read(filename: &Path) -> Vec<u8> {
		fs::read(filename).unwrap()
	}
	pub fn new(buf: &'buf [u8], big_endian: bool) -> Reader<'buf> {
		Reader {
			reader: io::Cursor::new(buf),
			big_endian,
		}
	}
	/*
	pub fn open(filename : &str, big_endian : bool) -> (Vec<u8>, Reader) {
		Reader {
			reader : io::Cursor::new(fs::read(filename).unwrap().into_boxed_slice()),
			big_endian
		}
	}
	*/

	/*
	pub fn clone_at(&self, absolute_offset: u64) -> Self {
		let mut result = self.clone();
		result.set_position(absolute_offset);
		result
	}
	*/

	pub fn truncate2(&mut self, end: u64) {
		self.resize(self.position()..end);
	}
	pub fn resize(&mut self, range: std::ops::Range<u64>) {
		*self = self.resized(range);
	}

	#[must_use]
	pub fn truncated2(&self, end: u64) -> Self {
		self.resized(self.position()..end)
	}
	#[must_use]
	pub fn resized(&self, range: std::ops::Range<u64>) -> Self {
		let start_index = self.position() - range.start;
		let mut result = self.resized_zero(range);
		result.set_position(start_index);
		result
	}
	#[must_use]
	pub fn resized_zero(&self, range: std::ops::Range<u64>) -> Self {
		Reader::new(
			&self.buf()[range.start as usize..range.end as usize],
			self.big_endian,
		)
	}

	pub fn buf(&self) -> &'buf [u8] {
		self.get_ref()
	}
	pub fn remaining_buf(&self) -> &'buf [u8] {
		&self.buf()[self.position() as usize..]
	}

	pub fn len(&self) -> u64 {
		self.buf().len() as u64
	}
	pub fn remaining_len(&self) -> u64 {
		self.len() - self.position()
	}

	pub fn try_get<T: Readable>(&mut self) -> Option<T> {
		let result: T = self.try_get_unvalidated()?;
		if result.validate() {
			Some(result)
		} else {
			None
		}
	}
	pub fn try_get_unvalidated<T: Readable>(&mut self) -> Option<T> {
		let mut buffer = T::new_buffer();
		let buffer_bytes = T::buffer_as_mut(&mut buffer);
		self.reader.read_exact(buffer_bytes).ok()?;
		let result = if (self.big_endian) {
			T::convert_big(buffer)
		} else {
			T::convert_little(buffer)
		};
		Some(result)
	}

	pub fn get<T: Readable + std::fmt::Debug>(&mut self) -> T {
		let pos = self.position();
		let mut buffer = T::new_buffer();
		let buffer_bytes = T::buffer_as_mut(&mut buffer);
		let len = buffer_bytes.len();
		self.reader.read_exact(buffer_bytes).unwrap();
		let result = if (self.big_endian) {
			T::convert_big(buffer)
		} else {
			T::convert_little(buffer)
		};
		assert!(
			result.validate(),
			"invalid value '{result:?}' at index {pos}..{}",
			pos + len as u64
		);
		result
	}

	pub fn skip(&mut self, len: i64) -> Option<()> {
		self.reader
			.seek(SeekFrom::Current(len))
			.is_ok_and(|n| n <= self.len())
			.then_some(())
	}

	pub fn slice(&mut self, size: usize) -> &'buf [u8] {
		self.try_slice(size).expect("slice out of range")
	}
	pub fn try_slice(&mut self, size: usize) -> Option<&'buf [u8]> {
		if !self
			.position()
			.checked_add(size as u64)
			.is_some_and(|end| end <= self.len())
		{
			return None;
		}
		let result = &self.buf()[self.position() as usize..self.position() as usize + size];
		self.skip(size as i64);
		Some(result)
	}

	/*
	pub fn object_slice<T : Readable>(&mut self, count: usize) -> &'buf [T] {
		assert!(self.big_endian != cfg!(target_endian = "little"));
		let byte_slice = self.slice(count * std::mem::size_of::<T>());
		let result = unsafe { std::slice::from_raw_parts(byte_slice.as_ptr() as *const T, count) };
		assert!(result.iter().all(T::validate));
		result
	}
	*/

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
			matches!(c,
				b'.' | b'-' | b'$' | b'0'..=b'9' | b'A'..=b'Z' | b'_' | b'a'..=b'z'
			)
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
	f.is_finite() && (-100000.0..100000.0).contains(&f)
}
fn validate_float64(f: f64) -> bool {
	f.is_finite() && (-100000.0..100000.0).contains(&f)
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
