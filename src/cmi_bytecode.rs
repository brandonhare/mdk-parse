use crate::Reader;

use std::fmt::Write;

fn var_target(index: u8) -> &'static str {
	match index {
		0 => "Global",
		1 => "Arena",
		2 => "Entity",
		3 => "Direct",
		n => format!("(Unknown {n})").leak(),
	}
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

#[derive(Default)]
struct BlockInfo {
	index: usize,
	offset: u32,
}
impl std::fmt::Display for BlockInfo {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.offset == 0 {
			f.write_str("(None)")
		} else {
			write!(f, "block_{} ({:X})", self.index, self.offset)
		}
	}
}

struct BranchInfo {
	code: u8,
	target1: BlockInfo,
	target2: BlockInfo,
}
impl std::fmt::Display for BranchInfo {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.code {
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

struct CompInfo {
	comp: u8,
	value2: f32,
	value3: f32,
}
impl std::fmt::Display for CompInfo {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let value2 = self.value2;
		let value3 = self.value3;
		match self.comp {
			1 | 3 => write!(f, "comp: (value < {value2})"),
			2 | 4 => write!(f, "comp: ({value2} < value)"),
			5 => write!(f, "comp: (value == {value2})"),
			6 => write!(f, "comp: (value == {value2})"),
			7 => write!(f, "comp: ({value2} <= value <= {value3})"),
			8 => write!(f, "comp: ({value2} </= value </= {value3})"),
			n => write!(
				f,
				"comp: (unknown: {n}, value2: {value2}, value3: {value3})"
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
		//write!(f, "value: {}", self.value)
		} else {
			write!(f, "{}_vars[{}]", var_target(self.target), self.index)
			/*
			write!(
				f,
				"target: {}, index: {}",
				var_target(self.target),
				self.index
			)
			*/
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
		write!(f, "{}_flags[{}]", var_target(self.target), self.index)
	}
}
fn flag_var(reader: &mut Reader) -> FlagVar {
	let target = reader.u8();
	let index = reader.u8();
	//assert_eq!(index & !31, 0, "flag value out of range");
	FlagVar { target, index }
}

fn read_path(reader: &Reader, offset: u32) -> Vec<crate::PathDataEntry> {
	let mut reader = reader.clone_at(offset as usize);
	let count = reader.u32();
	(0..count)
		.map(|_| {
			let chunk: [u32; 10] = reader.get();
			unsafe { std::mem::transmute(chunk) }
		})
		.collect()
}

pub fn parse_cmi(filename: &str, name: &str, reader: &mut Reader) -> String {
	let mut summary = String::new();
	if reader.position() == 0 {
		return summary;
	}

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
			wl!("main (offset {block_offset:X})");
		} else {
			wl!("block_{block_index} (offset {block_offset:X})");
		}

		reader.set_position(block_offset as usize);
		loop {
			let cmd = reader.u8();
			if cmd == 0xFF {
				break;
			}
			w!("[{cmd:02X} ");
			match cmd {
				0x0 | 0x7 | 0x1E => {
					wl!("Invalid!]");
					break;
				}
				0x01 => {
					wl!("Save bytecode?]");
				}
				0x02 => {
					let path_offset = reader.u32();
					let path = read_path(reader, path_offset);
					let value1 = reader.u8();
					let value2 = reader.u8();
					let value3 = reader.u16();
					let rest = reader.u8();
					let vec = if rest == 0 { reader.vec3() } else { [0.0; 3] };
					wl!("Set path] v1: {value1}, v2: {value2}, v3: {value3}, rest: {rest}, vec: {vec:?}, path (offset {path_offset:X}): {path:?}");
				}
				0x03 => {
					let cmi_data_3_offset = reader.u32();
					wl!("Set animation?] offset: {cmi_data_3_offset:X}");
				}
				0x04 => {
					let mut code1 = reader.u8();
					let mut code2 = 0;
					let mut index1 = 0;
					let mut index2 = 0;
					let mut f1 = 0.0;
					let mut f2 = 0.0;
					if code1 == 7 {
						code2 = reader.u8();
						if code2 == 0xfe {
							index1 = reader.u32();
							index2 = reader.u32();
						} else if code2 == 0xfc || code2 == 0x0c {
							index1 = reader.u32();
						}
						if code2 == 0xfc {
							code1 = 0xfc;
						}
					} else if code1 == 0x2b {
						f1 = reader.f32();
						f2 = reader.f32();
					}
					let code3 = reader.u8();
					let mut f3 = 0.0;
					if code3 == 6 || code3 == 10 {
						f3 = reader.f32();
					}
					let mut name = "";
					if matches!(code3, 2 | 7 | 4 | 5 | 6 | 10) {
						name = reader.pascal_str();
					}
					let mut num1 = 0;
					if code3 == 5 {
						num1 = reader.u32();
					}
					wl!("Give order] {code1} {code2} {index1} {index2} {f1} {f2} {code3} {f3} {name} {num1}");
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
					wl!("Branch if alien with name at index] name: {name}, index: {index}, {branch}");
				}
				0x0B => {
					let code = reader.u8();
					wl!("Set some command byte] {code}");
				}
				0x0C => {
					let count = reader.u8();
					w!("Random jump] targets:");
					for _ in 0..count {
						let block = read_block(&mut blocks, reader);
						w!(" {}", block);
					}
					wl!();
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
							"Set entity value] value: {value}, (some flag set: {})",
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
					wl!("Mortar path] path data offset: {offset:2X}");
				}
				0x1D => {
					let value1 = reader.u8();
					let name = reader.pascal_str();
					let target = read_block(&mut blocks, reader);
					wl!("CreateChain] value1: {value1}, name: {name}, target: {target}");
				}
				0x1F => {
					let count = reader.u8();
					w!("Set some part flags] names: [");
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
					w!("Clear some part flags?] names: [");
					for i in 0..count {
						let name = reader.pascal_str();
						if i != 0 {
							w!(", {name}")
						} else {
							w!("{name}")
						}
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
					wl!("Branch on vertical velocity] {comp}, {branch}");
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
					wl!("Branch on distance to player] {comp} {branch}");
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
					wl!("Branch on distance to something] {comp}, {branch}");
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
					wl!("Set entity someCmiField3] {var_data}");
				}
				0x3B => {
					let anim_offset = reader.u32();
					wl!("Set anim] offset: {anim_offset:X}");
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
					let target = read_block(&mut blocks, reader);
					if has_name == 0 {
						wl!("Spawn badguy] point index: {point_index}, name: {name2}, target: {target}");
					} else {
						wl!("Spawn badguy] target name: {name1}, name: {name2}, target: {target}");
					}
				}
				0x3E => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on angle to player] {comp}, {branch}");
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
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on variable compare] {var}, {comp}, {branch}");
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
					wl!("Set someDataField2] value: {value}");
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
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn entity 3] name: {name}, pos: {pos:?}, init target: {init_target}");
				}
				0x57 => {
					let min_dist = reader.u16();
					let max_dist = reader.u16();
					let angle = reader.u8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch if visible] min dist: {min_dist}, max dist: {max_dist}, angle: {angle}, {branch}");
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
					let pos = reader.vec3();
					let angle = reader.f32();
					let branch = branch_code(&mut blocks, reader);
					wl!("Trigger? (pos?)] pos: {pos:?}, angle: {angle}, {branch}");
				}
				0x61 => {
					let on = reader.u8();
					wl!("Set entity flag 80] set: {on}");
				}
				0x62 => {
					let triangle_id = reader.u8();
					let visflag = reader.u8();
					wl!("Set Triangle Visibility] id: {triangle_id}, visflag: {visflag}");
				}
				0x63 => {
					let trigger_index = (reader.i8() - 1) % 16;
					let id = reader.u8();
					let target = read_block(&mut blocks, reader);
					wl!("Set triangle damage trigger] trigger index: {trigger_index}, id: {id}, target: {target}");
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
					wl!("Set entitry arena2OrFloatValue] value: {value}");
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
					wl!("Set arena index] index: {index}");
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
					let init_target = read_block(&mut blocks, reader);
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
					wl!("Find entity and branch on comparison] name: {name}, {comp}, {branch}");
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
						[0.0; 3]
					};
					if value1 < 150 {
						if point_index == 0xFF {
							wl!("Create bubble] chance: {value1}%, pos: {pos:?}");
						} else {
							wl!("Create bubble] chance: {value1}%, pos: somePoints[{point_index:?}]");
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
					wl!("Shatter triangle 2] tri id: {tri_id}, vec: {vec1:?}, hitPoint1: {vec2:?}, hitPoint2: {vec3:?}");
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
					wl!("Create fan] name: {name}, bbox: {min:?}-{max:?}, value1: {value1}, value2: {value2}, speed: {speed}");
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
					wl!("Activate conveyor] id: {id}, name: {name}, speed: {speed}, size: {size:?}, scale: {scale:?}");
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
					let arena_index = reader.i32();
					let object_name = reader.pascal_str();
					let arena_name = reader.pascal_str();
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn Door] name: {object_name}, arena: {arena_name}, pos: {position:?}, angle: {angle}, arena_index: {arena_index}, init target: {init_target}");
				}
				0x96 => {
					let anim1_offset = reader.u32();
					let anim2_offset = reader.u32();
					wl!(
						"Set anims] anim1 offset: {anim1_offset:X}, anim2 offset: {anim2_offset:X}"
					);
				}
				0x97 => {
					let str1 = reader.pascal_str();
					let str2 = reader.pascal_str();
					let str3 = reader.pascal_str();
					let str4 = reader.pascal_str();
					wl!("Set door? properties] names: [\"{str1}\", \"{str2}\", \"{str3}\", \"{str4}\"]");
				}
				0x98 => {
					let flag = reader.u32();
					wl!("Set entity cmiFlag2] flag: {flag:X}");
				}
				0x99 => {
					let value = reader.f32();
					wl!("Set entity someDataField (float)] value: {value}");
				}
				0x9C => {
					let index = reader.u8();
					let name = reader.pascal_str();
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn alien] name: {name}, position: somePoints[{index}], init target: {init_target}");
				}
				0x9F => {
					let value1 = reader.u8();
					let pos1 = match value1 {
						0 => [0.0; 3],
						1 | 2 => reader.vec3(),
						n => {
							println!("invalid 0x9f opcode {n}");
							[0.0; 3]
						}
					};
					let pos2 = reader.vec3();
					let name = reader.pascal_str();
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn blit alien] name: {name}, position type: {value1}, pos1: {pos1:?}, pos2: {pos2:?}, init target: {init_target}");
				}
				0xA0 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on yaw] {comp}, {branch}");
				}
				0xA1 => {
					let position = reader.vec3();
					let object_name = reader.pascal_str();
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn Entity 1] name: {object_name}, pos: {position:?}, init target: {init_target}");
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
					wl!("Branch arena thing index comparison] thing index: {thing_index}, {comp}, {branch}");
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
					wl!("Some pickup comparison branch 1?] pickup index: {pickup_index}, {comp}, {branch}");
				}
				0xAF => {
					let pickup_type = reader.u8();
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Some pickup comparison branch 2?] pickup type: {pickup_type}, {comp}, {branch}");
				}
				0xB0 => {
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on flags 0x40000] {branch}");
				}
				0xB1 => {
					let value = var_or_data(reader);
					wl!("Set someCmiField1] value = {value}");
				}
				0xB2 => {
					let value1 = reader.u8();
					let pos = if value1 == 3 {
						let _ = reader.u8();
						[0.0; 3]
					} else {
						reader.vec3()
					};
					let radius = reader.f32();
					let value2 = reader.f32();
					let value3 = reader.f32();
					let value4 = reader.u8();
					wl!("Explosion] pos: {pos:?}, radius: {radius}, value1: {value1}, value2: {value2}, value3: {value3}, value4: {value4}");
				}
				0xB3 => {
					let name = reader.pascal_str();
					let init_target = read_block(&mut blocks, reader);
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
						wl!("Set some arena stuff based on arena var] var index: {var_index}, value1: {value1}, value2: {value2}, value3: {value3}");
					} else if kind == 0 {
						let thing_index = (reader.u8() - 1) % 16;
						let value3 = reader.u32();
						wl!("Set some arena stuff based on arena thing index] thing index: {thing_index}, value: {value3}");
					} else {
						wl!("Set some arena stuff (unknown)] kind: {kind}");
					}
				}
				/*
				0xB7 => {
					let var_target = var_target(reader.u8());
					let var_index = reader.u8();
					let value = reader.u8();
					let offset = reader.u32();
					let index = push_block(&mut blocks, offset);
					wl!("Do some cmiData3?, call?] target: {var_target}, index: {var_index}, value: {value}, offset: block_{index} ({offset:X})");
				}
				*/
				0xB8 => {
					let [value1, radius, size] = reader.vec3();
					wl!("Destroy alien (and damage area)] value1?: {value1}, radius?: {radius}, size? : {size}");
				}
				0xB9 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on someCmiDataValues0] {comp}, {branch}");
				}
				0xBA => {
					let scale = 30.0 / reader.f32();
					wl!("Set someCmiField3] 30 * someCmiDataValues[0] * {scale}");
				}
				0xBB => {
					let horizontal_speed = reader.f32();
					let vertical_speed = reader.f32();
					wl!("Add random velocity] horizontal: {horizontal_speed}, vertical: {vertical_speed}");
				}
				0xBC => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to player] {comp} {branch}");
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
						wl!("Branch on axis distance to player] index: {index} (abs: {abs}), {comp}, {branch}");
					} else {
						wl!("Branch on axis distance to player] index: {index} (abs: {abs}), {comp}, {branch}");
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
				0xC3 => {
					let value = reader.i8();
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on some alien value] value: {value}, {branch}");
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
					wl!("Set someData] name: {name}, value1: {value1}, value2: {value2}, value3: {value3}");
				}
				0xC7 => {
					let data = var_or_data(reader);
					wl!("Set someCmiData] {data}");
				}
				0xC8 => {
					let speed = reader.f32();
					let target = reader.vec3();
					let branch = branch_code(&mut blocks, reader);
					wl!("Set someAnimVector, branch if done] speed: {speed}, target: {target:?}, {branch}");
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
				0xCD => {
					let value = reader.u8();
					wl!("Set someCmiField12] value: {value}");
				}
				0xCE => {
					let path_offset = reader.u32();
					let path = read_path(reader, path_offset);
					let length = reader.f32();
					let name = reader.pascal_str();
					let target = read_block(&mut blocks, reader);
					wl!("Spawn aliens on path] name: {name}, spacing: {length}, init target: {target}, path (offset {path_offset:X}): {path:?}");
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
				0xD2 => {
					let value = var_or_data(reader);
					wl!("Set someScale] scale: {value}");
				}
				0xD3 => {
					wl!("Zero velocity]");
				}
				0xD5 => {
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on distance to thing] {comp}, {branch}");
				}
				0xD8 => {
					let var = simple_var(reader);
					let value = reader.f32();
					wl!("Add var with delta] {var} += {value} * dt");
				}
				0xD9 => {
					let code = reader.u8();
					let value = reader.u32();
					wl!("Set some travglobal offset] code: {code}, value: {value}");
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
					wl!("Branch on instruction count] {comp}, {branch}");
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
					let init_target = read_block(&mut blocks, reader);
					wl!("Spawn Entity 2] name: {object_name}, pos: {position:?}, angle: {angle}, arena_index: {arena_index}, init target: {init_target}");
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
					wl!("Branch on angle to player] {comp}, {branch}");
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
						n => '?',
					};
					let comp = compare(reader);
					let branch = branch_code(&mut blocks, reader);
					wl!("Branch on position component] component: {component}, {comp}, {branch}");
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
					wl!("Branch on some global pickup data] {comp}, {branch}");
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
				0xF4 => {
					let add = reader.u8() != 0;
					let value = reader.f32();
					if add {
						wl!("Add global cmiField1] value += {value}");
					} else {
						wl!("Set global cmiField1] value = {value}");
					}
				}
				0xF7 => {
					let msg_type = reader.u8();
					let message = reader.pascal_str();
					let duration = reader.f32();
					wl!("Display Message] type: {msg_type}, message: {message}, duration: {duration}");
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
				n => {
					wl!("?]");
					break;
				}
			}
		}
		wl!("(end offset {:X})\n", reader.position());
		block_index += 1;
	}
	summary
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
