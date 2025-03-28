//! A mostly reverse-engineered parsing of the game's custom scripting bytecode

use std::borrow::Cow;
use std::fmt::Write;

use crate::{Reader, Vec3};

struct FlagNames<'a> {
	names: &'a [(u32, &'a str)],
	value: u32,
}
impl std::fmt::Display for FlagNames<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut first = true;
		let mut rest = self.value;
		for &(mask, name) in self.names {
			if self.value & mask == mask {
				if first {
					first = false;
				} else {
					f.write_char('|')?;
				}
				f.write_str(name)?;
				rest &= !mask;
			}
		}
		if rest != 0 {
			if !first {
				f.write_char('|')?;
			}
			f.write_fmt(format_args!("0x{rest:X}"))?;
		}
		f.write_fmt(format_args!(" (0x{:X})", self.value))
	}
}
fn flag_names<'a>(names: &'a [(u32, &'a str)], value: u32) -> FlagNames<'a> {
	FlagNames { names, value }
}

fn var_target(index: u8) -> &'static str {
	match index {
		0 => "Global",
		1 => "Arena",
		2 => "Entity",
		3 => "Direct",
		4 => "SomeDynamicThing",
		5 => "Door",
		n => format!("(Unknown {n})").leak(),
	}
}

fn tri_visflag(flag: u8) -> &'static str {
	static TRI_VISFLAGS: &[&str] = &[
		"HIDE_AND_NOCLIP",
		"SHOW_AND_COLLIDE",
		"HIDE",
		"SHOW",
		"NOCLIP",
		"COlLIDE",
	];
	TRI_VISFLAGS[flag as usize]
}

fn push_block(blocks: &mut Vec<u32>, offset: u32) -> BlockInfo {
	if offset == 0 {
		return BlockInfo { index: 0, offset };
	}
	let index = if let Some(index) = blocks.iter().position(|&o| o == offset) {
		index
	} else {
		let result = blocks.len();
		blocks.push(offset);
		result
	};
	BlockInfo { index, offset }
}
fn read_block(blocks: &mut Vec<u32>, reader: &mut Reader) -> BlockInfo {
	push_block(blocks, reader.u32())
}

fn push_ext_block<'a>(
	offsets: &mut CmiScript<'a>, target_name: &'a str, target_offset: u32, reason: &'static str,
) -> BlockInfo {
	offsets.called_scripts.push(CmiCalledScript {
		target_name,
		target_offset,
		reason,
	});
	BlockInfo {
		offset: target_offset,
		index: usize::MAX,
	}
}

fn read_ext_block<'a>(
	reader: &mut Reader<'a>, offsets: &mut CmiScript<'a>, target_name: &'a str,
	reason: &'static str,
) -> BlockInfo {
	let target = reader.u32();
	push_ext_block(offsets, target_name, target, reason)
}

#[derive(Default)]
struct BlockInfo {
	index: usize,
	offset: u32,
}
impl std::fmt::Display for BlockInfo {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.offset == 0 {
			f.write_str("(None)")
		} else if self.index != usize::MAX {
			write!(f, "block_{} ({:06X})", self.index, self.offset)
		} else {
			write!(f, "external ({:06X})", self.offset)
		}
	}
}

#[derive(Default)]
struct BranchInfo {
	code: u8,
	target1: BlockInfo,
	target2: BlockInfo,
}
impl std::fmt::Display for BranchInfo {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.code {
			0 => write!(f, "(none)"),
			0xFE => write!(
				f,
				"{{ call {} }} else {{ call {} }}",
				self.target1, self.target2
			),
			0xFC => write!(f, "{{ call {} }}", self.target1),
			0xFD => write!(f, "{{ return }}"),
			0xC => write!(f, "{{ goto {} }}", self.target1),
			code => write!(f, "{{ unknown (code: {code:2X}) }}"),
		}
	}
}
fn branch_code(blocks: &mut Vec<u32>, reader: &mut Reader) -> BranchInfo {
	let code = reader.u8();
	let mut target1 = Default::default();
	let mut target2 = Default::default();
	if code == 0xFE {
		target1 = read_block(blocks, reader);
		target2 = read_block(blocks, reader);
	} else if code == 0xFC || code == 0xC {
		target1 = read_block(blocks, reader);
	}
	BranchInfo {
		code,
		target1,
		target2,
	}
}

struct CompValue;
impl std::fmt::Display for CompValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("value")
	}
}
struct CompInfo<T = CompValue> {
	comp: u8,
	value2: f32,
	value3: f32,
	value: T,
}
impl<T: std::fmt::Display> std::fmt::Display for CompInfo<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let value = &self.value;
		let value2 = self.value2;
		let value3 = self.value3;
		match self.comp {
			1 | 3 => write!(f, "({value} < {value2})"),
			2 | 4 => write!(f, "({value2} < {value})"),
			5 => write!(f, "({value} == {value2})"),
			6 => write!(f, "({value} == {value2})"),
			7 => write!(f, "({value2} <= {value} <= {value3})"),
			8 => write!(f, "({value2} </= {value} </= {value3})"),
			n => write!(
				f,
				"(unknown: {n}, value: {value}, value2: {value2}, value3: {value3})"
			),
		}
	}
}
fn compare(reader: &mut Reader) -> CompInfo {
	let comp = reader.u8();
	let value2 = reader.f32();
	let mut value3 = 0.0;
	if comp == 7 || comp == 8 {
		value3 = reader.f32();
	}
	CompInfo {
		comp,
		value2,
		value3,
		value: CompValue,
	}
}
fn compare_with<T: std::fmt::Display>(reader: &mut Reader, value: T) -> CompInfo<T> {
	let comp = compare(reader);
	CompInfo {
		comp: comp.comp,
		value2: comp.value2,
		value3: comp.value3,
		value,
	}
}

struct VarOrData {
	target: u8,
	value: f32,
	index: u8,
}
impl std::fmt::Display for VarOrData {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.target == 3 {
			self.value.fmt(f)
		} else {
			write!(f, "{}_vars[{}]", var_target(self.target), self.index)
		}
	}
}
fn var_or_data(reader: &mut Reader) -> VarOrData {
	let target = reader.u8();
	let mut value = 0.0;
	let mut index = 0;
	if target == 3 {
		value = reader.f32();
	} else {
		index = reader.u8();
	}
	VarOrData {
		target,
		value,
		index,
	}
}
fn simple_var(reader: &mut Reader) -> VarOrData {
	let target = reader.u8();
	let index = reader.u8();
	VarOrData {
		target,
		value: 0.0,
		index,
	}
}
struct FlagVar {
	target: u8,
	index: u8,
}
impl std::fmt::Display for FlagVar {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let index = self.index & 31;

		let value = 1u32 << index;
		if self.target == 5 {
			write!(
				f,
				"{}_flags[{}]",
				var_target(self.target),
				flag_names(DOOR_FLAG_NAMES, value)
			)?;
		} else {
			write!(f, "{}_flags[0x{:X}]", var_target(self.target), value)?;
		}

		if self.index != index {
			write!(f, " (index clipped: {})", self.index)
		} else {
			Ok(())
		}
	}
}
fn flag_var(reader: &mut Reader) -> FlagVar {
	let target = reader.u8();
	let index = reader.u8();
	//assert_eq!(index & !31, 0, "flag value out of range");
	FlagVar { target, index }
}

static DOOR_FLAG_NAMES: &[(u32, &str)] = &[
	(0x1, "OPEN"),
	(0x2, "OPENING"),
	(0x4, "CLOSING"),
	(0x8, "CLOSED"),
	(0x10, "HIDE_WHEN_OPEN"),
	(0x20, "STAY_OPEN"),
	(0x40, "LOCKED"),
	(0x80, "JUST_NUKED"),
	(0x100, "HIDE_LOCK"),
];

fn get_anim_name<'a>(reader: &Reader<'a>, anim_offset: u32) -> Option<&'a str> {
	let mut anim_reader = reader.clone_at(anim_offset as usize);
	if anim_reader.u32() == 0 {
		anim_reader.try_str(8) // anim data
	} else {
		None
	}
}

#[derive(Default)]
pub struct CmiScript<'a> {
	pub summary: String,

	pub anim_names: Vec<&'a str>,
	pub anim_offsets: Vec<u32>,
	pub path_offsets: Vec<u32>,

	pub called_scripts: Vec<CmiCalledScript<'a>>,
	pub call_origins: Vec<CmiCallOrigin<'a>>, // used by caller cmi
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CmiCalledScript<'a> {
	pub target_offset: u32,
	pub target_name: &'a str,
	pub reason: &'static str,
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CmiCallOrigin<'a> {
	pub arena_name: &'a str,
	pub source_name: &'a str,
	pub target_name: &'a str,
	pub reason: Cow<'a, str>,
	pub source_offset: u32,
}

impl<'a> CmiScript<'a> {
	pub fn parse(mut reader: Reader<'a>) -> Self {
		parse_cmi(&mut reader)
	}
}

fn parse_cmi<'a>(reader: &mut Reader<'a>) -> CmiScript<'a> {
	let mut result = CmiScript::default();

	if reader.position() == 0 {
		return result;
	}

	let mut summary = String::new();
	let offsets = &mut result;

	macro_rules! w {
		()=>{};
		($arg:expr $(,$rest:expr)* $(,)?) => {
			write!(summary, $arg $(,$rest)*).unwrap()
		};
	}
	macro_rules! wl {
		()=>{summary.push('\n');};
		($arg:expr $(,$rest:expr)* $(,)?) => {
			writeln!(summary, $arg $(,$rest)*).unwrap()
		};
	}

	let mut blocks = vec![reader.position() as u32];
	let mut block_index = 0;

	while block_index < blocks.len() {
		let block_offset = blocks[block_index];

		if block_index == 0 {
			wl!("main (offset {block_offset:06X})");
		} else {
			wl!("block_{block_index} (offset {block_offset:06X})");
		}

		reader.set_position(block_offset as usize);
		loop {
			let cmd_offset = reader.position();
			let cmd = reader.u8();
			if cmd == 0xFF {
				break;
			}
			w!("[{cmd_offset:06X}: {cmd:02X} ");

			match cmd {
				0x0 | 0x7 | 0x1E | 0xFE | 0xFF => {
					eprintln!("invalid opcode {cmd:02X} at {cmd_offset:06X}!");
					wl!("Invalid!]");
					break;
				}
				0x01 => {
					wl!("Set script resume point]");
				}
				0x02 => {
					let path_offset = reader.u32();
					offsets.path_offsets.push(path_offset);
					let value1 = reader.u8();
					let value2 = reader.u8();
					let value3 = reader.u16();
					let vec = match reader.u8() {
						0 => Some(reader.vec3()),
						1 => None,
						n => {
							eprint!("cmi opcode 0x02 unknown vec param {n} at {cmd_offset:06X}");
							None
						}
					};
					// todo what are all these
					wl!(
						"Set path] v1: {value1}, v2: {value2}, v3: {value3}, vec: {vec:?}, path offset: {path_offset:06X}"
					);
				}
				0x03 => {
					let anim_offset = reader.u32();
					if let Some(anim_name) = get_anim_name(reader, anim_offset) {
						offsets.anim_names.push(anim_name);
						wl!("Set animation] name: {anim_name}");
					} else {
						offsets.anim_offsets.push(anim_offset);
						wl!("Set animation] anim offset: {anim_offset:06X}");
					}
				}
				0x04 => {
					let order_code = reader.u8();
					w!("Give order] ");
					let mut target_script = None;
					if order_code == 7 {
						let code = reader.u8();
						assert!(code == 0xFC || code == 0xC);
						let target = reader.u32();
						target_script = Some(target);
						w!("Run script ({target:06X})");
					} else if order_code == 0x2b {
						let dir = reader.vec2();
						w!("Set home (dir: {dir:?})");
					} else if order_code == 1 {
						w!("Set some home thing");
					} else {
						w!("Unknown! (code: {order_code})");
					}

					w!(", Target: ");

					let order_target = reader.u8();
					if order_target == 6 || order_target == 10 {
						let value = reader.f32();
						if order_target == 6 {
							w!("Visible (distance: {value})");
						} else {
							w!("Height (min y: {value})");
						}
					} else if order_target == 3 {
						w!("Everyone");
					}

					let name = match order_target {
						2 | 4 | 5 | 6 | 7 | 10 => Some(reader.pascal_str()),
						_ => None,
					};

					match order_target {
						2 => w!("Normal"),
						3 => (), // Everyone
						4 => w!("Single"),
						5 => {
							let value = reader.u32();
							w!("ID={value}");
						}
						6 => (), // Visible
						7 => w!("Children"),
						9 => w!("Buddy"),
						10 => (), // Height
						n => w!("Unknown (target: {n})"),
					}

					if let Some(name) = name {
						w!(", Name: {name}");

						if let Some(target_script) = target_script {
							push_ext_block(offsets, name, target_script, "Order");
						}
					} else if let Some(target_script) = target_script {
						push_ext_block(offsets, "Unknown", target_script, "Order");
					}

					wl!();
				}
				0x05 => {
					let value = reader.f32();
					wl!("Set camera zoom?] value: {value}");
				}
				0x06 => {
					wl!("Set someCmiField to 6]");
				}
				0x08 => {
					let angle = reader.i16().rem_euclid(360);
					wl!("Set yaw] angle: {angle}");
				}
				0x09 => {
					wl!("Clear function stack]");
				}
				0x0A => {
					let name = reader.pascal_str();
					let index = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Branch if alien with name at index] name: {name}, index: {index}, {branch}"
					);
				}
				0x0B => {
					let value = reader.u8();
					wl!("Set min order range] {value}");
				}
				0x0C => {
					let count = reader.u8();
					w!("Random jump] targets: [");
					for i in 0..count {
						let block = read_block(&mut blocks, reader);
						if i != 0 {
							w!(", ");
						}
						w!("{}", block);
					}
					wl!("]");
				}
				0x0D => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some global3 field] {branch}");
				}
				0x0E => {
					let distance = reader.u16();
					let angle = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on visible] distance: {distance}, angle: {angle}, {branch}");
				}
				0x0F => {
					wl!("Set some cmi field]");
				}
				0x10 => {
					let value = reader.u16();
					if value == 0 {
						wl!("Destroy entity]");
					} else {
						wl!(
							"Set entity health] value: {value}, (some flag set: {})",
							64999 < value
						);
					}
				}
				0x11 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some anim field] {branch}");
				}
				0x12 => {
					let value = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch with value?] value: {value}, {branch}");
				}
				0x13 => {
					wl!("Clear someAnimField3]");
				}
				0x14 => {
					wl!("Clear somePath]");
				}
				0x15 => {
					let index = reader.i32();
					wl!("Set someIndex] value: {index}");
				}
				0x16 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on has parts] {branch}");
				}
				0x17 => {
					let set = reader.u8() != 0;
					wl!("Set flags[1] and some data] set: {set}");
				}
				0x18 => {
					let value = reader.u8();
					let name = reader.pascal_str();
					wl!("Set someName4] value: {value}, name: {name}");
				}
				0x19 => {
					let name = reader.pascal_str();
					wl!("Set some name] name: {name}");
				}
				0x1A => {
					let name = reader.pascal_str();
					wl!("Set someName3] name: {name}");
				}
				0x1B => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some global var] {branch}");
				}
				0x1C => {
					let offset = reader.u32();
					offsets.path_offsets.push(offset);
					wl!("Mortar path] path offset: {offset:06X}");
				}
				0x1D => {
					let value1 = reader.u8();
					let name = reader.pascal_str();
					let target = read_ext_block(reader, offsets, name, "Create Chain");
					wl!("CreateChain] value1: {value1}, name: {name}, target: {target}");
				}
				0x1F => {
					let count = reader.u8();
					w!("Hide parts] names: [");
					for i in 0..count {
						let part_name = reader.pascal_str();
						if i != 0 {
							w!(", ");
						}
						w!("{part_name}");
					}
					wl!("]");
				}
				0x20 => {
					let count = reader.u8();
					w!("Show parts] names: [");
					for i in 0..count {
						let name = reader.pascal_str();
						if i != 0 { w!(", {name}") } else { w!("{name}") }
					}
					wl!("]");
				}
				0x21 => {
					let value = reader.u32() - 1;
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on somePath value] value: {value}, {branch}");
				}
				0x22 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on has someAlien] {branch}");
				}
				0x23 => {
					let on = reader.u8();
					wl!("Set entity flag 4] set: {on}");
				}
				0x24 => {
					let on = reader.u8();
					wl!("Set entity flag 2] set: {on}");
				}
				0x25 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on something] {branch}");
				}
				0x26 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on vertical velocity] if {comp} {branch}");
				}
				0x27 => {
					let var_data = var_or_data(reader);
					wl!("Anim some facing value] {var_data}");
				}
				0x28 => {
					let var_data = var_or_data(reader);
					wl!("Anim facing yaw value] {var_data}");
				}
				0x29 => {
					let index = reader.u8();
					wl!("Some sniper thing] index: {index}");
				}
				0x2A => {
					let mut name = reader.pascal_str();
					if name.is_empty() {
						name = reader.pascal_str()
					};
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch if part exists] name: {name}, {branch}");
				}
				0x2B => {
					let value = reader.vec2();
					wl!("Move home] value: {value:?}");
				}
				0x2C => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on somCmiField] {branch}");
				}
				0x2D => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to player] if {comp} {branch}");
				}
				0x2E => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on hiding spot] {branch}");
				}
				0x2F => {
					let value = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Weighted random call (direct)] weight: {value}, {branch}");
				}
				0x30 => {
					let value = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Weighted random call (framerate adjusted)] weight: {value}, {branch}");
				}
				0x31 => {
					let count = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some alien data] count: {count}, {branch}");
				}
				0x32..=0x35 => {
					let index = (cmd - 0x31) % 4;
					let var_data = var_or_data(reader);
					wl!("Set entity someCmiDataValue] values[{index}] = {var_data}");
				}
				0x36 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to something] if {comp} {branch}");
				}
				0x37 => {
					let var_data = var_or_data(reader);
					wl!("Set entity someCmiDataValue] values[5] = {var_data}");
				}
				0x38 => {
					let value = reader.i16();
					wl!("Add someCmiField10] delta: {value}");
				}
				0x39 => {
					let distance = reader.u16();
					let angle = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch if visible] distance: {distance}, angle: {angle}, {branch}");
				}
				0x3A => {
					let var_data = var_or_data(reader);
					wl!("Set anim framerate] framerate: {var_data}");
				}
				0x3B => {
					let anim_offset = reader.u32();
					if let Some(anim_name) = get_anim_name(reader, anim_offset) {
						offsets.anim_names.push(anim_name);
						wl!("Set anim] name: {anim_name}");
					} else {
						offsets.anim_offsets.push(anim_offset);
						wl!("Set anim] anim offset: {anim_offset:06X}");
					}
				}
				0x3C => {
					wl!("Face player 2]");
				}
				0x3D => {
					let has_name = reader.u8();
					let mut name1 = "";
					let mut point_index = 0;
					if has_name == 0 {
						point_index = reader.u8();
					} else {
						name1 = reader.pascal_str();
					}
					let name2 = reader.pascal_str();
					let target = read_ext_block(reader, offsets, name2, "Spawn (3D)");
					if has_name == 0 {
						wl!(
							"Spawn badguy] point index: {point_index}, name: {name2}, target: {target}"
						);
					} else {
						wl!("Spawn badguy] target name: {name1}, name: {name2}, target: {target}");
					}
				}
				0x3E => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on angle to player] if {comp} {branch}");
				}
				0x3F => {
					let set = reader.u8() == 0;
					wl!("{} flag 0x10]", if set { "Set" } else { "Clear" });
				}
				0x40 => {
					let var_data = var_or_data(reader);
					wl!("Delay] {var_data}");
				}
				0x41 => {
					let var = simple_var(reader);
					let value = reader.f32();
					wl!("Set Variable] {var} = {value}");
				}
				0x42 => {
					let var = simple_var(reader);
					let value = reader.f32();
					wl!("Add to variable] {var} += {value}");
				}
				0x43 => {
					let var = simple_var(reader);
					let comp = compare_with(reader, var);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on variable compare] if {comp} {branch}");
				}
				0x44 => {
					let flag = flag_var(reader);
					wl!("Set flag var] {flag} = true");
				}
				0x45 => {
					let flag = flag_var(reader);
					wl!("Clear flag var] {flag} = false");
				}
				0x46 => {
					let flag = flag_var(reader);
					wl!("Toggle flag var] {flag} = (toggle)");
				}
				0x47 | 0x48 => {
					let flag = flag_var(reader);
					let branch = branch_code(&mut blocks, reader);
					let condition = if cmd == 0x47 { "== true" } else { "== false" };
					wl!("Branch on flag var] if {flag} {condition} {branch}");
				}
				0x49 => {
					let value = reader.u8();
					wl!("Set max order range] value: {value}");
				}
				0x4A => {
					let value1 = reader.u8();
					let value2 = reader.u8();
					let name = reader.pascal_str();
					wl!("Set some alien] value1: {value1}, value2: {value2}, name: {name}");
				}
				0x4B => {
					wl!("Clear someCmiFIeld]");
				}
				0x4C => {
					let target = read_block(&mut blocks, reader);
					wl!("Set on killed function] target: {target}");
				}
				0x4D => {
					let silent = reader.u8() != 0;
					let msg = reader.pascal_str();
					wl!("Assert] message: \"{msg}\", (silent: {silent})");
				}
				0x4E => {
					let home = reader.vec3();
					wl!("Set home] {home:?}");
				}
				0x4F => {
					let pos = reader.vec3();
					wl!("Set position] pos: {pos:?}");
				}
				0x50 => {
					let dir = reader.vec3();
					wl!("Add velocity in facing dir] dir: {dir:?}");
				}
				0x51 => {
					let scale_dt = reader.u8() == 1;
					let speed = var_or_data(reader);
					wl!("Move in facing dir?] speed: {speed}, use dt: {scale_dt}");
				}
				0x52 => {
					let var_data = var_or_data(reader);
					wl!("Set somedata2] {var_data}");
				}
				0x53 => {
					let pos = reader.position();
					let code = reader.u8();
					if code != 0xFF {
						reader.set_position(pos);
						let var_data = var_or_data(reader);
						wl!("Set maybeRadius] {var_data}");
					} else {
						let target = reader.f32();
						let speed = reader.f32();
						wl!("Scale maybeRadius] target: {target}, speed: {speed}");
					}
				}
				0x54 => {
					let value = var_or_data(reader);
					wl!("Set someCmiField11] {value}");
				}
				0x55 => {
					let set = reader.u8() != 0;
					wl!("Set some data flag7] set: {set}");
				}
				0x56 => {
					let pos = reader.vec3();
					let name = reader.pascal_str();
					let target = read_ext_block(reader, offsets, name, "Spawn (56)");
					wl!("Spawn entity 3] name: {name}, pos: {pos:?}, init: {target}");
				}
				0x57 => {
					let min_dist = reader.u16();
					let max_dist = reader.u16();
					let angle = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Branch if visible] min dist: {min_dist}, max dist: {max_dist}, angle: {angle}, {branch}"
					);
				}
				0x58 => {
					let value = reader.u8();
					wl!("Set some cmi fields] value: {value}");
				}
				0x59 => {
					let sound_type = reader.u8();
					let mut point1 = None;
					let mut point2 = None;
					let mut point1_index = None;
					let mut point2_index = None;
					if sound_type & 0x10 == 0 {
						if sound_type & 0x20 == 0 {
							if sound_type & 0x40 == 0 {
							} else {
								point1 = Some(reader.vec3());
								point2 = point1;
							}
						} else {
							point1_index = Some(reader.u8());
							point2_index = point1_index;
						}
					} else {
						point1 = Some(reader.vec3());
					}

					let sound_name = reader.pascal_str();

					w!("Play? Sound] name: {sound_name}, type: {sound_type:X}");
					let mut print_sound = |prefix, data, index| {
						w!("{}", prefix);
						if let Some(data) = data {
							w!("data ({data:?})")
						} else if let Some(index) = index {
							w!("index ({index})")
						} else {
							w!("alien position")
						}
					};
					print_sound(", p1: ", point1, point1_index);
					print_sound(", p2: ", point2, point2_index);
					wl!("");
				}
				0x5A => {
					let name = reader.pascal_str();
					let value = reader.f32();
					wl!("Nothing?] name: {name}, value: {value}");
				}
				0x5B => {
					let var_data = var_or_data(reader);
					wl!("Set entity someCmiField4] {var_data}");
				}

				0x5C => {
					let value = reader.u16();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on anim field] value: {value}, {branch}");
				}
				0x5D => {
					let speed = reader.f32();
					let pos = reader.vec3();
					wl!("Move towards target] speed: {speed}, target: {pos:?}");
				}
				0x5E | 0x5F => {
					let count = reader.u8();
					if cmd == 0x5E {
						w!("Weighted random jump] targets: [");
					} else {
						w!("Weighted random call] targets: [");
					}
					for i in 0..count {
						let weight = reader.u8();
						let target = read_block(&mut blocks, reader);
						if i != 0 {
							w!(", ");
						}
						w!("(weight: {weight}, target: {target})");
					}
					wl!("]");
				}
				0x60 => {
					let min_pos = reader.vec2();
					let max_pos = reader.vec2();
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Branch on player in square] min XY: {min_pos:?}, max XY: {max_pos:?}, {branch}"
					);
				}
				0x61 => {
					let on = reader.u8();
					wl!("Set entity flag 80] set: {on}");
				}
				0x62 => {
					let triangle_id = reader.u8();
					let visflag = reader.u8();
					wl!(
						"Set Triangle Visibility] id: {triangle_id}, visflag: {}",
						tri_visflag(visflag)
					);
				}
				0x63 => {
					let trigger_index = (reader.i8() - 1) % 16;
					let id = reader.u8();
					let target = read_block(&mut blocks, reader);
					wl!(
						"Set triangle damage trigger] trigger index: {trigger_index}, id: {id}, target: {target}"
					);
				}
				0x64 => {
					let name = reader.pascal_str();
					wl!("Show arena] name: {name}");
				}
				0x65 => {
					wl!("Face player]");
				}
				0x66 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Call if path exists] {branch}");
				}
				0x67 => {
					let aabb_max = reader.vec3();
					let aabb_min = reader.vec3();
					let branch = branch_code(&mut blocks, reader);
					wl!("Trigger? (aabb)] min: {aabb_min:?}, max: {aabb_max:?}, {branch}");
				}
				0x68 => {
					let value = reader.f32();
					wl!("Look at target] random weight: {value}");
				}
				0x69 => {
					let values: [f32; 5] = reader.get();
					wl!("Turn to face stuff] values: {values:?}");
				}
				0x6A => {
					let value = reader.f32();
					wl!("Set entity arena2OrFloatValue] value: {value}");
				}
				0x6B => {
					let name = reader.pascal_str();
					wl!("Start sound] sound: {name}");
				}
				0x6C => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on hit bbox] {branch}");
				}
				0x6D => {
					let value = reader.u8();
					wl!("Hurt entity] value: {value}");
				}
				0x6E => {
					wl!("Destroy entity quiet]");
				}
				0x6F => {
					let index = reader.i32();
					wl!("Set entity ID] ID: {index}");
				}
				0x70 => {
					let name = reader.pascal_str();
					let pos = reader.vec3();
					let angle = reader.f32();
					if name.is_empty() {
						wl!("Teleport] pos: {pos:?}, angle: {angle}");
					} else {
						wl!("Teleport] arena: \"{name}\", pos: {pos:?}, angle: {angle}");
					}
				}
				0x71 => {
					let pos = reader.vec3();
					let name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, name, "Spawn (71)");
					wl!("Spawn alien] pos: {pos:?}, name: {name}, init target: {init_target}");
				}
				0x72 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someAlien] {branch}");
				}
				0x73 => {
					let angle = reader.f32();
					let distance = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on wall proximity] angle: {angle}, distance: {distance}, {branch}");
				}
				0x74 => {
					let flags = reader.u32();
					wl!("Set flags] flags: {flags:X}");
				}
				0x75 => {
					let flags = reader.u32();
					wl!("Clear flags] flags: {flags:X}");
				}
				0x76 => {
					let value1 = reader.u16();
					let value2 = reader.u16();
					wl!("Set some anim fields] value1: {value1}, value2: {value2}");
				}
				0x77 => {
					let name = reader.pascal_str();
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Find entity and branch on comparison] name: {name}, if {comp} {branch}");
				}
				0x78 => {
					let angle = reader.f32();
					wl!("Set pitch angle] angle: {angle}");
				}
				0x79 => {
					let distance = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance from floor] distance: {distance}, {branch}");
				}
				0x7A => {
					let speed = reader.f32();
					let angle = reader.f32();
					wl!("Set pitch angle] speed: {speed}, angle: {angle}");
				}
				0x7B => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch (arena)?] {branch}");
				}
				0x7C => {
					let value = reader.f32();
					wl!("Set someAngle] value: {value}");
				}
				0x7D => {
					wl!("Clear function stack]");
				}
				0x7E => {
					wl!("Look at player (pitch angle only)]");
				}
				0x7F => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someCmiField_10] if {comp} {branch}");
				}
				0x80 => {
					let name = reader.pascal_str();
					let value1 = reader.u8();
					let value2 = reader.u8();
					wl!("Set thing] name: {name}, value1: {value1}, value2: {value2}");
				}
				0x81 => {
					let kind = reader.u8();
					let count = reader.u8();
					w!("Blow off parts] kind: {kind}, parts: [");
					for i in 0..count {
						let name = reader.pascal_str();
						if i != 0 {
							w!(", {name}");
						} else {
							w!("{name}")
						}
					}
					wl!("]");
				}
				0x82 => {
					wl!("Create dent]");
				}
				0x83 => {
					let code = reader.u8();
					if 0x32 < code {
						wl!("Run muse5 command] code: {code}");
					} else {
						wl!("Run muse5 command] code: {code} (clear currentCmiArena)");
					}
				}
				0x84 => {
					let value1 = reader.u8();
					let point_index = reader.u8();
					let pos = if point_index == 0xFF {
						reader.vec3()
					} else {
						Vec3::default()
					};
					if value1 < 150 {
						if point_index == 0xFF {
							wl!("Create bubble] chance: {value1}%, pos: {pos:?}");
						} else {
							wl!(
								"Create bubble] chance: {value1}%, pos: somePoints[{point_index:?}]"
							);
						}
					} else {
						wl!("Setup new chunk] change: {}%", value1 - 150);
					}
				}
				0x85 => {
					let name = reader.pascal_str();
					let code = reader.u8();
					let value = reader.f32();
					wl!("Do something with material] name: {name}, code: {code}, value: {value}");
				}
				0x86 => {
					let var_data = var_or_data(reader);
					wl!("Add angle1] {var_data}");
				}
				0x87 => {
					let value = reader.f32();
					wl!("Screenshake] amount: {value}");
				}
				0x88 => {
					let count = reader.u8();
					let velocity = reader.vec3();
					let radius = reader.f32();
					let add_position = reader.u8() == 0;
					let position = reader.vec3();
					let min_u = reader.f32();
					wl!(
						"Create slimes] count: {count}, velocity: {velocity:?}, radius: {radius}, position: {position:?}, center at entity: {add_position}, u: {min_u}"
					);
				}
				0x89 => {
					let tri_id = reader.u8();
					let vec = reader.vec3();
					wl!("Shatter triangle 1] tri id: {tri_id}, vec: {vec:?}");
				}
				0x8A => {
					let tri_id = reader.u8();
					let vec1 = reader.vec3();
					let vec2 = reader.vec3();
					let vec3 = reader.vec3();
					wl!(
						"Shatter triangle 2] tri id: {tri_id}, vec: {vec1:?}, hitPoint1: {vec2:?}, hitPoint2: {vec3:?}"
					);
				}
				0x8B => {
					let tri_id = reader.u8();
					let v1 = reader.vec3();
					let v2 = reader.vec3();
					wl!("Shatter triangle 3] tri id: {tri_id}, v1: {v1:?}, v2: {v2:?}");
				}
				0x8C => {
					let tri_id = reader.u8();
					let colour = reader.i16();
					wl!("Set tri colour] tri id: {tri_id}, material: {colour}");
				}
				0x8D => {
					let index = reader.u8();
					let colour: [u8; 4] = reader.get();
					let time = reader.f32();
					wl!("Transparency fade] index: {index}, colour: {colour:?}, time: {time}");
				}
				0x8E => {
					let id = reader.u8();
					let name = reader.pascal_str();
					let a = reader.u8();
					let b = reader.u8();
					let speed = reader.f32();
					wl!("Activate fan] id: {id}, name: {name}, a: {a}, b: {b}, speed: {speed}");
				}
				0x8F => {
					let name = reader.pascal_str();
					wl!("Deactivate fan] name: {name}");
				}
				0x90 => {
					let name = reader.pascal_str();
					let min = reader.vec3();
					let max = reader.vec3();
					let value1 = reader.u8();
					let value2 = reader.u8();
					let speed = reader.f32();
					wl!(
						"Create fan] name: {name}, bbox: {min:?}-{max:?}, value1: {value1}, value2: {value2}, speed: {speed}"
					);
				}
				0x91 => {
					let name = reader.pascal_str();
					let speed = reader.f32();
					let delta = reader.f32();
					wl!("Set fan speed] name: {name}, speed: {speed}, delta: {delta}");
				}
				0x92 => {
					let id = reader.u8();
					let name = reader.pascal_str();
					let speed = reader.f32();
					let size = reader.vec3();
					let scale = reader.vec2();
					wl!(
						"Activate conveyor] id: {id}, name: {name}, speed: {speed}, size: {size:?}, scale: {scale:?}"
					);
				}
				0x93 => {
					let name = reader.pascal_str();
					wl!("Deactivate conveyor] name: {name}");
				}
				0x94 => {
					let name = reader.pascal_str();
					let speed = reader.f32();
					let delta = reader.f32();
					wl!("Set conveyor speed] name: {name}, speed: {speed}, delta: {delta}");
				}
				0x95 => {
					// spawn door
					let position = reader.vec3();
					let angle = reader.f32();
					let id = reader.i32();
					let object_name = reader.pascal_str();
					let arena_name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, object_name, "Spawn Door");
					wl!(
						"Spawn Door] pos: {position:?}, angle: {angle}, id: {id}, name: {object_name}, arena: {arena_name}, init target: {init_target}"
					);
				}
				0x96 => {
					let open_anim_offset = reader.u32();
					let close_anim_offset = reader.u32();

					w!("Set door anims] open: ");
					if let Some(open_name) = get_anim_name(reader, open_anim_offset) {
						offsets.anim_names.push(open_name);
						w!("{open_name}, close: ");
					} else {
						offsets.anim_offsets.push(open_anim_offset);
						w!("{open_anim_offset:06X}, close: ");
					}
					if let Some(close_name) = get_anim_name(reader, close_anim_offset) {
						offsets.anim_names.push(close_name);
						wl!("{close_name}");
					} else {
						offsets.anim_offsets.push(close_anim_offset);
						wl!("{close_anim_offset:06X}");
					}
				}
				0x97 => {
					let open_sound = reader.pascal_str();
					let close_sound = reader.pascal_str();
					let open_finish_sound = reader.pascal_str();
					let close_finish_sound = reader.pascal_str();
					wl!(
						"Set door sounds] open: \"{open_sound}\", close: \"{close_sound}\", open finish: \"{open_finish_sound}\", close finish: \"{close_finish_sound}\""
					);
				}
				0x98 => {
					let flags = reader.u32();
					wl!(
						"Set door flags] flags: {}",
						flag_names(DOOR_FLAG_NAMES, flags)
					);
				}
				0x99 => {
					let value = reader.f32();
					wl!("Set door open distance] distance: {value}");
				}
				0x9A => {
					let value = reader.i16();
					wl!("Wait for anim progress] value: {value}");
				}
				0x9B => {
					let value = var_or_data(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some stack value] value: {value}, {branch}");
				}
				0x9C => {
					let index = reader.u8();
					let name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, name, "Spawn (9C)");
					wl!(
						"Spawn alien] name: {name}, position: somePoints[{index}], init target: {init_target}"
					);
				}
				0x9D => {
					let name = reader.pascal_str();
					let arena_index = reader.u32();
					let speed = reader.f32();
					wl!(
						"Move to data thing] name: {name}, arena index: {arena_index}, speed: {speed}"
					);
				}
				0x9E => {
					let value1 = reader.u8();
					let damage = reader.u16();
					let value3 = reader.u8();
					let has_target = value3 & 2 != 0;
					let [v4, v5, v6]: [u8; 3] = reader.get();
					let value3 = (value3 as usize)
						| ((v4 as usize) << 8)
						| ((v5 as usize) << 0x10)
						| ((v6 as usize) << 0x18);
					if has_target {
						let target = reader.u32();
						let target = push_block(&mut blocks, target);
						wl!(
							"Check touch damage] value1: {value1}, damage: {damage}, value3: {value3}, target: {target}"
						);
					} else {
						wl!(
							"Check touch damage] value1: {value1}, damage: {damage}, value3: {value3}"
						);
					}
				}
				0x9F => {
					let value1 = reader.u8();
					let pos1 = match value1 {
						0 => Vec3::default(),
						1 | 2 => reader.vec3(),
						n => {
							println!("invalid 0x9f opcode {n}");
							Vec3::default()
						}
					};
					let pos2 = reader.vec3();
					let name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, name, "Spawn Blit");
					wl!(
						"Spawn blit alien] name: {name}, position type: {value1}, pos1: {pos1:?}, pos2: {pos2:?}, init target: {init_target}"
					);
				}
				0xA0 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on yaw] if {comp} {branch}");
				}
				0xA1 => {
					let position = reader.vec3();
					let object_name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, object_name, "Spawn Powerup");
					wl!(
						"Spawn Powerup] name: {object_name}, pos: {position:?}, init target: {init_target}"
					);
				}
				0xA2 => {
					let thing_index = (reader.u8() - 1) % 16;
					let value = reader.i16();
					wl!("Write arena thing index] thing index: {thing_index}, value: {value}");
				}
				0xA3 => {
					let thing_index = (reader.u8() - 1) % 16;
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Branch arena thing index comparison] thing index: {thing_index}, if {comp} {branch}"
					);
				}
				0xA4 => {
					let code = reader.u8();
					if code == 0 {
						wl!("Clear entity flag 0x80]");
					} else {
						let nums = reader.vec4();
						wl!("Set some entity data fields] code: {code}, nums: {nums:?}");
					}
				}
				0xA5 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on has target pos] {branch}");
				}
				0xA6 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on can see some target] {branch}");
				}
				0xA7 => {
					let distance = reader.f32();
					wl!("Move towards target] distance: {distance}");
				}
				0xA8 => {
					let triangle_id = reader.u8();
					let num = reader.u8();
					wl!("Set triangle vis? 2] id: {triangle_id}, num: {num}");
				}
				0xA9 => {
					let data = var_or_data(reader);
					wl!("Set someCmiData3] value = {data}");
				}
				0xAA => {
					let speed = reader.f32();
					wl!("Move towards player] speed: {speed}");
				}
				0xAB => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someAlien2] {branch}");
				}
				0xAC => {
					let kind = reader.u8();
					if kind == 3 {
						let index = reader.u8();
						let value = reader.f32();
						wl!("Explosion] point index: {index}, value: {value}");
					} else {
						let pos = reader.vec3();
						let value = reader.f32();
						wl!("Explosion] position: {pos:?}, kind: {kind}, value: {value}");
					}
				}
				0xAD => {
					let name = reader.pascal_str();
					if !name.is_empty() {
						wl!("Set currentCmiArena teleport] name: {name}");
					} else {
						let name = reader.pascal_str();
						let delta = reader.vec3();
						let angle = reader.f32();
						wl!("Teleport delta] name: {name}, delta: {delta:?}, delta angle: {angle}");
					}
				}
				0xAE => {
					let pickup_index = reader.u8();
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Some pickup comparison branch 1?] pickup index: {pickup_index}, if {comp} {branch}"
					);
				}
				0xAF => {
					let pickup_type = reader.u8();
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Some pickup comparison branch 2?] pickup type: {pickup_type}, {comp}, {branch}"
					);
				}
				0xB0 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on flags 0x40000] {branch}");
				}
				0xB1 => {
					let value = var_or_data(reader);
					wl!("Set some damage radius] value = {value}");
				}
				0xB2 => {
					let value1 = reader.u8();
					let pos = if value1 == 3 {
						let _ = reader.u8();
						Vec3::default()
					} else {
						reader.vec3()
					};
					let radius = reader.f32();
					let value2 = reader.f32();
					let value3 = reader.f32();
					let value4 = reader.u8();
					wl!(
						"Explosion] pos: {pos:?}, radius: {radius}, value1: {value1}, value2: {value2}, value3: {value3}, value4: {value4}"
					);
				}
				0xB3 => {
					let name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, name, "Spawn (B3)");
					wl!("Spawn alien] name: {name}, init target: {init_target}");
				}
				0xB4 => {
					let has_delta = reader.u8() != 0;
					if has_delta {
						let delta = reader.vec3();
						wl!("Teleport to someDynamicThing] delta: {delta:?}");
					} else {
						wl!("Teleport to someDynamicThing]");
					}
				}
				0xB5 => {
					let kind = reader.u8();
					if kind == 1 {
						let var_index = reader.u8();
						let value1 = reader.f32();
						let value2 = reader.f32();
						let value3 = reader.i32();
						wl!(
							"Set some arena stuff based on arena var] var index: {var_index}, value1: {value1}, value2: {value2}, value3: {value3}"
						);
					} else if kind == 0 {
						let thing_index = (reader.u8() - 1) % 16;
						let value3 = reader.u32();
						wl!(
							"Set some arena stuff based on arena thing index] thing index: {thing_index}, value: {value3}"
						);
					} else {
						wl!("Set some arena stuff (unknown)] kind: {kind}");
					}
				}
				0xB6 => {
					eprintln!("encountered unfinished opcode 0xB6 at {block_offset:06X}");
					let var = simple_var(reader);
					let value = reader.f32();
					// target?
					// todo probably broken
					wl!("Weird] var: {var}, value: {value}");
				}
				0xB7 => {
					let var = simple_var(reader);
					let count = reader.u8();
					w!("Call by var index] index: {var}, targets: [");
					for i in 0..count {
						let target = read_block(&mut blocks, reader);
						if i != 0 {
							w!(", ");
						}
						w!("{target}");
					}
					wl!("]");
				}
				0xB8 => {
					let [value1, radius, size] = reader.vec3().into();
					wl!(
						"Destroy alien (and damage area)] value1?: {value1}, radius?: {radius}, size? : {size}"
					);
				}
				0xB9 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someCmiDataValues0] if {comp} {branch}");
				}
				0xBA => {
					let a = reader.u8();
					let scale = 30.0 / reader.f32();
					wl!("Set someCmiField3] 30 * someCmiDataValues[0] * {scale} (unused: {a})");
				}
				0xBB => {
					let horizontal_speed = reader.f32();
					let vertical_speed = reader.f32();
					wl!(
						"Add random velocity] horizontal: {horizontal_speed}, vertical: {vertical_speed}"
					);
				}
				0xBC => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to player] if {comp} {branch}");
				}
				0xBD => {
					let value1 = reader.u8();
					if value1 != 0 && value1 != 1 {
						wl!("Move towards player] (noop)");
					} else {
						let max_speed = reader.f32();
						if value1 == 0 {
							let target_z = reader.f32();
							wl!(
								"Move towards player] max speed: {max_speed}, target z: {target_z}"
							);
						} else {
							wl!("Move towards player] max speed: {max_speed}");
						}
					}
				}
				0xBE => {
					let value = reader.u8() != 0;
					let name = reader.pascal_str();
					wl!("Set fan affects damp] name: {name}, on: {value}");
				}
				0xBF => {
					let index = reader.u8();
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					let abs = (index & 0x80) == 0;
					let index = index & !0x80;
					if index < 3 {
						let index = (b'x' + index) as char;
						wl!(
							"Branch on axis distance to player] index: {index} (abs: {abs}), if {comp} {branch}"
						);
					} else {
						wl!(
							"Branch on axis distance to player] index: {index} (abs: {abs}), if {comp} {branch}"
						);
					}
				}
				0xC0 => {
					let delta = reader.vec3();
					let height = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on can move to] delta: {delta:?}, height: {height}, {branch}");
				}
				0xC1 => {
					let code = reader.u8();
					if code <= 2 {
						wl!("Face velocity] code: {code}");
					} else if code < 4 {
						let target = reader.pascal_str();
						wl!("Face entity] code: {code}, target: {target}");
					} else {
						wl!("Face unknown?] code: {code}");
					}
				}
				0xC2 => {
					let visflag = tri_visflag(reader.u8());
					let id = reader.u8();
					let vs: [u8; 3] = reader.get();
					wl!("Do something with bsp vis] id: {id}, visflag: {visflag}, vs: {vs:?}");
				}
				0xC3 => {
					let value = reader.i8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some alien value] value: {value}, {branch}");
				}
				0xC4 => {
					eprintln!("encountered unfinished opcode 0xC4 at {block_offset:06X}");
					let num = var_or_data(reader);
					wl!("Set dtiArenaNum] num: {num}");
					// todo breaks out of loop here?
				}
				0xC5 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on hide] {branch}");
				}
				0xC6 => {
					let name = reader.pascal_str();
					let value1 = reader.u8();
					let value2 = reader.u32();
					let value3 = reader.u32();
					wl!(
						"Set someData] name: {name}, value1: {value1}, value2: {value2}, value3: {value3}"
					);
				}
				0xC7 => {
					let data = var_or_data(reader);
					wl!("Set someCmiData] {data}");
				}
				0xC8 => {
					let speed = reader.f32();
					let target = reader.vec3();
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Set someAnimVector, branch if done] speed: {speed}, target: {target:?}, {branch}"
					);
				}
				0xC9 => {
					let scale = reader.f32();
					let angle = reader.f32();
					wl!("Add some anim facing thing] scale: {scale}, angle: {angle}");
				}
				0xCA => {
					let background_hidden = reader.u8();
					wl!("Set background visibility] hidden: {background_hidden}");
				}
				0xCB => {
					let use_radius = reader.u8() == 1;
					if use_radius {
						let offset = reader.f32();
						wl!("Angle camera to alien] offset: {offset}");
					} else {
						wl!("Angle camera to alien]");
					}
				}
				0xCC => {
					wl!("Bounce]");
				}
				0xCD => {
					let value = reader.u8();
					wl!("Set someCmiField12] value: {value}");
				}
				0xCE => {
					let path_offset = reader.u32();
					offsets.path_offsets.push(path_offset);
					let length = reader.f32();
					let name = reader.pascal_str();
					let target = read_ext_block(reader, offsets, name, "Spawn on path");
					wl!(
						"Spawn aliens on path] name: {name}, spacing: {length}, init target: {target}, path offset: {path_offset:06X}"
					);
				}
				0xCF => {
					let speed = reader.f32();
					let angle = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Turn to angle] angle: {angle}, speed: {speed}, {branch}");
				}
				0xD0 => {
					let name = reader.pascal_str();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on has part] name: {name}, {branch}");
				}
				0xD1 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some alien stuff] {branch}");
				}
				0xD2 => {
					let value = var_or_data(reader);
					wl!("Set someScale] scale: {value}");
				}
				0xD3 => {
					wl!("Zero velocity]");
				}
				0xD4 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some field] {branch}");
				}
				0xD5 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to thing] if {comp} {branch}");
				}
				0xD6 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on angle to thing] if {comp} {branch}");
				}
				0xD7 => {
					let value = var_or_data(reader);
					wl!("Increase some global field to value] value: {value}");
				}
				0xD8 => {
					let var = simple_var(reader);
					let value = reader.f32();
					wl!("Add var] {var} += {value} * dt");
				}
				0xD9 => {
					let code = reader.u8();
					let value = reader.u32();
					wl!("Set some travglobal offset] code: {code}, value: {value}");
				}
				0xDA => {
					let angle = reader.f32();
					wl!("Set pitch angle] angle: {angle}");
				}
				0xDB => {
					let y = reader.f32();
					let z = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Target fire] y: {y}, z: {z}, target: {branch}");
				}
				0xDC => {
					let pos = reader.vec3();
					wl!("Set target] pos: {pos:?}");
				}
				0xDD => {
					let some_flag = reader.u8() == 1;
					let branch = branch_code(&mut blocks, reader);
					wl!("Try jumping] flag: {some_flag}, {branch}");
				}
				0xDE => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on instruction count] if {comp} {branch}");
				}
				0xDF => {
					let name = reader.pascal_str();
					wl!("Load arena] name: {name}");
				}
				0xE0 => {
					let active = reader.u8() != 0;
					if !active {
						wl!("Stop sliding]");
					} else {
						let angle = reader.f32();
						let speed = reader.f32();
						wl!("Update sliding] angle: {angle}, speed: {speed}");
					}
				}
				0xE1 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on pSomething existing] {branch}");
				}
				0xE2 => {
					let pos = reader.vec3();
					let value1 = reader.f32();
					let value2 = reader.f32();
					wl!("Set some stuff] pos: {pos:?}, value1: {value1}, value2: {value2}");
				}
				0xE3 => {
					eprintln!("encountered unfinished opcode 0xB6 at {block_offset:06X}");
					wl!("?]");
					// todo
					break;
				}
				0xE4 => {
					let name = reader.pascal_str();
					wl!("Set someDynamicThing] name: {name}");
				}
				0xE5 => {
					let turn_speed = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Turn towards home] turn speed: {turn_speed}, complete: {branch}");
				}
				0xE6 => {
					let position = reader.vec3();
					let angle = reader.f32();
					let arena_index = reader.i32();
					let object_name = reader.pascal_str();
					let init_target = read_ext_block(reader, offsets, object_name, "Spawn");
					wl!(
						"Spawn Entity 2] name: {object_name}, pos: {position:?}, angle: {angle}, arena_index: {arena_index}, init target: {init_target}"
					);
				}
				0xE7 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someCmiField and stuff] {branch}");
				}
				0xE8 => {
					let value = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Fixed branch?] value: {value}, {branch}");
				}
				0xE9 => {
					let name = reader.pascal_str();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on sound playing] name: {name}, {branch}");
				}
				0xEA => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on angle to player] if {comp} {branch}");
				}
				0xEB => {
					let nums = reader.vec4();
					wl!("Turn params] nums: {nums:?}");
				}
				0xEC => {
					let pos = reader.vec3();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on floor] pos: {pos:?}, {branch}");
				}
				0xED => {
					let min = reader.vec3();
					let max = reader.vec3();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on inside box] min: {min:?}, max: {max:?}, {branch}");
				}
				0xEE => {
					let component = match reader.u8() {
						n if n < 3 => (b'x' + n) as char,
						n => {
							eprintln!("invalid opcode 0xEE component {n}");
							'?'
						}
					};
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on position component] component: {component}, if {comp} {branch}");
				}
				0xEF => {
					let min = reader.vec3();
					let max = reader.vec3();
					wl!("Set someBbox] min: {min:?}, max: {max:?}");
				}
				0xF0 => {
					let value = reader.u8();
					wl!("Set global someCmiField] value: {value}");
				}
				0xF1 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some global pickup data] if {comp} {branch}");
				}
				0xF2 => {
					let has_matrix = reader.u8() != 0;
					if has_matrix {
						let matrix: [[f32; 4]; 3] = reader.get();
						wl!("Set some transform matrix] transform: {matrix:?}");
					} else {
						wl!("Clear some transform matrix]");
					}
				}
				0xF3 => {
					let index = reader.u8();
					let distance = reader.i16();
					let branch = branch_code(&mut blocks, reader);
					wl!(
						"Branch on visible] point index: {index}, distance: {distance}, target: {branch}"
					);
				}
				0xF4 => {
					let add = reader.u8() != 0;
					let value = reader.f32();
					if add {
						wl!("Add global cmiField1] value += {value}");
					} else {
						wl!("Set global cmiField1] value = {value}");
					}
				}
				0xF5 => {
					let mut name_len = reader.u8();
					let index = if name_len == 0 {
						let index = reader.u8();
						name_len = reader.u8();
						index
					} else {
						0
					};
					let name = reader.str(name_len as usize);
					wl!("Get buddy] index: {index}, name: {name}");
				}
				0xF6 => {
					let flags = reader.u8();
					let speed = reader.f32();
					let enable = flags & 1 != 0;
					let pitch = flags & 0x80 != 0;
					wl!("Turn to some thing] enable: {enable}, pitch: {pitch}, speed: {speed}");
				}
				0xF7 => {
					let msg_type = reader.u8();
					let message = reader.pascal_str();
					let duration = reader.f32();
					wl!(
						"Display Message] type: {msg_type}, message: {message}, duration: {duration}"
					);
				}
				0xF8 => {
					let value1 = reader.u8();
					let speed_x = reader.f32();
					let speed_y = reader.f32();
					if value1 == 0 {
						let value4 = reader.f32();
						wl!("Set sliding vars] x: {speed_x}, y: {speed_y}, value: {value4}");
					} else {
						wl!("Set sliding vars] x: {speed_x}, y: {speed_y}");
					}
				}
				0xF9 => {
					let named = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					if named == 1 {
						let name = reader.pascal_str();
						wl!("Branch on sound] name: {name}, {branch}");
					} else {
						wl!("Branch on sound] {branch}");
					}
				}
				0xFA => {
					let index = reader.u8();
					let name = if index == 0xFF {
						reader.pascal_str()
					} else {
						""
					};
					let dims = reader.u8();
					let [x_min, y_min, x_max, y_max, z_min, z_max];
					if dims == 2 {
						[x_min, y_min, x_max, y_max] = reader.vec4();
						[z_min, z_max] = [0.0, 0.0];
					} else {
						[x_min, y_min, z_min, x_max, y_max, z_max] = reader.get();
					}
					let target = branch_code(&mut blocks, reader);

					w!("Branch on part in box] ");
					if index == 0xFF {
						w!("name: {name}, ");
					} else {
						w!("index: {index}, ");
					}
					if dims == 2 {
						w!("min: [{x_min}, {y_min}], max: [{x_max}, {y_max}], ");
					} else {
						w!("min: [{x_min}, {y_min}, {z_min}], max: [{x_max}, {y_max}, {z_max}], ");
					}
					wl!("target: {target}");
				}
				0xFB => {
					let value = reader.u8();
					wl!("Set some flag about player pos] value: {value}");
				}
				0xFC => {
					let count = reader.u8();
					w!("Random call] targets:");
					for _ in 0..count {
						let target = read_block(&mut blocks, reader);
						w!(" {target}");
					}
					wl!();
				}
				0xFD => {
					wl!("Return]");
				}
			}
		}
		wl!("(end offset {:06X})\n", reader.position());
		block_index += 1;
	}

	result.summary = summary;
	result.anim_names.sort_unstable();
	result.anim_names.dedup();
	result.anim_offsets.sort_unstable();
	result.anim_offsets.dedup();
	result.path_offsets.sort_unstable();
	result.path_offsets.dedup();
	result.called_scripts.sort_unstable();
	result.called_scripts.dedup();

	result
}

#[cfg(test)]
mod tests {
	#[test]
	fn test_index() {
		for index in 0..255i32 {
			let i2 = index + ((index >> 4) + ((index < 0 && (index & 0xf) != 0) as i32)) * -0x10;
			//println!("{index} -> {i2}");
			assert_eq!(index % 16, i2, "{index} {i2}");
		}
	}
}
