import { readFileSync } from "fs";
import * as cmi from "./cmi";

for (let level_num = 3; level_num <= 3; ++level_num) {
	const level_data = readFileSync(`../assets/TRAVERSE/LEVEL${level_num}/LEVEL${level_num}.CMI`);
	cmi.go(new DataView(level_data.buffer, 4));
}
