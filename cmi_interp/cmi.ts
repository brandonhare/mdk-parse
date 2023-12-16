
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

enum EntityFlags {
}

class Entity {
	name: string;
	id: number;
	position: vec3;
	yawAngle = 0;
	health = 10;

	flags: EntityFlags = 0;
	minOrderRange = 0;
	maxOrderRange = 0;
	variables = [0, 0, 0, 0];

	arena: Arena;
	scriptOffset = 0;
	scriptDelay = 0;
	scriptOffsetAfterDelay = 0;

	log: string[] = [];

	constructor(name: string, id: number, parent: Arena, position: vec3 = [0, 0, 0], yawAngle = 0) {
		this.name = name;
		this.id = id;
		this.arena = parent;
		this.position = position;
		this.yawAngle = yawAngle;

		this.log.push(`Name: ${name}, id: ${id}, position: ${position}, angle: ${yawAngle}`);
	}

	executeScript() {
		let offset = this.scriptOffset;

		if (this.scriptOffsetAfterDelay) {
			if (this.scriptDelay > 0) {
				return;
			}
			offset = this.scriptOffsetAfterDelay;
			this.scriptOffsetAfterDelay = 0;
		}

		if (!offset)
			return;

		const script = this.arena.data.clone(offset);
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
	arenaVariables = [0, 0, 0, 0];

	constructor(name: string, id: number, level: Level, data: Reader) {
		super(name, id, null!);
		this.arena = this;
		this.scriptOffset = data.offset;

		this.level = level;
		this.data = data;
	}

	spawnEntity(entity: Entity, scriptOffset: number) {
		this.entities.push(entity);
		const setupOffset = this.setupOffsets.get(entity.name);
		if (setupOffset) {
			entity.scriptOffset = setupOffset;
			entity.log.push("Running setup");
			entity.executeScript();
			entity.log.push("Setup complete");
		}
		entity.scriptOffset = scriptOffset;
	}

	forAllEntities(func: (entity: Entity) => void, includeSelf: boolean) {
		for (const entity of this.entities) {
			func(entity);
		}
		if (includeSelf)
			func(this);
	}
};
class Level {
	name: string;
	arenas: Arena[] = [];
	variables = [0, 0, 0, 0];

	constructor(name: string) { this.name = name; }

	findArena(name: string) {
		for (const arena of this.arenas) {
			if (arena.name === name)
				return arena;
		}
		assert(false, "arena not found");
	}

	forAllEntities(func: (entity: Entity) => void, includeArenas: boolean) {
		for (const arena of this.arenas) {
			arena.forAllEntities(func, includeArenas);
		};
	}
};



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

	const initOffsets = readOffsets(data);
	const meshOffsets = readOffsets(data);
	const setupOffsets = readOffsets(data);
	const arenaOffsets = readOffsets(data);

	const level = new Level(levelName);

	let arenaIndex = 0;
	arenaOffsets.forEach((offset, name) => {
		data.offset = offset;
		data.pstr();
		data.pstr();
		const scriptOffset = data.u32();
		const arena = new Arena(name, arenaIndex++, level, data.clone(scriptOffset));
		level.arenas.push(arena);
	});

	setupOffsets.forEach((offset, name) => {
		const [arenaName, entityName] = name.split('$');
		const arena = level.findArena(arenaName);
		arena.setupOffsets.set(entityName, offset);
	});

	for (const entityDef of entities) {
		const arena = level.findArena(entityDef.arenaName);
		const initOffset = initOffsets.get(`${entityDef.arenaName}$${entityDef.name}_${entityDef.id}`) ?? 0;

		const entity = new Entity(entityDef.name, entityDef.id, arena, entityDef.position);
		arena.spawnEntity(entity, initOffset);
	}

	for (let i = 0; i < 10; ++i) {
		level.forAllEntities((entity) => {
			entity.scriptDelay = Math.max(0, entity.scriptDelay - 0.5);
			entity.executeScript();
		}, true);
	}

	return level;
}

class PickupEntity extends Entity {}
class DoorEntity extends Entity {
	targetArena: Arena;
	doorFlags: DoorFlags = DoorFlags.CLOSED;
	openDistance = 20;

	constructor(name: string, id: number, parent: Arena, position: vec3, angle: number, targetArena: Arena) {
		super(name, id, parent, position, angle);
		this.log[0] += ", target arena: " + targetArena.name;
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

function getVar(entity: Entity, script: Reader): number {
	const enum VarType {
		LEVEL = 0,
		ARENA = 1,
		ENTITY = 2,
		VALUE = 3,
		DYNAMIC = 4,
		DOOR = 5,
	}
	const varType = script.u8();
	if (varType === 3)
		return script.f32();
	const index = script.u8();
	assert(index >= 0 && index <= 3);
	switch (varType) {
		case VarType.LEVEL:
			return entity.arena.level.variables[index];
		case VarType.ARENA:
			return entity.arena.arenaVariables[index];
		case VarType.ENTITY:
			return entity.variables[index];
		default:
			assert(false, "todo var type " + varType);
	}
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
		case 0x01: { // set script resume point
			log("set resume point");
			entity.scriptOffset = offset + 1;
			break;
		}
		case 0x0B: // set min order range
			entity.minOrderRange = script.u8();
			log("set min order range", entity.minOrderRange);
			break;
		case 0x09: { // clear script
			log("end script");
			entity.scriptOffset = 0;
			// todo clear stack
			return false;
		}
		case 0x10: { // set health
			entity.health = script.u16();
			if (entity.health === 0) {
				log("destroyed");
				// todo destroy
				return false;
			} else {
				log("set health", entity.health);
			}
			break;
		}
		case 0x3B: { // set animation
			let animOffset = script.u32();
			const currentOffset = script.offset;
			script.offset = animOffset;
			const num = script.u32();
			if (num === 0) {
				const name = script.str(8);
				log("set animation", name);
			} else {
				log("set animation", animOffset);
			}
			script.offset = currentOffset;
			break;
		}
		case 0x40: { // delay
			const delay = getVar(entity, script);
			log("delay", delay);
			entity.scriptDelay = delay;
			entity.scriptOffsetAfterDelay = script.offset;
			return false;
		}
		case 0x49: { // set max order range
			entity.maxOrderRange = script.u8();
			log("set max order range", entity.maxOrderRange);
			break;
		}
		case 0x74: {
			const flags = script.u32();
			log("set flags", flags);
			entity.flags |= flags;
			break;
		}
		case 0x95: { // spawn door
			const position = script.vec3();
			const angle = script.f32();
			const id = script.i32();
			const name = script.pstr();
			const targetArenaName = script.pstr();
			const scriptOffset = script.u32();
			const targetArena = arena.level.findArena(targetArenaName);
			log("spawn door", position, angle, id, name, targetArenaName, scriptOffset);
			const door = new DoorEntity(name, id, arena, position, angle, targetArena);
			arena.spawnEntity(door, scriptOffset);
			break;
		}
		case 0x96: { // set door animations
			assert(entity instanceof DoorEntity);
			const openAnimOffset = script.u32();
			const closeAnimOffset = script.u32();
			log("set door animations");
			// todo set these for real
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
			const flags = script.u32();
			entity.doorFlags = (entity.doorFlags & 0xF) | flags;
			log("set door flags", flags, DoorFlags[flags]);
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
			log("finished script");
			return false;
		default:
			log("unknown opcode!");
			entity.scriptOffset = 0;
			return false;
	}
	return true;
}
