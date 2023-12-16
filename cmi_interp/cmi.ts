
function assert<T>(condition: T, message = "assert failed"): asserts condition {
	if (!condition)
		throw new Error("assert failed: " + message);
}
function assertExists<T>(value: T, message = "value does not exist") {
	assert(!!value, message);
	return value;
}

export type vec3 = [number, number, number];

export class Reader {
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
	vec3(): vec3 {
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
	id: number;
	position: vec3;
	yawAngle = 0;
	health = 100; // todo find default

	arena: Arena;
	script: Reader | null = null;
	callstack: number[] = [];

	log: string[] = [];

	constructor(name: string, id: number, parent: Arena, position: vec3 = [0, 0, 0], yawAngle = 0) {
		this.name = name;
		this.id = id;
		this.arena = parent;
		this.position = position;
		this.yawAngle = yawAngle;

		this.log.push(`Name: ${name}, id: ${id}, position: ${position}, angle: ${yawAngle}`);
	}

	setScriptOffset(offset: number) {
		if (!offset) {
			this.script = null;
		} else if (this.script) {
			this.script.offset = offset;
		} else {
			this.script = this.arena.data.clone(offset);
		}
	}

	executeScript() {
		const script = this.script;
		if (!script)
			return;
		while (true) {
			if (!executeOpcode(this, script)) {
				break;
			}
		}
	}
};

class Arena extends Entity {
	level: Level;
	data: Reader;
	setupOffsets = new Map<string, number>();
	entities: Entity[] = [];

	constructor(name: string, id: number, level: Level, data: Reader) {
		super(name, id, null!);
		this.arena = this;
		if (data.offset)
			this.script = data;

		this.level = level;
		this.data = data;
	}

	spawnEntity(entity: Entity, scriptOffset: number) {
		this.entities.push(entity);
		const setupOffset = this.setupOffsets.get(entity.name);
		if (setupOffset) {
			entity.setScriptOffset(setupOffset);
			entity.executeScript();
		}
		entity.setScriptOffset(scriptOffset);
	}

	runScripts() {
		for (const entity of this.entities) {
			entity.executeScript();
		}
		this.executeScript();
	}
};
class Level {
	name: string;
	arenas = new Map<string, Arena>();

	constructor(name: string) { this.name = name; }

	runScripts() {
		this.arenas.forEach(arena => arena.runScripts());
	}
};


function readOffsets(data: Reader) {
	const count = data.u32();
	const result = new Map<string, number>();
	for (let i = 0; i < count; ++i) {
		const name = data.pstr();
		const offset = data.u32();
		result.set(name, offset);
	}
	return result;
}

export type BspEntity = {
	arenaName: string,
	name: string,
	id: number,
	position: vec3,
	value: number,
};

export function go(buffer: DataView, entities: BspEntity[]) {
	const data = new Reader(buffer);
	const levelName = data.str(12);
	data.skip(4); // filesize

	const initOffsets = readOffsets(data);
	const meshOffsets = readOffsets(data);
	const setupOffsets = readOffsets(data);
	const arenaOffsets = readOffsets(data);

	const level = new Level(levelName);

	const arenas = arenaOffsets as Map<string, any> as Map<string, Arena>;
	level.arenas = arenas;
	let arenaIndex = 0;
	arenaOffsets.forEach((offset, name) => {
		data.offset = offset;
		data.pstr();
		data.pstr();
		const scriptOffset = data.u32();
		const arena = new Arena(name, arenaIndex++, level, data.clone(scriptOffset));
		arenas.set(name, arena);
	});

	setupOffsets.forEach((offset, name) => {
		const [arenaName, entityName] = name.split('$');
		const arena = assertExists(arenas.get(arenaName));
		arena.setupOffsets.set(entityName, offset);
	});

	for (const entityDef of entities) {
		const arena = assertExists(arenas.get(entityDef.arenaName));
		const initOffset = initOffsets.get(`${entityDef.arenaName}$${entityDef.name}_${entityDef.id}`) ?? 0;

		const entity = new Entity(entityDef.name, entityDef.id, arena, entityDef.position);
		arena.spawnEntity(entity, initOffset);
	}

	level.runScripts();
	level.runScripts();

	return level;
}

class PickupEntity extends Entity {}
class DoorEntity extends Entity {
	targetArena: Arena;
	doorFlags: DoorFlags = DoorFlags.CLOSED; // todo check default
	openDistance = 100; // todo find default

	constructor(name: string, id: number, parent: Arena, position: vec3, angle: number, targetArena: Arena) {
		super(name, id, parent, position, angle);
		this.targetArena = targetArena;
	}
}

enum DoorFlags {
	OPEN = 1,
	OPENING = 2,
	CLOSING = 4,
	CLOSED = 8,
	HIDE_WHEN_OPEN = 0x10,
	STAY_OPEN = 0x20,
	LOCKED = 0x40,
	JUST_NUKED = 0x80,
	HIDE_LOCK = 0x100
}


function executeOpcode(entity: Entity, script: Reader) {

	function log(...msg: any[]) {
		/*
		const entityName = (entity instanceof Arena)
			? entity.name
			: `${entity.arena.name}$${entity.name}_${entity.id}`;
		*/
		//const logMsg = `[${entityName.padEnd(18)}][${offset.toString(16).padStart(6, "0").toUpperCase()}: ${opcode.toString(16).padStart(2, "0").toUpperCase()}]: ${msg.join(' ')}`;
		//console.log(logMsg);
		const logMsg = `[${offset.toString(16).padStart(6, "0").toUpperCase()}: ${opcode.toString(16).padStart(2, "0").toUpperCase()}]: ${msg.join(' ')}`;
		entity.log.push(logMsg);
	}

	const arena = entity.arena;
	const offset = script.offset;
	const opcode = script.u8();

	switch (opcode) {
		case 1: { // set script resume point
			log("set resume point");
			// todo do it
			break;
		}
		case 0x0B: // set some command byte
			const someValue = script.u8();
			// todo
			log("set some value", someValue);
			break;
		case 0x09: { // clear callstack
			log("clear function stack");
			entity.callstack.length = 0; // todo is this correct
			break;
		}
		case 0x10: { // set health
			entity.health = script.u16();
			log("set health", entity.health);
			// todo destroy on health = 0?
			break;
		}
		case 0x49: { // set some command byte
			const someValue = script.u8();
			// todo
			log("set some value", someValue);
			break;
		}
		case 0x95: { // spawn door
			const position = script.vec3();
			const angle = script.f32();
			const id = script.i32();
			const name = script.pstr();
			const targetArenaName = script.pstr();
			const scriptOffset = script.u32();
			const targetArena = assertExists(arena.level.arenas.get(targetArenaName));
			log("spawn door", position, angle, id, name, targetArenaName, scriptOffset);
			const door = new DoorEntity(name, id, arena, position, angle, targetArena);
			arena.spawnEntity(door, scriptOffset);
			break;
		}
		case 0x96: { // set door animations
			assert(entity instanceof DoorEntity);
			const openAnimOffset = script.u32();
			const closeAnimOffset = script.u32();
			log("set door animations", openAnimOffset, closeAnimOffset);
			break;
		}
		case 0x97: { // set door sounds
			assert(entity instanceof DoorEntity);
			const openSoundName = script.pstr();
			const closeSoundName = script.pstr();
			const openFinishSoundName = script.pstr();
			const closeFinishSoundName = script.pstr();
			log("set door sounds", openSoundName, closeSoundName, openFinishSoundName, closeFinishSoundName);
			break;
		}
		case 0x98: { // set door flags
			assert(entity instanceof DoorEntity);
			// todo check masking
			entity.doorFlags = script.u32();
			log("set door flags", entity.doorFlags, DoorFlags[entity.doorFlags]);
			break;
		}
		case 0x99: { // set door open distance
			assert(entity instanceof DoorEntity);
			entity.openDistance = script.f32();
			log("set door open distance", entity.openDistance);
			break;
		}
		case 0xA1: { // spawn pickup
			const position = script.vec3();
			const name = script.pstr();
			const scriptOffset = script.u32();

			log("spawn pickup", name, position);
			const entity = new PickupEntity(name, 0, arena, position);
			arena.spawnEntity(entity, scriptOffset);
			break;
		}
		case 0xE6: { // spawn entity
			const position = script.vec3();
			const angle = script.f32();
			const id = script.i32();
			const name = script.pstr();
			const scriptOffset = script.u32();
			log("spawn entity", name);
			const entity = new Entity(name, id, arena, position, angle);
			arena.spawnEntity(entity, scriptOffset);
			break;
		}
		case 0xFF: // end
			assert(entity.callstack.length === 0);
			log("finished script");
			entity.script = null;
			return false;
		default:
			log("unknown opcode!");
			entity.script = null;
			return false;
	}
	return true;
}
