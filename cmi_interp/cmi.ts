
function assert<T>(condition: T, message = ""): asserts condition {
	if (!condition)
		throw new Error("assert failed: " + message);
}
function assertExists<T>(value: T) {
	assert(!!value, "value does not exist!");
	return value;
}

class Reader {
	data: DataView;
	offset: number;

	constructor(data: DataView, offset: number = 0) {
		this.data = data;
		this.offset = offset;
	}

	clone(target_offset = this.offset) {
		return new Reader(this.data, target_offset);
	}
	skip(n: number) {
		this.offset += n;
	}

	u8() {
		return this.data.getUint8(this.offset++);
	}
	i8() {
		return this.data.getInt8(this.offset++);
	}
	u16() {
		const result = this.data.getUint16(this.offset, true);
		this.offset += 2;
		return result;
	}
	i16() {
		const result = this.data.getInt16(this.offset, true);
		this.offset += 2;
		return result;
	}
	u32() {
		const result = this.data.getUint32(this.offset, true);
		this.offset += 4;
		return result;
	}
	i32() {
		const result = this.data.getInt32(this.offset, true);
		this.offset += 4;
		return result;
	}
	f32() {
		const result = this.data.getFloat32(this.offset, true);
		this.offset += 4;
		return result;
	}
	vec3(): [number, number, number] {
		const x = this.data.getFloat32(this.offset, true);
		const y = this.data.getFloat32(this.offset + 4, true);
		const z = this.data.getFloat32(this.offset + 8, true);
		this.offset += 12;
		return [x, y, z];
	}

	str(n: number) {
		const bytes: number[] = [];
		for (let i = 0; i < n; ++i) {
			const c = this.data.getUint8(this.offset + i);
			if (c === 0) {
				for (let j = i + 1; j < n; ++j) {
					assert(this.data.getUint8(this.offset + j) === 0);
				}
				break;
			}
			assert(c >= 32 && c < 127, "string not ascii!");
			bytes.push(c);
		}
		this.offset += n;
		return String.fromCharCode(...bytes);
	}
	pstr() {
		return this.str(this.u8());
	}
}


class Entity {
	name: string;
	callstack: Reader[];

	constructor(name: string, reader?: Reader) {
		this.name = name;
		this.callstack = (reader?.offset) ? [reader] : [];
	}
};
class Arena extends Entity {
	entities: Entity[] = [];
	entity_setup_templates: Entity[] = [];
};
interface Level {
	name: string;
	arenas: Map<string, Arena>;
};

export function go(buffer: DataView) {
	const data = new Reader(buffer);
	const level_name = data.str(12);
	data.skip(4);

	function read_offsets() {
		const num_offsets = data.u32();
		const result = new Array<[string, number]>(num_offsets);
		for (let i = 0; i < num_offsets; ++i) {
			const name = data.pstr();
			const offset = data.u32();
			result[i] = [name, offset];
		}
		return result;
	}

	const init_offsets = read_offsets();
	const mesh_offsets = read_offsets();
	const setup_offsets = read_offsets();
	const arena_offsets = read_offsets();

	const level: Level = {
		name: level_name,
		arenas: new Map()
	};

	for (const arena of arena_offsets) {
		const name = arena[0];
		const offset = arena[1];

		const bytecode = data.clone(offset);
		const m1 = bytecode.pstr();
		const m2 = bytecode.pstr();
		bytecode.offset = bytecode.u32();

		level.arenas.set(name, new Arena(name, bytecode));
	}
	for (const init of init_offsets) {
		const [arena_name, entity_name] = init[0].split('$');
		const offset = init[1];

		const arena = level.arenas.get(arena_name)!;
		arena.entities.push(new Entity(entity_name, data.clone(offset)));
	}
	for (const setup of setup_offsets) {
		const [arena_name, entity_name] = setup[0].split('$');
		const offset = setup[1];

		const arena = level.arenas.get(arena_name)!;
		arena.entity_setup_templates.push(new Entity(entity_name, data.clone(offset)));
	}

	level.arenas.forEach(arena => {
		for (const entity of arena.entities) {
			run_bytecode(level, arena, entity);
		}
		for (const entity of arena.entity_setup_templates) {
			run_bytecode(level, arena, entity);
		}

		run_bytecode(level, arena, arena);

		for (const entity of arena.entities) {
			run_bytecode(level, arena, entity);
		}
	});

	return level;
}

const temp: OpcodeBase[] = [];

function run_bytecode(level: Level, arena: Arena, entity: Entity) {

	if (entity.callstack.length === 0)
		return;

	console.log(`running ${level.name} ${arena.name} ${entity.name}`);

	let loopcount = 10000;
	while (entity.callstack.length > 0) {
		assert(entity.callstack.length < 1000, "stack overflow");

		const data = entity.callstack[entity.callstack.length - 1];
		while (true) {
			assert(loopcount--, "infinite loop");

			const opcode = data.u8();
			if (opcode === 0xFF) {
				console.log("finished executing");
				if (entity.callstack.length !== 1) {
					console.warn("callstack not empty!");
				}
				entity.callstack.length = 0;
				return;
			}

			let thing = opcodes[opcode];
			assert(thing !== null, "invalid opcode " + opcode);
			if (thing === 0xFF) {
				// done
				if (entity.callstack.length > 1)
					console.warn("finished executing but callstack remains!");
				entity.callstack.length = 0;
				return;
			} else if (thing instanceof Todo) {
				// todo
				entity.callstack.length = 0;
				return;
			}


			if (!Array.isArray(thing)) {
				temp.length = 1;
				temp[0] = thing;
				thing = temp;
			}

			for (const code of thing) {
				if (typeof (code) === "number") {
					data.skip(code);
					continue;
				}
				switch (code) {
					case 'p': {
						data.pstr();
						break;
					}
					case 'b': {
						const c = data.u8();
						if (c === 0xFE) {
							data.skip(8);
						} else if (c === 0xFC || c === 0xC) {
							data.skip(4);
						}
						break;
					}
					case "c": {
						const c = data.u8();
						if (c === 7 || c === 8)
							data.skip(8);
						else
							data.skip(4);
						break;
					}
					case 'v': {
						if (data.u8() === 3)
							data.skip(4);
						else
							data.skip(1);
						break;
					}
				}
			}

		}
	}
}

class Todo { constructor(public n: number) {} }
function todo(n: number) { return new Todo(n); }

const p = 'p';
const b = 'b';
const v = 'v';
const c = 'c';

type OpcodeBase = number | typeof p | typeof b | typeof v | typeof c;
type Opcode = OpcodeBase | OpcodeBase[] | null | Todo;

const pb: Opcode = [p, b];
const cb: Opcode = [c, b];

const opcodes: Opcode[] = [
	// 0x00
	null,
	0, // 0x01 !
	todo(0x02),
	todo(0x03),
	todo(0x04),
	4,
	0,
	null,
	2,
	0, //0x09 !
	[p, 1, b],
	1,
	todo(0x0C),
	b,
	[3, b],
	0,

	// 0x10
	2,
	b,
	[4, b],
	0,
	0,
	4,
	b,
	1,
	[1, p],
	p,
	p,
	b,
	4,
	[1, p, 4],
	null,
	todo(0x1F),

	// 0x20
	todo(0x20),
	[4, b],
	b,
	1,
	1,
	b,
	cb,
	v,
	v,
	1,
	todo(0x2A),
	8,
	b,
	cb,
	b,
	[4, b],

	// 0x30
	[4, b],
	[1, b],
	v,
	v,
	v,
	v,
	cb,
	v,
	2,
	[3, b],
	v,
	todo(0x3B),
	0,
	todo(0x3D),
	cb,
	1,

	// 0x40
	v,
	6,
	6,
	[2, c, b],
	2,
	2,
	2,
	[2, b],
	[2, b],
	1,
	[2, p],
	0,
	4,
	[1, p],
	12,
	12,

	// 0x50
	12,
	[1, v],
	v,
	todo(0x53),
	v,
	1,
	[12, p, b],
	[5, b],
	1,
	todo(0x59),
	[p, 4],
	v,
	2,
	16,
	todo(0x5E),
	todo(0x5F),

	// 0x60
	[16, b],
	1,
	2,
	6,
	p,
	0,
	b,
	[24, b],
	4,
	20,
	4,
	p,
	b,
	1,
	0,
	4,

	// 0x70
	[p, 16],
	[12, p, 4],
	b,
	[8, b],
	4,
	4,
	4,
	[p, c, b],
	4,
	[4, b],
	8,
	b,
	4,
	0, //!
	0,
	cb,

	// 0x80
	[p, 2],
	todo(0x81),
	0,
	1,
	todo(0x84),
	[p, 5],
	v,
	4,
	34,
	13,
	37,
	25,
	3,
	9,
	[1, p, 6],
	p,

	// 0x90
	[p, 30],
	[p, 8],
	[1, p, 24],
	p,
	[p, 2],
	[20, p, p, 4],
	todo(0x96),
	[p, p, p, p],
	4,
	4,
	2,
	[v, b],
	[1, p, 4],
	[p, 8],
	todo(0x9E),
	todo(0x9F),

	// 0xA0
	cb,
	[12, p, 4],
	3,
	[1, c, b],
	todo(0xA4),
	b,
	b,
	4,
	2,
	v,
	4,
	b,
	todo(0xAC),
	todo(0xAD),
	[1, c, b],
	[1, c, b],

	// 0xB0
	b,
	v,
	todo(0xB2),
	[p, 4],
	todo(0xB4),
	todo(0xB5),
	null,
	todo(0xB7),
	12,
	cb,
	5,
	8,
	cb,
	todo(0xBD),
	[1, p],
	[1, c, b],

	// 0xC0
	[16, b],
	todo(0xC1),
	5,
	[1, b],
	null,
	b,
	[p, 9],
	v,
	[16, b],
	8,
	1,
	todo(0xCB),
	0,
	1,
	[8, p, 4],
	[8, b],

	// 0xD0
	pb,
	b,
	v,
	0,
	b,
	cb,
	cb,
	v,
	6,
	5,
	4,
	[8, b],
	12,
	[1, b],
	cb,
	p,

	// 0xE0
	todo(0xE0),
	b,
	20,
	null,
	p,
	[4, b],
	[20, p, b],
	b,
	[1, b],
	pb,
	cb,
	16,
	[12, b],
	[24, b],
	[1, c, b],
	24,

	// 0xF0
	1,
	cb,
	todo(0xF2),
	[3, b],
	5,
	todo(0xF5),
	5,
	[1, p, 4],
	todo(0xF8),
	todo(0xF9),
	todo(0xFA),
	1,
	todo(0xFC),
	0, //!
	null,
	null,
];
