use std::collections::HashMap;
use std::fmt::Write;

use crate::data_formats::{Mesh, SoundInfo, Texture};
use crate::file_formats::{mti::Material, Bni, Cmi, Dti, Mti, Mto, Sni};
use crate::{output_writer::OutputWriter, reader::Reader};

struct ColourMap([u64; 4]);
impl ColourMap {
	fn from_tex(tex: &Texture) -> Self {
		Self::new(&tex.pixels)
	}
	fn from_frames(frames: &[Texture]) -> Self {
		let mut result = Self([0; 4]);
		for frame in frames {
			result.extend(&frame.pixels);
		}
		result
	}
	fn new(pixels: &[u8]) -> Self {
		let mut result = Self([0; 4]);
		result.extend(pixels);
		result
	}
	fn extend(&mut self, pixels: &[u8]) {
		for &p in pixels {
			self.0[(p >> 6) as usize] |= 1 << (p & 63);
		}
	}

	fn compare(&self, pal1: &[u8], pal2: &[u8]) -> bool {
		debug_assert_eq!(pal1.len(), 256 * 3);
		debug_assert_eq!(pal2.len(), 256 * 3);

		for (&mask, (block1, block2)) in self
			.0
			.iter()
			.zip(pal1.chunks_exact(64 * 3).zip(pal2.chunks_exact(64 * 3)))
		{
			for i in 0..64 {
				if mask & (1 << i) != 0 && block1[i * 3..(i + 1) * 3] != block2[i * 3..(i + 1) * 3]
				{
					return false;
				}
			}
		}
		true
	}
}

fn filter_textures(frames: &[Texture], palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<&str>) {
	if arenas.len() == 1 {
		return;
	}
	arenas.sort_unstable_by(|arena1, arena2| {
		let c1 = arena1.as_bytes()[0] == b'C';
		let c2 = arena2.as_bytes()[0] == b'C';
		c1.cmp(&c2).then(arena1.cmp(arena2))
	});
	arenas.dedup();
	let colour_map = ColourMap::from_frames(frames);
	for i in 1..arenas.len() {
		let arena1 = arenas[i];
		let pal1 = &palettes[arena1];
		for &arena2 in &arenas[0..i] {
			if arena2.is_empty() {
				continue;
			}
			let pal2 = &palettes[arena2];
			if colour_map.compare(pal1, pal2) {
				arenas[i] = "";
				break;
			}
		}
	}
	arenas.retain(|name| !name.is_empty());
}
fn filter_texture(tex: &Texture, palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<&str>) {
	filter_textures(std::slice::from_ref(tex), palettes, arenas)
}

pub fn parse_traverse() {
	// save shared sounds
	{
		let trav_sni = std::fs::read("assets/TRAVERSE/TRAVERSE.SNI").unwrap();
		let trav_sni = Sni::parse(Reader::new(&trav_sni));
		let mut output = OutputWriter::new("assets/TRAVERSE/Sounds", true);
		for (name, sound) in &trav_sni.sounds {
			sound.save_as(name, &mut output);
		}
		assert!(
			trav_sni.anims.is_empty(),
			"traverse.sni contained unexpected animations"
		);
		assert!(
			trav_sni.bsps.is_empty(),
			"traverse.sni contained unexpected bsps"
		);
	}

	let trav_bni = std::fs::read("assets/TRAVERSE/TRAVSPRT.BNI").unwrap();
	let trav_bni = Bni::parse(Reader::new(&trav_bni));

	let mut temp_filename = String::new();

	for level_index in 3..=8 {
		temp_filename.clear();
		write!(temp_filename, "assets/TRAVERSE/LEVEL{level_index}").unwrap();
		let output = OutputWriter::new(&temp_filename, true);

		let mut read_file = |ext| {
			temp_filename.clear();
			write!(
				temp_filename,
				"assets/TRAVERSE/LEVEL{level_index}/LEVEL{level_index}{ext}"
			)
			.unwrap();
			std::fs::read(&temp_filename).unwrap()
		};

		// load files
		let cmi = read_file(".CMI");
		let cmi = Cmi::parse(Reader::new(&cmi));
		let dti = read_file(".DTI");
		let dti = Dti::parse(Reader::new(&dti));
		let mto = read_file("O.MTO");
		let mto = Mto::parse(Reader::new(&mto));
		let mti = read_file("S.MTI");
		let mti = Mti::parse(Reader::new(&mti));
		let sni_o = read_file("O.SNI");
		let sni_o = Sni::parse(Reader::new(&sni_o));
		let sni_s = read_file("S.SNI");
		let sni_s = Sni::parse(Reader::new(&sni_s));

		// gather assets

		let mut all_sounds = HashMap::<&str, &SoundInfo>::new();
		let mut all_meshes = HashMap::<&str, &Mesh>::new();
		let mut all_pens = HashMap::<&str, i32>::new();
		let mut all_textures = HashMap::<&str, &[Texture]>::new();

		let mut palettes = HashMap::<&str, Vec<u8>>::new();

		all_meshes.extend(trav_bni.meshes.iter().map(|(name, mesh)| (*name, mesh)));
		all_textures.extend(
			trav_bni
				.textures
				.iter()
				.map(|(name, tex)| (*name, std::slice::from_ref(tex))),
		);
		all_textures.extend(
			trav_bni
				.animations_2d
				.iter()
				.map(|(name, frames)| (*name, frames.as_slice())),
		);

		all_meshes.extend(
			cmi.entities
				.iter()
				.filter_map(|(&name, entity)| entity.mesh.as_ref().map(|mesh| (name, mesh))),
		);

		all_sounds.extend(sni_o.sounds.iter().map(|(name, sound)| (*name, sound)));
		all_sounds.extend(sni_s.sounds.iter().map(|(name, sound)| (*name, sound)));
		all_meshes.extend(sni_o.bsps.iter().map(|(name, bsp)| (*name, &bsp.mesh)));
		all_meshes.extend(sni_s.bsps.iter().map(|(name, bsp)| (*name, &bsp.mesh)));
		all_textures.extend(
			sni_o
				.anims
				.iter()
				.map(|(name, frames)| (*name, frames.as_slice())),
		);
		all_textures.extend(
			sni_s
				.anims
				.iter()
				.map(|(name, frames)| (*name, frames.as_slice())),
		);

		for (name, mat) in mti.materials.iter() {
			match mat {
				Material::Pen(pen) => {
					all_pens.insert(name, *pen);
				}
				Material::Texture(tex, _) => {
					all_textures.insert(*name, std::slice::from_ref(tex));
				}
				Material::AnimatedTexture(frames, _) => {
					all_textures.insert(name, frames.as_slice());
				}
			}
		}

		for arena in &mto.arenas {
			all_meshes.extend(arena.meshes.iter().map(|(name, mesh)| (*name, mesh)));
			all_meshes.insert(arena.name, &arena.bsp.mesh);
			// don't add arena sounds, do that later so we can organize them in folders

			let num_bytes = dti.num_pal_free_pixels as usize * 3;
			let mut palette = dti.pal.to_vec();
			palette[4 * 16 * 3..4 * 16 * 3 + num_bytes]
				.copy_from_slice(&arena.palette[..num_bytes]);
			palettes.insert(arena.name, palette);

			for (name, mat) in arena.mti.materials.iter() {
				match mat {
					Material::Pen(pen) => {
						all_pens.insert(name, *pen);
					}
					Material::Texture(tex, _) => {
						all_textures.insert(*name, std::slice::from_ref(tex));
					}
					Material::AnimatedTexture(frames, _) => {
						all_textures.insert(name, frames.as_slice());
					}
				}
			}
		}

		// clone palettes to corridors
		for arena in cmi.arenas.iter() {
			if !palettes.contains_key(arena.name) {
				if let Some(pal) = palettes.get(&arena.name[1..]) {
					palettes.insert(arena.name, pal.clone());
				}
			}
		}

		// save sounds
		{
			let mut output = output.push_dir("Sounds");
			for arena in &mto.arenas {
				let song = cmi
					.arenas
					.iter()
					.find(|a| a.name == arena.name)
					.and_then(|a| all_sounds.get(a.song).map(|song| (a.song, *song)));

				if song.is_none() && arena.sounds.is_empty() {
					continue;
				}
				let mut output = output.push_dir(arena.name);
				if let Some((song_name, song)) = song {
					song.save_as(song_name, &mut output);
				}

				for (name, sound) in &arena.sounds {
					sound.save_as(name, &mut output);
				}
			}

			for (name, sound) in all_sounds.iter() {
				sound.save_as(name, &mut output);
			}
		}

		let mut used_textures = HashMap::<&str, Vec<&str>>::new();

		// save meshes
		{
			let output = output.push_dir("Meshes");
			for (&name, mesh) in all_meshes.iter() {
				let Some(arenas) = cmi
					.entities
					.get(name)
					.map(|entity| &entity.arenas)
					.filter(|a| !a.is_empty())
				else {
					//println!("level {level_index} mesh {name} not in any arenas"); // todo

					for &tex_name in mesh.materials.iter() {
						if all_textures.contains_key(tex_name) {
							used_textures
								.entry(tex_name)
								.or_default()
								.extend(palettes.keys());
						} else if !all_pens.contains_key(tex_name) {
							//println!("level {level_index} mesh {name} missing tex {tex_name}"); // todo
						}
					}

					continue;
				};

				for &tex_name in mesh.materials.iter() {
					if all_textures.contains_key(tex_name) {
						used_textures
							.entry(tex_name)
							.or_default()
							.extend(arenas.iter());
					} else if !all_pens.contains_key(tex_name) {
						//println!("level {level_index} mesh {name} missing tex {tex_name}"); // todo
					}
				}
			}

			// save textures

			let mut output = output.push_dir("Textures"); // inside mesh folder
			for (&name, arenas) in used_textures.iter_mut() {
				let tex = all_textures[name];
				filter_textures(tex, &palettes, arenas);

				if arenas.len() == 1 {
					Texture::save_animated(tex, name, 24, &mut output, Some(&palettes[arenas[0]]));
				} else {
					let mut output = output.push_dir(name);
					for arena in arenas.iter() {
						Texture::save_animated(tex, arena, 24, &mut output, Some(&palettes[arena]));
					}
				}
			}
		}

		let mut temp_arenas: Vec<&str> = Vec::new();
		// save unused/other textures
		{
			let mut tex_output = output.push_dir("Other Textures");
			let mut anim_output = output.push_dir("Animations");

			dti.skybox.save_as("Sky", &mut tex_output, Some(dti.pal));

			for (&name, tex) in all_textures.iter() {
				if used_textures.contains_key(name) {
					continue;
				}
				temp_arenas.clear();
				temp_arenas.extend(palettes.keys());
				temp_arenas.sort_unstable();
				filter_textures(tex, &palettes, &mut temp_arenas);

				let output = if tex.len() == 1 {
					&mut tex_output
				} else {
					&mut anim_output
				};
				if temp_arenas.len() == 1 {
					Texture::save_animated(tex, name, 24, output, Some(&palettes[temp_arenas[0]]));
				} else {
					let mut output = output.push_dir(name);
					for arena in temp_arenas.iter() {
						Texture::save_animated(tex, arena, 24, &mut output, Some(&palettes[arena]));
					}
				}
			}
		}
	}
}
