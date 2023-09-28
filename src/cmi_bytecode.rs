use crate::Reader;

use std::fmt::Write;

fn var_target(index: u8) -> &'static str {
	match index {
		0 => "Global",
		1 => "Arena",
		2 => "Entity",
		n => format!("(Unknown {n})").leak(),
	}
}

pub fn parse_cmi(start_offset: usize, reader: &mut Reader) -> String {
	let mut summary = format!("Start offset: {start_offset}\n");
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
	loop {
		let cmd = reader.u8();
		if cmd == 0xFF {
			break;
		}
		w!("[{cmd:02X} ");
		match cmd {
			0x01 => {
				wl!("Save bytecode?]");
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
			0x09 => {
				wl!("End?]");
			}
			0x0B => {
				let code = reader.u8();
				wl!("Set some command byte] {code}");
			}
			0x40 => {
				let index = reader.u8();
				if index == 3 {
					let f = reader.f32();
					wl!("Delay]: {f}");
				} else {
					let index2 = reader.u8();
					wl!("Delay (Variable)] var index 1: {index} var index 2: {index2}");
				}
			}
			0x41 => {
				let var_target = var_target(reader.u8());
				let var_index = reader.u8();
				let value = reader.f32();
				wl!("Set Variable] target: {var_target}, index: {var_index}, value: {value}");
			}
			0x47 => {
				let flag_target = var_target(reader.u8());
				let code1 = reader.u8();
				let code2 = reader.u8();
				let mut offset1 = 0;
				let mut offset2 = 0;
				match code2 {
					0xFE => {
						offset1 = reader.u32();
						offset2 = reader.u32();
					}
					0xFC | 0x0C => {
						offset1 = reader.u32();
					}
					_ => {}
				}
				wl!("Branch? (on flag?)] flag target: {flag_target}, code1: {code1:02X}, code2: {code2:02X}, offset1: {offset1}, offset2: {offset2}");
			}
			0x56 => {
				let pos = reader.vec3();
				let name = reader.pascal_str();
				let cmi_index = reader.u32();
				wl!("Spawn entity 3] name: {name}, pos: {pos:?}, cmi init: {cmi_index}");
			}
			0x60 => {
				let pos = reader.vec3();
				let angle = reader.f32();
				let code = reader.u8();
				let mut offset1 = 0;
				let mut offset2 = 0;
				match code {
					0xFE => {
						offset1 = reader.u32();
						offset2 = reader.u32();
					}
					0xFC | 0x0C => {
						offset1 = reader.u32();
					}
					_ => {}
				}
				wl!("Trigger? (pos?)] pos: {pos:?}, angle: {angle}, code: {code:02X}, offset1: {offset1}, offset2: {offset2}");
			}
			0x62 => {
				let triangle_id = reader.u8();
				let visflag = reader.u8();
				wl!("Set Triangle Visibility] id: {triangle_id}, visflag: {visflag}");
			}
			0x63 => {
				let code = reader.u8();
				let index = reader.u8();
				let value = reader.u32();
				wl!("Set some arena value?] code: {code}, index: {index}, value: {value}");
			}
			0x67 => {
				let aabb_max: [f32; 3] = reader.vec3();
				let aabb_min: [f32; 3] = reader.vec3();
				let code = reader.u8();
				let mut offset1 = 0;
				let mut offset2 = 0;
				match code {
					0xFE => {
						offset1 = reader.u32();
						offset2 = reader.u32();
					}
					0xFC | 0x0C => {
						offset1 = reader.u32();
					}
					_ => {}
				}
				wl!("Trigger? (aabb)] min: {aabb_min:?}, max: {aabb_max:?}, code: {code:02X}, offset1: {offset1}, offset2: {offset2}");
			}
			0x7B => {
				let code = reader.u8();
				let mut offset1 = 0;
				let mut offset2 = 0;
				match code {
					0xFE => {
						offset1 = reader.u32();
						offset2 = reader.u32();
					}
					0xFC | 0x0C => {
						offset1 = reader.u32();
					}
					_ => {}
				}
				wl!("Branch (arena)?] code: {code:02X}, offset1: {offset1}, offset2: {offset2}");
			}
			0x95 => {
				// spawn door
				let position = reader.vec3();
				let angle = reader.f32();
				let arena_index = reader.i32();
				let object_name = reader.pascal_str();
				let arena_name = reader.pascal_str();
				let cmi_init_offset = reader.u32();
				wl!("Spawn Door] name: {object_name}, arena: {arena_name}, pos: {position:?}, angle: {angle}, arena_index: {arena_index}, cmi init offset: {cmi_init_offset}");
			}
			0xA1 => {
				let position = reader.vec3();
				let object_name = reader.pascal_str();
				let cmi_init_offset = reader.u32();
				wl!("Spawn Entity 1] name: {object_name}, pos: {position:?}, cmi init offset: {cmi_init_offset}");
			}
			0xA8 => {
				let triangle_id = reader.u8();
				let num = reader.u8();
				wl!("Set triangle vis? 2] id: {triangle_id}, num: {num}");
			}
			0xE6 => {
				let position = reader.vec3();
				let angle = reader.f32();
				let arena_index = reader.i32();
				let object_name = reader.pascal_str();
				let cmi_init_offset = reader.u32();
				wl!("Spawn Entity 2] name: {object_name}, pos: {position:?}, angle: {angle}, arena_index: {arena_index}, cmi init offset: {cmi_init_offset}");
			}
			//0xDF => { let arena_name = reader.pascal_str(); }
			0xFC => {
				let count = reader.u8();
				let nums = reader.get_vec::<u32>(count as usize);
				wl!("Random code jump] offsets: {nums:?}");
			}
			_ => {
				let remaining_start_offset = reader.position();
				wl!("?]\nRemaining Stream (starting at {remaining_start_offset}):");

				loop {
					let b = reader.u8();
					if b == 0xFF {
						break;
					}
					w!("{b:02X}");
				}
				let end_offset = reader.position();
				wl!(
					"\nStream finished at {end_offset}, {} total bytes ({} remaining stream bytes, {} unused in remainder of chunk)",
					end_offset - start_offset,
					end_offset - remaining_start_offset,
					reader.remaining_len(),
				);
				break;
			}
		}
	}
	summary
}
