use std::mem;

use serde::{Serialize, Serializer};

#[derive(Serialize)]
struct Asset {
	version: &'static str,
}
impl Default for Asset {
	fn default() -> Self {
		Self { version: "2.0" }
	}
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Mesh {
	name: String,
	primitives: Vec<Primitive>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Primitive {
	indices: AccessorIndex,
	attributes: Attributes,
	#[serde(skip_serializing_if = "Option::is_none")]
	material: Option<MaterialIndex>,
}
#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
struct Attributes {
	position: AccessorIndex,
	#[serde(skip_serializing_if = "Option::is_none")]
	texcoord_0: Option<AccessorIndex>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Material {
	name: String,
	pbr_metallic_roughness: PbrMetallicRoughness,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum PbrMetallicRoughness {
	BaseColorTexture(TextureInfo),
	BaseColorFactor([f32; 4]),
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TextureInfo {
	index: TextureIndex,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Texture {
	name: String,
	sampler: usize,
	source: ImageIndex,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Image {
	uri: String,
	name: String,
}

const FILTER_NEAREST: isize = 9728;
//const FILTER_LINEAR: isize = 9729;
//const WRAP_CLAMP: isize = 33071;
const WRAP_REPEAT: isize = 10497;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sampler {
	mag_filter: isize,
	min_filter: isize,
	wrap_s: isize,
	wrap_t: isize,
}
impl Default for Sampler {
	fn default() -> Self {
		Self {
			mag_filter: FILTER_NEAREST,
			min_filter: FILTER_NEAREST,
			wrap_s: WRAP_REPEAT,
			wrap_t: WRAP_REPEAT,
		}
	}
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
	buffer_view: BufferViewIndex,
	component_type: usize,
	count: usize,
	#[serde(rename = "type")]
	element_type: &'static str,
	#[serde(skip)]
	usage: PrimitiveUsage,
	min: Vec<f64>,
	max: Vec<f64>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Buffer {
	#[serde(serialize_with = "serialize_buffer_uri")]
	uri: Vec<u8>,
	byte_length: usize,
}

fn serialize_buffer_uri<S: Serializer>(uri: &[u8], s: S) -> Result<S::Ok, S::Error> {
	s.serialize_str(&to_uri(uri))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BufferView {
	buffer: BufferIndex,
	byte_length: usize,
	target: usize,
	byte_offset: usize,
	//pub byte_stride : Option<usize>
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Scene {
	name: String,
	nodes: [NodeIndex; 1],
}
impl Default for Scene {
	fn default() -> Self {
		Self {
			name: Default::default(),
			nodes: [NodeIndex(0)],
		}
	}
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Node {
	name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	mesh: Option<MeshIndex>,
	#[serde(skip_serializing_if = "Option::is_none")]
	translation: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	children: Vec<NodeIndex>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct MeshIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct MaterialIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
struct BufferIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
struct BufferViewIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct AccessorIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
struct ImageIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
struct TextureIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
struct NodeIndex(usize);

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Gltf {
	asset: Asset,
	scene: usize,
	scenes: [Scene; 1],
	#[serde(skip_serializing_if = "Vec::is_empty")]
	nodes: Vec<Node>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	meshes: Vec<Mesh>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	materials: Vec<Material>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	textures: Vec<Texture>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	images: Vec<Image>,
	samplers: [Sampler; 1],
	#[serde(skip_serializing_if = "Vec::is_empty")]
	accessors: Vec<Accessor>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	buffers: Vec<Buffer>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	buffer_views: Vec<BufferView>,
}

pub enum PrimitiveUsage {
	Indices,
	Positions,
	UVs,
}

impl Gltf {
	pub fn new(name: String) -> Self {
		let mut result = Self::default();
		result.scenes[0].name = name.clone();
		result.nodes.push(Node {
			name,
			mesh: None,
			translation: None,
			children: Vec::new(),
		});
		result
	}
	pub fn add_colour(&mut self, name: String, colour: [f32; 4]) -> MaterialIndex {
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::BaseColorFactor(colour),
		});
		MaterialIndex(self.materials.len() - 1)
	}
	pub fn add_texture(&mut self, name: String, relative_filename: String) -> MaterialIndex {
		let image_index = ImageIndex(self.images.len());
		self.images.push(Image {
			name: name.clone(),
			uri: relative_filename,
		});
		let texture_index = TextureIndex(self.textures.len());
		self.textures.push(Texture {
			name: name.clone(),
			sampler: 0,
			source: image_index,
		});

		let material_index = MaterialIndex(self.materials.len());
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::BaseColorTexture(TextureInfo {
				index: texture_index,
			}),
		});
		material_index
	}

	pub fn add_positions(&mut self, data: &[[f32; 3]]) -> AccessorIndex {
		self.add_data(data, PrimitiveUsage::Positions)
	}
	pub fn add_uvs(&mut self, data: &[[f32; 2]]) -> AccessorIndex {
		self.add_data(data, PrimitiveUsage::UVs)
	}
	pub fn add_indices(&mut self, data: &[u16]) -> AccessorIndex {
		self.add_data(data, PrimitiveUsage::Indices)
	}
	pub fn add_data<T: BufferData>(&mut self, data: &[T], usage: PrimitiveUsage) -> AccessorIndex {
		if matches!(usage, PrimitiveUsage::Indices) {
			assert!(T::NUM_COMPONENTS == 1, "indices must be flat!");
		}

		let data_u8 = BufferData::to_u8(data);
		let buffer_index = BufferIndex(self.buffers.len());

		self.buffers.push(Buffer {
			uri: data_u8.to_owned(),
			byte_length: data_u8.len(),
		});

		let target = match usage {
			PrimitiveUsage::Indices => 34963, // ELEMENT_ARRAY_BUFFER
			_ => 34962,                       // ARRAY_BUFFER
		};
		let view_index = BufferViewIndex(self.buffer_views.len());
		self.buffer_views.push(BufferView {
			buffer: buffer_index,
			byte_length: data_u8.len(),
			byte_offset: 0,
			target,
		});

		let (min, max) = T::to_minmax(data);

		let accessor_index = AccessorIndex(self.accessors.len());
		self.accessors.push(Accessor {
			usage,
			buffer_view: view_index,
			component_type: T::COMPONENT_TYPE,
			count: data.len(),
			element_type: T::ACCESSOR_TYPE,
			min,
			max,
		});

		accessor_index
	}

	pub fn add_mesh(&mut self, name: String) -> MeshIndex {
		let mesh = MeshIndex(self.meshes.len());
		self.meshes.push(Mesh {
			primitives: Vec::new(),
			name: name.clone(),
		});
		let node_index = NodeIndex(self.nodes.len());
		self.nodes[0].children.push(node_index);
		self.nodes.push(Node {
			mesh: Some(mesh),
			name,
			translation: None,
			children: Vec::new(),
		});

		assert_eq!(self.meshes.len() + 1, self.nodes.len());
		mesh
	}

	pub fn set_mesh_position(&mut self, mesh: MeshIndex, position: [f32; 3]) {
		self.nodes[mesh.0 + 1].translation = Some(position);
	}

	pub fn add_mesh_simple(
		&mut self, name: String, data: &[AccessorIndex], material: Option<MaterialIndex>,
	) -> MeshIndex {
		let mesh = self.add_mesh(name);
		self.add_mesh_primitive(mesh, data, material);
		mesh
	}

	pub fn add_mesh_primitive(
		&mut self, mesh: MeshIndex, data: &[AccessorIndex], material: Option<MaterialIndex>,
	) -> PrimitiveIndex {
		let mut indices = None;
		let mut positions = None;
		let mut texcoord_0 = None;
		for prim in data {
			match self.accessors[prim.0].usage {
				PrimitiveUsage::Indices => indices = Some(*prim),
				PrimitiveUsage::Positions => positions = Some(*prim),
				PrimitiveUsage::UVs => texcoord_0 = Some(*prim),
			}
		}

		let mesh = &mut self.meshes[mesh.0];
		let prim_index = PrimitiveIndex(mesh.primitives.len());
		mesh.primitives.push(Primitive {
			attributes: Attributes {
				position: positions.expect("missing positions primitives!"),
				texcoord_0,
			},
			indices: indices.expect("missing indices primitives!"),
			material,
		});
		prim_index
	}

	pub fn combine_buffers(&mut self) {
		for view in &mut self.buffer_views {
			let buffer_index = view.buffer.0;
			if buffer_index == 0 {
				continue;
			}

			let mut src: Buffer = mem::take(&mut self.buffers[buffer_index]);
			assert_eq!(view.byte_offset, 0);
			assert!(src.byte_length != 0 && src.byte_length == src.uri.len());

			let dest = &mut self.buffers[0];

			while dest.byte_length % 4 != 0 {
				dest.uri.push(0);
				dest.byte_length += 1;
			}

			view.buffer.0 = 0;
			view.byte_offset = dest.byte_length;

			dest.byte_length += src.byte_length;
			dest.uri.append(&mut src.uri);
		}
		self.buffers.truncate(1);
	}
}

fn to_uri(data: &[u8]) -> String {
	let mut result = "data:application/octet-stream;base64,".to_owned();
	use base64::{engine::general_purpose, Engine};
	general_purpose::STANDARD.encode_string(data, &mut result);
	result
}

pub trait BufferData: Sized + Copy + PartialOrd + std::fmt::Debug {
	const COMPONENT_TYPE: usize;
	const ACCESSOR_TYPE: &'static str = "SCALAR";
	const NUM_COMPONENTS: usize = 1;

	fn to_u8(arr: &[Self]) -> &[u8] {
		unsafe { std::slice::from_raw_parts(arr.as_ptr() as *const u8, std::mem::size_of_val(arr)) }
	}

	type InnerType: Copy + Into<f64>;
	fn to_array(&self) -> &[Self::InnerType];

	fn to_minmax(arr: &[Self]) -> (Vec<f64>, Vec<f64>) {
		let mut min = arr[0];
		let mut max = arr[0];

		for next in arr.iter().skip(1) {
			min = min.minmax1(next).0;
			max = max.minmax1(next).1;
		}

		let result: (Vec<f64>, Vec<f64>) = (
			min.to_array().iter().copied().map(Into::into).collect(),
			max.to_array().iter().copied().map(Into::into).collect(),
		);

		result
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		if self < rhs {
			(*self, *rhs)
		} else {
			(*rhs, *self)
		}
	}
}
impl BufferData for i8 {
	const COMPONENT_TYPE: usize = 5120;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u8 {
	const COMPONENT_TYPE: usize = 5121;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for i16 {
	const COMPONENT_TYPE: usize = 5122;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u16 {
	const COMPONENT_TYPE: usize = 5123;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u32 {
	const COMPONENT_TYPE: usize = 5125;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for f32 {
	const COMPONENT_TYPE: usize = 5126;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}

fn minmax_arr<T: Copy, const N: usize>(
	arr1: &[T; N], arr2: &[T; N], func: impl Fn(&T, &T) -> (T, T),
) -> ([T; N], [T; N]) {
	let mut mins = *arr1;
	let mut maxs = *arr1;

	for i in 0..N {
		(mins[i], maxs[i]) = func(&arr1[i], &arr2[i]);
	}

	(mins, maxs)
}

impl<T: BufferData + Into<f64>> BufferData for [T; 2] {
	const COMPONENT_TYPE: usize = T::COMPONENT_TYPE;
	const ACCESSOR_TYPE: &'static str = "VEC2";
	const NUM_COMPONENTS: usize = 2;

	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		minmax_arr(self, rhs, T::minmax1)
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 3] {
	const COMPONENT_TYPE: usize = T::COMPONENT_TYPE;
	const ACCESSOR_TYPE: &'static str = "VEC3";
	const NUM_COMPONENTS: usize = 3;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		minmax_arr(self, rhs, T::minmax1)
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 4] {
	const COMPONENT_TYPE: usize = T::COMPONENT_TYPE;
	const ACCESSOR_TYPE: &'static str = "VEC4";
	const NUM_COMPONENTS: usize = 4;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		minmax_arr(self, rhs, T::minmax1)
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 3 * 3] {
	const COMPONENT_TYPE: usize = T::COMPONENT_TYPE;
	const ACCESSOR_TYPE: &'static str = "MAT3";
	const NUM_COMPONENTS: usize = 3 * 3;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		minmax_arr(self, rhs, T::minmax1)
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 4 * 4] {
	const COMPONENT_TYPE: usize = T::COMPONENT_TYPE;
	const ACCESSOR_TYPE: &'static str = "MAT4";
	const NUM_COMPONENTS: usize = 4 * 4;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}

	fn minmax1(&self, rhs: &Self) -> (Self, Self) {
		minmax_arr(self, rhs, T::minmax1)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_minmax_arr() {
		fn test(a1: [i16; 3], a2: [i16; 3]) {
			let (min, max) = minmax_arr(&a1, &a2, BufferData::minmax1);
			println!("{a1:?},{a2:?} -> {min:?},{max:?}");
			for i in 0..3 {
				assert!(min[i] <= max[i]);

				assert!(min[i] <= a1[i]);
				assert!(min[i] <= a2[i]);
				assert!(max[i] >= a1[i]);
				assert!(max[i] >= a2[i]);

				assert!(min[i] == a1[i] || min[i] == a2[i]);
				assert!(max[i] == a1[i] || max[i] == a2[i]);
			}
		}

		for a in [-10, -5, 0, 5, 10i16] {
			for b in [-14, -7, 0, 7, 14] {
				let (min, max) = BufferData::minmax1(&a, &b);
				assert!(min <= max);
				assert!(min <= a);
				assert!(min <= b);
				assert!(max >= a);
				assert!(max >= b);
			}
		}

		for a in [-10, -5, 0, 5, 10] {
			for b in [-14, -7, 0, 7, 14] {
				for c in [-9, -3, 0, 3, 9] {
					let arr1 = [a, b, c];

					for a in [-10, -5, 0, 5, 10] {
						for b in [-14, -7, 0, 7, 14] {
							for c in [-9, -3, 0, 3, 9] {
								let arr2 = [a, b, c];

								test(arr1, arr2);
							}
						}
					}
				}
			}
		}
	}
}
