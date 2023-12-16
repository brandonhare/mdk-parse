import * as fs from "fs";
import * as path from "path";
import * as cmi from "./cmi";

function parse_dti(buffer: DataView): cmi.BspEntity[] {
	const data = new cmi.Reader(buffer);
	const filename = data.str(12);
	data.skip(4); // filesize
	const offset0 = data.u32();
	const offset1 = data.u32();
	const entitiesOffset = data.u32();
	const offset3 = data.u32();
	const offset4 = data.u32();

	data.offset = entitiesOffset;
	const numArenas = data.u32();

	const result: cmi.BspEntity[] = [];
	for (let i = 0; i < numArenas; ++i) {
		data.offset = entitiesOffset + 4 + 16 * i;
		const arenaName = data.str(8);
		const arenaOffset = data.u32();

		data.offset = arenaOffset;
		const numEntities = data.u32();
		for (let j = 0; j < numEntities; ++j) {
			const entityType = data.i32();
			if (entityType !== 2 && entityType !== 4) {
				data.skip(4 + 4 + 12 + 12);
				continue;
			}
			const id = data.i32();
			const value = data.i32();
			const position = data.vec3();
			const name = data.str(12);

			result.push({ arenaName, name, id, position, value });
		}
	}

	return result;
}

for (let level_num = 3; level_num <= 8; ++level_num) {
	const dti_buffer = fs.readFileSync(path.resolve(`../assets/TRAVERSE/LEVEL${level_num}/LEVEL${level_num}.DTI`));

	const dtiEntities = parse_dti(new DataView(dti_buffer.buffer, 4));

	const cmi_buffer = fs.readFileSync(path.resolve(`../assets/TRAVERSE/LEVEL${level_num}/LEVEL${level_num}.CMI`));

	const level = cmi.go(new DataView(cmi_buffer.buffer, 4), dtiEntities);

	const outputPath = "output/level" + level_num;
	for (const arena of level.arenas.values()) {
		const arenaPath = outputPath + '/' + arena.name;
		if (!fs.existsSync(arenaPath))
			fs.mkdirSync(arenaPath, { recursive: true });


		const counts = new Map<string, number>();
		for (const entity of arena.entities) {
			const name = entity.name + '_' + entity.id;
			counts.set(name, (counts.get(name) ?? 0) + 1);
		}

		const seenCount = new Map<string, number>();
		for (const entity of [arena, ...arena.entities]) {
			let name = (entity === arena) ? entity.name : entity.name + '_' + entity.id;

			if (counts.get(name)! > 1) {
				const count = seenCount.get(name) ?? 1;
				seenCount.set(name, count + 1);
				name += ` (${count})`;
			}

			fs.writeFile(`${arenaPath}/${name}.txt`, entity.log.join('\n'), () => {});
		}
	}

}
