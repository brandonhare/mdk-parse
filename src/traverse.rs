use std::collections::HashMap;
use std::fmt::Write;

use crate::data_formats::{
	image_formats::ColourMap, Mesh, SoundInfo, Texture, TextureHolder, TextureResult,
};
use crate::file_formats::{
	mti::{Material, Mti, Pen},
	Bni, Cmi, Dti, Mto, Sni,
};
use crate::{output_writer::OutputWriter, reader::Reader};

fn filter_textures<'a>(
	frames: &[Texture], palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<(&'a str, &'a str)>,
) -> usize {
	if arenas.len() == 1 {
		return 1;
	}
	let colour_map = ColourMap::from_frames(frames);
	filter_colours(colour_map, palettes, arenas)
}
fn filter_colours<'a>(
	colour_map: ColourMap, palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<(&'a str, &'a str)>,
) -> usize {
	if arenas.len() == 1 {
		return 1;
	}

	for arena in arenas.iter_mut() {
		debug_assert_eq!(arena.0, arena.1);
		arena.1 = arena.0;
	}
	arenas.sort_unstable_by(|arena1, arena2| {
		let c1 = arena1.0.as_bytes()[0] == b'C';
		let c2 = arena2.0.as_bytes()[0] == b'C';
		c1.cmp(&c2).then(arena1.0.cmp(arena2.0))
	});
	arenas.dedup();

	let mut num_unique = arenas.len();
	for i in 1..arenas.len() {
		let arena1 = arenas[i].0;
		let pal1 = &palettes[arena1];
		for (arena2_src, arena2_dest) in &arenas[0..i] {
			if arena2_src != arena2_dest {
				continue;
			}
			let pal2 = &palettes[arena2_src];
			if colour_map.compare(pal1, pal2) {
				arenas[i].1 = *arena2_src;
				num_unique -= 1;
				break;
			}
		}
	}
	num_unique
}

pub fn parse_traverse(save_sounds: bool, save_textures: bool, save_meshes: bool) {
	// save shared sounds
	if save_sounds {
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
		let mut cmi = Cmi::parse(Reader::new(&cmi));
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
		let mut all_pens = HashMap::<&str, Pen>::new();
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

		if save_sounds {
			all_sounds.extend(sni_o.sounds.iter().map(|(name, sound)| (*name, sound)));
			all_sounds.extend(sni_s.sounds.iter().map(|(name, sound)| (*name, sound)));
		}

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

		{
			let mut palette_output = output.push_dir("Palettes");
			for arena in &mto.arenas {
				for (mesh_name, mesh) in &arena.meshes {
					all_meshes.insert(mesh_name, mesh);
					let entity_arenas = &mut cmi.entities.entry(mesh_name).or_default().arenas;
					if !entity_arenas.contains(&arena.name) {
						entity_arenas.push(arena.name);
						entity_arenas.sort_unstable();
					}
				}
				all_meshes.insert(arena.name, &arena.bsp.mesh);
				let entity_arenas = &mut cmi.entities.entry(arena.name).or_default().arenas;
				if !entity_arenas.contains(&arena.name) {
					entity_arenas.push(arena.name);
					entity_arenas.sort_unstable();
				}

				// don't add arena sounds, do that later so we can organize them in folders

				let num_bytes = dti.num_pal_free_pixels as usize * 3;
				let mut palette = dti.pal.to_vec();
				palette[4 * 16 * 3..4 * 16 * 3 + num_bytes]
					.copy_from_slice(&arena.palette[..num_bytes]);
				if save_textures {
					palette_output.write_palette(arena.name, &palette);
				}
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
		}

		// clone palettes to corridors
		for arena in cmi.arenas.iter() {
			if !palettes.contains_key(arena.name) {
				if let Some(pal) = palettes.get(&arena.name[1..]) {
					palettes.insert(arena.name, pal.clone());
				}
			}
		}

		// register corridor and level meshes in cmi entities
		for &mesh_name in all_meshes.keys() {
			if !palettes.contains_key(mesh_name) {
				continue;
			}
			let entity_arenas = &mut cmi.entities.entry(mesh_name).or_default().arenas;
			entity_arenas.clear();
			entity_arenas.push(mesh_name);
		}

		all_meshes.extend(
			cmi.entities
				.iter()
				.filter_map(|(&name, entity)| entity.mesh.as_ref().map(|mesh| (name, mesh))),
		);

		// save sounds
		if save_sounds {
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
		} else {
			debug_assert!(all_sounds.is_empty());
		}

		let mut used_textures = HashMap::<&str, Vec<(&str, &str)>>::new();

		// save meshes/textures
		{
			let mut output = output.push_dir("Meshes");
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
								.extend(palettes.keys().map(|arena| (*arena, *arena)));
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
							.extend(arenas.iter().map(|arena| (*arena, *arena)));
					} else if !all_pens.contains_key(tex_name) {
						//println!("level {level_index} mesh {name} missing tex {tex_name}"); // todo
					}
				}
			}

			// save mesh textures

			{
				let mut output = output.push_dir("Textures"); // inside mesh folder
				for (&name, arenas) in used_textures.iter_mut() {
					let tex = all_textures[name];
					let num_unique = filter_textures(tex, &palettes, arenas);

					if !save_textures {
						continue;
					}

					if num_unique == 1 {
						Texture::save_animated(
							tex,
							name,
							24,
							&mut output,
							Some(&palettes[arenas[0].0]),
						);
					} else {
						for (arena_src, arena_dest) in arenas.iter() {
							if arena_src == arena_dest {
								temp_filename.clear();
								write!(temp_filename, "{name}_{arena_src}").unwrap();
								Texture::save_animated(
									tex,
									&temp_filename,
									24,
									&mut output,
									Some(&palettes[arena_src]),
								);
							}
						}
					}
				}
			}

			struct TravTextureLookup<'a> {
				translucent_colours: [[u8; 4]; 4],
				pens: &'a HashMap<&'a str, Pen>,
				textures: &'a HashMap<&'a str, &'a [Texture<'a>]>,
				texture_arenas: &'a HashMap<&'a str, Vec<(&'a str, &'a str)>>,
				palette: &'a [u8],
				current_arena: &'a str,
			}
			impl<'a> TextureHolder<'a> for TravTextureLookup<'a> {
				fn lookup(&mut self, name: &str) -> TextureResult<'a> {
					if let Some(arenas) = self.texture_arenas.get(name) {
						let is_unique = arenas[1..].iter().all(|(src, dest)| src != dest);

						let path = if is_unique {
							format!("Textures/{name}.png")
						} else {
							let dest_arena = arenas
								.iter()
								.find(|(src, _)| *src == self.current_arena)
								.unwrap()
								.1;
							format!("Textures/{name}_{dest_arena}.png")
						};
						let tex = self.textures[name];
						let width = tex[0].width;
						let height = tex[0].height;
						assert!(tex[1..]
							.iter()
							.all(|t| t.width == width && t.height == height));
						return TextureResult::SaveRef {
							width,
							height,
							path,
						};
					}
					if let Some(pen) = self.pens.get(name) {
						return TextureResult::Pen(*pen);
					}
					//eprintln!("missing texture {name}");
					TextureResult::None
				}
				fn get_used_colours(&self, name: &str, colours: &mut ColourMap) {
					if let Some(tex) = self.textures.get(name) {
						for frame in *tex {
							colours.extend(frame.pixels.iter());
						}
					} else if let Some(Pen::Colour(n)) = self.pens.get(name) {
						colours.push(*n);
					}
				}
				fn get_palette(&self) -> &[u8] {
					debug_assert!(self.palette.len() == 256 * 3);
					self.palette
				}
				fn get_translucent_colours(&self) -> [[u8; 4]; 4] {
					self.translucent_colours
				}
			}

			let mut textures = TravTextureLookup {
				translucent_colours: dti.translucent_colours,
				pens: &all_pens,
				textures: &all_textures,
				texture_arenas: &used_textures,
				palette: &[],
				current_arena: "",
			};

			// save meshes
			let mut mesh_arenas = Vec::new();
			if save_meshes {
				for (&name, &mesh) in all_meshes.iter() {
					mesh_arenas.clear();

					if let Some(cmi_entity) = cmi.entities.get(name) {
						mesh_arenas.extend(cmi_entity.arenas.iter().map(|arena| (*arena, *arena)));
					}

					if mesh_arenas.is_empty() {
						for matname in &mesh.materials {
							let Some(tex_arenas) = used_textures.get(matname) else {
								continue;
							};
							for (src, dest) in tex_arenas {
								if src == dest {
									mesh_arenas.push((src, src));
								}
							}
						}
					}
					if mesh_arenas.is_empty() {
						// todo
						// eprintln!("level {level_index} mesh {name} cant find arenas");
						mesh_arenas.extend(mto.arenas.iter().map(|arena| (arena.name, arena.name)));
					}

					mesh_arenas.sort_unstable();
					mesh_arenas.dedup();

					let used_colours = mesh.get_used_colours(&textures);
					let num_unique_arenas =
						filter_colours(used_colours, &palettes, &mut mesh_arenas);

					if num_unique_arenas == 1 {
						textures.current_arena = mesh_arenas[0].0;
						textures.palette = &palettes[textures.current_arena];
						mesh.save_textured_as(name, &mut output, &mut textures);
					} else {
						// save multiple meshes with the different textures
						for (src, dest) in &mesh_arenas {
							if src != dest {
								continue;
							}
							textures.current_arena = src;
							textures.palette = &palettes[textures.current_arena];
							temp_filename.clear();
							write!(temp_filename, "{name}_{src}").unwrap();
							mesh.save_textured_as(&temp_filename, &mut output, &mut textures);
						}
					}
				}
			}
		}

		// save unused/other textures
		if save_textures {
			let mut temp_arenas: Vec<(&str, &str)> = Vec::new();
			let mut tex_output = output.push_dir("Other Textures");
			let mut anim_output = output.push_dir("Animations");

			dti.skybox.save_as("Sky", &mut tex_output, Some(dti.pal));

			for (&name, tex) in all_textures.iter() {
				if used_textures.contains_key(name) {
					continue;
				}
				temp_arenas.clear();
				temp_arenas.extend(palettes.keys().map(|arena| (*arena, *arena)));
				temp_arenas.sort_unstable();
				let num_unique = filter_textures(tex, &palettes, &mut temp_arenas);

				let output = if tex.len() == 1 {
					&mut tex_output
				} else {
					&mut anim_output
				};
				if num_unique == 1 {
					Texture::save_animated(
						tex,
						name,
						24,
						output,
						Some(&palettes[temp_arenas[0].0]),
					);
				} else {
					for (arena_src, arena_dest) in temp_arenas.iter() {
						if arena_src == arena_dest {
							temp_filename.clear();
							write!(temp_filename, "{name}_{arena_src}").unwrap();
							Texture::save_animated(
								tex,
								&temp_filename,
								24,
								output,
								Some(&palettes[arena_src]),
							);
						}
					}
				}
			}
		}
	}
}
