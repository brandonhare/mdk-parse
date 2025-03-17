/// An implementation of the [GLTF](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html) 3D model file format.
use serde::{Serialize, Serializer};
use std::mem;

use crate::{Vec2, Vec3};

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
	#[serde(skip_serializing_if = "Option::is_none")]
	mode: Option<PrimitiveMode>,
}
#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
struct Attributes {
	position: AccessorIndex,
	#[serde(skip_serializing_if = "Option::is_none")]
	texcoord_0: Option<AccessorIndex>,
	#[serde(skip_serializing_if = "Option::is_none")]
	color_0: Option<AccessorIndex>,
}

#[derive(Serialize, Clone, Copy, Eq, PartialEq)]
#[serde(into = "usize")]
pub enum PrimitiveMode {
	Points = 0,
	Lines = 1,
	LineLoop = 2,
	LineStrip = 3,
	Triangles = 4,
	TriangleStrip = 5,
	TriangleFan = 6,
}
impl From<PrimitiveMode> for usize {
	fn from(value: PrimitiveMode) -> Self {
		value as usize
	}
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Material {
	name: String,
	pbr_metallic_roughness: PbrMetallicRoughness,
	#[serde(skip_serializing_if = "Option::is_none")]
	alpha_mode: Option<AlphaMode>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum PbrMetallicRoughness {
	BaseColorTexture(TextureInfo),
	BaseColorFactor([f32; 4]),
	RoughnessFactor(f32),
}
#[derive(Serialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AlphaMode {
	Opaque,
	Mask,
	Blend,
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

#[derive(Serialize, Default, Clone)]
#[serde(into = "usize")]
enum FilterType {
	#[default]
	Linear = 9729,
	Nearest = 9728,
}
impl From<FilterType> for usize {
	fn from(value: FilterType) -> usize {
		value as usize
	}
}
#[derive(Serialize, Default, Clone)]
#[serde(into = "usize")]
enum WrapType {
	#[default]
	Repeat = 10497,
	Clamp = 33071,
}
impl From<WrapType> for usize {
	fn from(value: WrapType) -> usize {
		value as usize
	}
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sampler {
	mag_filter: FilterType,
	min_filter: FilterType,
	wrap_s: WrapType,
	wrap_t: WrapType,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
	buffer_view: BufferViewIndex,
	component_type: AccessorComponentType,
	#[serde(skip_serializing_if = "is_false")]
	normalized: bool,
	count: usize,
	#[serde(rename = "type")]
	element_type: AccessorType,
	#[serde(skip_serializing_if = "AccessorMinMaxValue::is_none")]
	min: AccessorMinMaxValue,
	#[serde(skip_serializing_if = "AccessorMinMaxValue::is_none")]
	max: AccessorMinMaxValue,
}
fn is_false(value: &bool) -> bool {
	!value
}

#[derive(Serialize, Clone)]
#[serde(into = "usize")]
pub enum AccessorComponentType {
	SignedByte = 5120,
	UnsignedByte = 5121,
	SignedShort = 5122,
	UnsignedShort = 5123,
	UnsignedInt = 5125,
	Float = 5126,
}
impl From<AccessorComponentType> for usize {
	fn from(value: AccessorComponentType) -> Self {
		value as usize
	}
}
#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AccessorType {
	Scalar,
	Vec2,
	Vec3,
	Vec4,
	Mat2,
	Mat3,
	Mat4,
}

#[derive(Serialize, Default, Clone, Copy)]
#[serde(untagged)]
pub enum AccessorMinMaxValue {
	#[default]
	None,
	Scalar([f64; 1]),
	Vec2([f64; 2]),
	Vec3([f64; 3]),
	Vec4([f64; 4]),
}
impl AccessorMinMaxValue {
	fn is_none(&self) -> bool {
		matches!(self, Self::None)
	}
}
impl std::ops::Deref for AccessorMinMaxValue {
	type Target = [f64];
	fn deref(&self) -> &Self::Target {
		use AccessorMinMaxValue as AV;
		match self {
			AV::None => &[],
			AV::Scalar(values) => values.as_slice(),
			AV::Vec2(values) => values.as_slice(),
			AV::Vec3(values) => values.as_slice(),
			AV::Vec4(values) => values.as_slice(),
		}
	}
}
impl std::ops::DerefMut for AccessorMinMaxValue {
	fn deref_mut(&mut self) -> &mut Self::Target {
		use AccessorMinMaxValue as AV;
		match self {
			AV::None => &mut [],
			AV::Scalar(values) => values.as_mut_slice(),
			AV::Vec2(values) => values.as_mut_slice(),
			AV::Vec3(values) => values.as_mut_slice(),
			AV::Vec4(values) => values.as_mut_slice(),
		}
	}
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
	#[serde(skip_serializing_if = "Option::is_none")]
	target: Option<usize>,
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
	translation: Option<Vec3>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	children: Vec<NodeIndex>,
	#[serde(skip_serializing_if = "serde_json::Map::is_empty")]
	extras: serde_json::Map<String, serde_json::Value>,

	#[serde(skip)]
	parent: Option<NodeIndex>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum AnimationChannelTargetPath {
	Translation,
	Rotation,
	Scale,
	Weights,
}
#[derive(Serialize)]
struct AnimationChannelTarget {
	node: NodeIndex,
	path: AnimationChannelTargetPath,
}
#[derive(Serialize)]
struct AnimationChannel {
	sampler: usize,
	target: AnimationChannelTarget,
}
#[derive(Debug, Serialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AnimationInterpolationMode {
	Linear,
	Step,
	CubicSpline,
}
#[derive(Serialize)]
struct AnimationSampler {
	input: AccessorIndex,
	output: AccessorIndex,
	#[serde(skip_serializing_if = "Option::is_none")]
	interpolation: Option<AnimationInterpolationMode>,
}
#[derive(Serialize)]
struct Animation {
	name: String,
	channels: Vec<AnimationChannel>,
	samplers: Vec<AnimationSampler>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct NodeIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct MeshIndex(usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveIndex(MeshIndex, usize);
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct MaterialIndex(usize);
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
pub struct AnimationIndex(usize);

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
	#[serde(skip_serializing_if = "Vec::is_empty")]
	samplers: Vec<Sampler>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	accessors: Vec<Accessor>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	buffers: Vec<Buffer>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	buffer_views: Vec<BufferView>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	animations: Vec<Animation>,

	#[serde(skip)]
	debug_cube: Option<MeshIndex>,
}

enum PrimitiveTarget {
	AnimationData,
	AnimationTimestamps,
	Indices = 34963,
	Vertices = 34962,
}

impl Gltf {
	pub fn new(name: String) -> Self {
		Gltf {
			scenes: [Scene {
				name: name.clone(),
				nodes: [NodeIndex(0)],
			}],
			nodes: vec![Node {
				name,
				mesh: None,
				translation: None,
				children: Vec::new(),
				parent: None,
				extras: Default::default(),
			}],
			..Default::default()
		}
	}

	pub fn get_root_node(&self) -> NodeIndex {
		NodeIndex(0)
	}

	#[must_use]
	pub fn create_colour_material(&mut self, name: String, colour: [f32; 4]) -> MaterialIndex {
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::BaseColorFactor(colour),
			alpha_mode: None,
		});
		MaterialIndex(self.materials.len() - 1)
	}

	#[must_use]
	pub fn create_translucent_material(&mut self, name: String) -> MaterialIndex {
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::BaseColorFactor([1.0; 4]),
			alpha_mode: Some(AlphaMode::Blend),
		});
		MaterialIndex(self.materials.len() - 1)
	}
	#[must_use]
	pub fn create_shiny_material(&mut self, name: String) -> MaterialIndex {
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::RoughnessFactor(0.0),
			alpha_mode: None,
		});
		MaterialIndex(self.materials.len() - 1)
	}

	#[must_use]
	pub fn create_texture_material_ref(
		&mut self, name: String, relative_filename: String, alpha_mode: Option<AlphaMode>,
	) -> MaterialIndex {
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

		if self.samplers.is_empty() {
			self.samplers.push(Default::default());
		}

		let material_index = MaterialIndex(self.materials.len());
		self.materials.push(Material {
			name,
			pbr_metallic_roughness: PbrMetallicRoughness::BaseColorTexture(TextureInfo {
				index: texture_index,
			}),
			alpha_mode: alpha_mode.filter(|mode| !matches!(mode, AlphaMode::Opaque)),
		});
		material_index
	}

	#[must_use]
	pub fn create_texture_material_embedded(
		&mut self, name: String, data: &[u8], alpha_mode: Option<AlphaMode>,
	) -> MaterialIndex {
		self.create_texture_material_ref(name, to_uri_mime(data, "image/png"), alpha_mode)
	}

	#[must_use]
	pub fn create_node(&mut self, name: String, mesh: Option<MeshIndex>) -> NodeIndex {
		let result = NodeIndex(self.nodes.len());
		self.nodes.push(Node {
			name,
			mesh,
			translation: None,
			children: Vec::new(),
			parent: None,
			extras: Default::default(),
		});
		result
	}
	pub fn create_child_node(
		&mut self, parent: NodeIndex, name: String, mesh: Option<MeshIndex>,
	) -> NodeIndex {
		let child_node = self.create_node(name, mesh);
		self.set_node_parent(parent, child_node);
		child_node
	}
	pub fn get_node_name_mut(&mut self, node: NodeIndex) -> &mut String {
		&mut self.nodes[node.0].name
	}
	pub fn set_node_parent(&mut self, parent: NodeIndex, child: NodeIndex) {
		let node = &mut self.nodes[child.0];
		if let Some(parent_index) = node.parent.replace(parent) {
			let old_parent_children = &mut self.nodes[parent_index.0].children;
			let index = old_parent_children
				.iter()
				.position(|&i| i == child)
				.expect("invalid node setup!");
			old_parent_children.remove(index);
		}
		self.nodes[parent.0].children.push(child);
	}
	pub fn set_node_mesh(&mut self, node: NodeIndex, mesh: MeshIndex) {
		self.nodes[node.0].mesh = Some(mesh);
	}
	pub fn set_node_position(&mut self, node: NodeIndex, position: Vec3) {
		self.nodes[node.0].translation = Some(position);
	}
	pub fn get_node_mesh(&self, node: NodeIndex) -> Option<MeshIndex> {
		self.nodes[node.0].mesh
	}
	pub fn set_node_extras(
		&mut self, node: NodeIndex, name: impl Into<String>, value: impl Into<serde_json::Value>,
	) {
		self.nodes[node.0].extras.insert(name.into(), value.into());
	}

	pub fn create_base_node(&mut self, name: String, mesh: Option<MeshIndex>) -> NodeIndex {
		self.create_child_node(self.get_root_node(), name, mesh)
	}

	pub fn create_mesh(&mut self, name: String) -> MeshIndex {
		let mesh = MeshIndex(self.meshes.len());
		self.meshes.push(Mesh {
			name,
			primitives: Vec::new(),
		});
		mesh
	}

	fn add_primitive_data<T: BufferData>(
		&mut self, data: &[T], target: PrimitiveTarget,
	) -> AccessorIndex {
		if matches!(target, PrimitiveTarget::Indices) {
			assert!(
				matches!(T::ACCESSOR_TYPE, AccessorType::Scalar),
				"indices must be flat!"
			);
		}

		let data_u8 = BufferData::to_u8(data);
		let buffer_index = BufferIndex(self.buffers.len());

		self.buffers.push(Buffer {
			uri: data_u8.to_owned(),
			byte_length: data_u8.len(),
		});

		let target = match target {
			PrimitiveTarget::AnimationTimestamps | PrimitiveTarget::AnimationData => None,
			PrimitiveTarget::Indices | PrimitiveTarget::Vertices => Some(target as usize),
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
			buffer_view: view_index,
			component_type: T::COMPONENT_TYPE,
			normalized: T::NORMALIZED,
			count: data.len(),
			element_type: T::ACCESSOR_TYPE,
			min,
			max,
		});

		accessor_index
	}

	pub fn add_mesh_primitive(
		&mut self, mesh: MeshIndex, positions: &[Vec3], indices: &[u16],
		material: Option<MaterialIndex>,
	) -> PrimitiveIndex {
		let position = self.add_primitive_data(positions, PrimitiveTarget::Vertices);
		let indices = self.add_primitive_data(indices, PrimitiveTarget::Indices);

		let primitives = &mut self.meshes[mesh.0].primitives;
		let primitive_index = primitives.len();
		primitives.push(Primitive {
			attributes: Attributes {
				position,
				texcoord_0: None,
				color_0: None,
			},
			indices,
			material,
			mode: None,
		});

		PrimitiveIndex(mesh, primitive_index)
	}
	pub fn set_primitive_mode(&mut self, primitive: PrimitiveIndex, mode: PrimitiveMode) {
		self.meshes[primitive.0.0].primitives[primitive.1].mode = Some(mode);
	}

	pub fn add_primitive_uvs(&mut self, primitive: PrimitiveIndex, uvs: &[Vec2]) {
		if uvs.is_empty() {
			return;
		}
		let uvs = self.add_primitive_data(uvs, PrimitiveTarget::Vertices);
		self.meshes[primitive.0.0].primitives[primitive.1]
			.attributes
			.texcoord_0 = Some(uvs);
	}
	pub fn add_primitive_colours(&mut self, primitive: PrimitiveIndex, colours: &[[u8; 4]]) {
		if colours.is_empty() {
			return;
		}
		let colours = self.add_primitive_data(colours, PrimitiveTarget::Vertices);
		self.meshes[primitive.0.0].primitives[primitive.1]
			.attributes
			.color_0 = Some(colours);
	}

	pub fn create_mesh_from_primitive(
		&mut self, name: String, positions: &[Vec3], indices: &[u16], uvs: Option<&[Vec2]>,
		material: Option<MaterialIndex>,
	) -> MeshIndex {
		let mesh = self.create_mesh(name);
		let prim = self.add_mesh_primitive(mesh, positions, indices, material);
		if let Some(uvs) = uvs {
			self.add_primitive_uvs(prim, uvs);
		}
		mesh
	}

	pub fn create_animation(&mut self, name: String) -> AnimationIndex {
		let result = AnimationIndex(self.animations.len());
		self.animations.push(Animation {
			name,
			channels: Vec::new(),
			samplers: Vec::new(),
		});
		result
	}

	pub fn create_animation_timestamps(&mut self, num_frames: usize, fps: f32) -> AccessorIndex {
		let period = fps.recip();
		self.add_animation_timestamps(
			&(0..num_frames)
				.map(|n| n as f32 * period)
				.collect::<Vec<f32>>(),
		)
	}
	pub fn add_animation_timestamps(&mut self, timestamps: &[f32]) -> AccessorIndex {
		self.add_primitive_data(timestamps, PrimitiveTarget::AnimationTimestamps)
	}

	pub fn add_animation_translation(
		&mut self, animation: AnimationIndex, node: NodeIndex, timestamps: AccessorIndex,
		path: &[Vec3], interpolation: Option<AnimationInterpolationMode>,
	) {
		let data = self.add_primitive_data(path, PrimitiveTarget::AnimationData);

		let anim = &mut self.animations[animation.0];
		let sampler_index = anim.samplers.len();
		anim.samplers.push(AnimationSampler {
			input: timestamps,
			output: data,
			interpolation,
		});
		anim.channels.push(AnimationChannel {
			sampler: sampler_index,
			target: AnimationChannelTarget {
				node,
				path: AnimationChannelTargetPath::Translation,
			},
		});
	}

	pub fn combine_buffers(&mut self) {
		// todo dont merge buffers of different types?
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

	pub fn render_json(&mut self) -> String {
		serde_json::to_string(self).unwrap()
	}

	pub fn get_cube_mesh(&mut self) -> MeshIndex {
		if let Some(result) = self.debug_cube {
			return result;
		};

		let (cube_verts, cube_indices) = make_cube(0.5);
		let cube_material = self.create_colour_material("Debug".to_owned(), [1.0, 0.0, 1.0, 1.0]);

		let result = self.create_mesh_from_primitive(
			"Cube".to_owned(),
			&cube_verts,
			&cube_indices,
			None,
			Some(cube_material),
		);
		self.debug_cube = Some(result);
		result
	}

	pub fn create_points_nodes(
		&mut self, name: String, points: &[Vec3], parent: Option<NodeIndex>,
	) -> NodeIndex {
		let cube = self.get_cube_mesh();

		let container = self.create_node(name, None);

		for (i, &point) in points.iter().enumerate() {
			let node = self.create_child_node(container, format!("{i}"), Some(cube));
			self.set_node_position(node, point);
		}

		if let Some(parent) = parent {
			self.set_node_parent(parent, container);
		}

		container
	}
}

const fn make_unit_cube() -> ([Vec3; 8], [u16; 36]) {
	let points = [
		Vec3::from_array([-0.5, -0.5, -0.5]),
		Vec3::from_array([-0.5, -0.5, 0.5]),
		Vec3::from_array([-0.5, 0.5, -0.5]),
		Vec3::from_array([-0.5, 0.5, 0.5]),
		Vec3::from_array([0.5, -0.5, -0.5]),
		Vec3::from_array([0.5, -0.5, 0.5]),
		Vec3::from_array([0.5, 0.5, -0.5]),
		Vec3::from_array([0.5, 0.5, 0.5]),
	];
	let indices = [
		0, 1, 2, 2, 1, 3, // -x
		0, 4, 1, 1, 4, 5, // -y
		0, 2, 4, 4, 2, 6, // -z
		4, 6, 5, 5, 6, 7, // +x
		2, 3, 6, 6, 3, 7, // +y
		1, 5, 3, 3, 5, 7, // +z
	];
	(points, indices)
}

fn make_cube(scale: f32) -> ([Vec3; 8], [u16; 36]) {
	const CUBE: ([Vec3; 8], [u16; 36]) = make_unit_cube();
	(CUBE.0.map(|vec| vec * scale), CUBE.1)
}

fn to_uri(data: &[u8]) -> String {
	to_uri_mime(data, "application/octet-stream")
}
fn to_uri_mime(data: &[u8], mime: &str) -> String {
	use base64::{Engine, engine::general_purpose};
	let mut result = format!("data:{mime};base64,");
	general_purpose::STANDARD.encode_string(data, &mut result);
	result
}

pub trait BufferData: Sized + Copy + PartialOrd + std::fmt::Debug {
	const COMPONENT_TYPE: AccessorComponentType;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Scalar;
	const NORMALIZED: bool = false;

	type InnerType: Copy + Into<f64>;

	fn to_u8(arr: &[Self]) -> &[u8] {
		unsafe { std::slice::from_raw_parts(arr.as_ptr() as *const u8, std::mem::size_of_val(arr)) }
	}

	fn to_array(&self) -> &[Self::InnerType];

	fn to_minmax_value(&self) -> AccessorMinMaxValue {
		fn value<T: Copy + Into<f64>, const N: usize>(arr: &[T]) -> [f64; N] {
			assert_eq!(arr.len(), N);
			std::array::from_fn(|i| arr[i].into())
		}
		let arr = self.to_array();
		match Self::ACCESSOR_TYPE {
			AccessorType::Scalar => AccessorMinMaxValue::Scalar(value(arr)),
			AccessorType::Vec2 => AccessorMinMaxValue::Vec2(value(arr)),
			AccessorType::Vec3 => AccessorMinMaxValue::Vec3(value(arr)),
			AccessorType::Vec4 => AccessorMinMaxValue::Vec4(value(arr)),
			_ => AccessorMinMaxValue::None,
		}
	}

	fn to_minmax(arr: &[Self]) -> (AccessorMinMaxValue, AccessorMinMaxValue) {
		use AccessorMinMaxValue as AV;
		let Some((first, rest)) = arr.split_first() else {
			return (AV::None, AV::None);
		};

		let first = first.to_minmax_value();
		if first.is_none() {
			return (first, first);
		}

		rest.iter()
			.fold((first, first), |(mut min, mut max), value| {
				let value = value.to_minmax_value();
				debug_assert_eq!(min.len(), value.len());
				for ((min, max), value) in min.iter_mut().zip(max.iter_mut()).zip(value.iter()) {
					if value < min {
						*min = *value;
					}
					if value > max {
						*max = *value;
					}
				}
				(min, max)
			})
	}
}
impl BufferData for i8 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::SignedByte;
	const NORMALIZED: bool = true;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u8 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::UnsignedByte;
	const NORMALIZED: bool = true;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for i16 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::SignedShort;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u16 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::UnsignedShort;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for u32 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::UnsignedInt;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}
impl BufferData for f32 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::Float;
	type InnerType = Self;
	fn to_array(&self) -> &[Self] {
		std::slice::from_ref(self)
	}
}

impl<T: BufferData + Into<f64>> BufferData for [T; 2] {
	const COMPONENT_TYPE: AccessorComponentType = T::COMPONENT_TYPE;
	const NORMALIZED: bool = T::NORMALIZED;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Vec2;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 3] {
	const COMPONENT_TYPE: AccessorComponentType = T::COMPONENT_TYPE;
	const NORMALIZED: bool = T::NORMALIZED;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Vec3;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 4] {
	const COMPONENT_TYPE: AccessorComponentType = T::COMPONENT_TYPE;
	const NORMALIZED: bool = T::NORMALIZED;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Vec4;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 3 * 3] {
	const COMPONENT_TYPE: AccessorComponentType = T::COMPONENT_TYPE;
	const NORMALIZED: bool = T::NORMALIZED;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Mat3;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}
}
impl<T: BufferData + Into<f64>> BufferData for [T; 4 * 4] {
	const COMPONENT_TYPE: AccessorComponentType = T::COMPONENT_TYPE;
	const NORMALIZED: bool = T::NORMALIZED;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Mat4;
	type InnerType = T;
	fn to_array(&self) -> &[T] {
		self
	}
}

impl BufferData for Vec3 {
	const COMPONENT_TYPE: AccessorComponentType = AccessorComponentType::Float;
	const ACCESSOR_TYPE: AccessorType = AccessorType::Vec3;
	type InnerType = f32;
	fn to_array(&self) -> &[Self::InnerType] {
		self.as_slice()
	}
}
