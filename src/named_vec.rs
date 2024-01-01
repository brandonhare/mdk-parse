use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

pub type NamedValue<'a, T> = (Cow<'a, str>, T);

#[derive(Default, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct NamedVec<'a, T> {
	pub values: Vec<NamedValue<'a, T>>,
}

impl<'a, T: std::fmt::Debug> std::fmt::Debug for NamedVec<'a, T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_map().entries(self.iter()).finish()
	}
}

impl<'a, T> NamedVec<'a, T> {
	pub fn new() -> Self {
		Self { values: Vec::new() }
	}
	pub fn with_capacity(capacity: usize) -> Self {
		Self {
			values: Vec::with_capacity(capacity),
		}
	}
	pub fn get(&self, name: &str) -> Option<&NamedValue<'a, T>> {
		self.values.iter().find(|entry| entry.0 == name)
	}
	pub fn get_mut(&mut self, name: &str) -> Option<&mut NamedValue<'a, T>> {
		self.values.iter_mut().find(|entry| entry.0 == name)
	}
	pub fn insert<S: Into<Cow<'a, str>>>(&mut self, name: S, value: T) {
		self.values.push((name.into(), value));
	}

	pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
		self.values
			.iter()
			.map(|(name, value)| (name.deref(), value))
	}
}
impl<'a, T, S: Into<Cow<'a, str>>> Extend<(S, T)> for NamedVec<'a, T> {
	fn extend<Iter: IntoIterator<Item = (S, T)>>(&mut self, iter: Iter) {
		self.values
			.extend(iter.into_iter().map(|(name, value)| (name.into(), value)))
	}
}
impl<'a, T, S: Into<Cow<'a, str>>> FromIterator<(S, T)> for NamedVec<'a, T> {
	fn from_iter<Iter: IntoIterator<Item = (S, T)>>(iter: Iter) -> Self {
		Self {
			values: iter
				.into_iter()
				.map(|(name, value)| (name.into(), value))
				.collect(),
		}
	}
}

impl<'a, T> std::ops::Index<&str> for NamedVec<'a, T> {
	type Output = NamedValue<'a, T>;
	fn index(&self, name: &str) -> &Self::Output {
		if let Some(result) = self.get(name) {
			result
		} else {
			panic!("no entry named {name}");
		}
	}
}
impl<'a, T> std::ops::IndexMut<&str> for NamedVec<'a, T> {
	fn index_mut(&mut self, name: &str) -> &mut Self::Output {
		if let Some(result) = self.get_mut(name) {
			result
		} else {
			panic!("no entry named {name}");
		}
	}
}

impl<'a, T> Deref for NamedVec<'a, T> {
	type Target = Vec<NamedValue<'a, T>>;
	fn deref(&self) -> &Self::Target {
		&self.values
	}
}
impl<'a, T> DerefMut for NamedVec<'a, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.values
	}
}
