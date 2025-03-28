//! Exports TRAVERSE assets (everything in-game)
use std::collections::HashMap;

use crate::data_formats::mesh::ColourMap;
use crate::data_formats::{Mesh, Pen, Texture, TextureHolder, TextureResult, Wav};
use crate::file_formats::{
	Bni, Cmi, Dti, Fti, Mto, Sni,
	mti::{Material, Mti},
};
use crate::{OutputWriter, Reader};

pub fn parse_traverse(save_sounds: bool, save_textures: bool, save_meshes: bool) {
	// the base palette is loaded from the font file for some reason!
	// this is required since a couple of levels have invalid colours in their versions
	let sys_pal = {
		let fti = std::fs::read("assets/MISC/mdkfont.fti").unwrap();
		let fti = Fti::parse(Reader::new(&fti));
		fti.palette.to_owned()
	};

	let trav_bni = std::fs::read("assets/TRAVERSE/TRAVSPRT.BNI").unwrap();
	let trav_bni = Bni::parse(Reader::new(&trav_bni));

	let mut all_palettes: HashMap<String, Vec<u8>> = Default::default();

	for level_index in 3usize..=8 {
		println!("  Parsing traverse level {level_index}...");
		let mut output = OutputWriter::new(format!("assets/TRAVERSE/LEVEL{level_index}"), true);

		let read_file = |ext| {
			std::fs::read(format!(
				"assets/TRAVERSE/LEVEL{level_index}/LEVEL{level_index}{ext}"
			))
			.unwrap()
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

		let mut all_sounds = HashMap::<&str, &Wav>::new();
		let mut all_meshes = HashMap::<&str, &Mesh>::new();
		let mut all_pens = HashMap::<&str, Pen>::new();
		let mut all_textures = HashMap::<&str, &[Texture]>::new();

		let mut palettes = HashMap::<String, Vec<u8>>::new();

		if save_sounds {
			all_sounds.extend(sni_o.sounds.iter().map(|(name, sound)| (*name, sound)));
			all_sounds.extend(sni_s.sounds.iter().map(|(name, sound)| (*name, sound)));
		}

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

		// add mto assets/arenas/palettes
		{
			let mut palette_output = output.push_dir("Palettes");
			for arena in &mto.arenas {
				let cmi_arena = cmi
					.arenas
					.iter_mut()
					.find(|cmi_arena| cmi_arena.name == arena.name)
					.unwrap();
				for (mesh_name, mesh) in &arena.meshes {
					let dup = all_meshes.insert(mesh_name, mesh);
					assert!(dup.is_none_or(|m| mesh == m), "duplicate mesh {mesh_name}");
					// register mesh as belonging to this arena
					let entity_arenas = &mut cmi.entities.entry(mesh_name).or_default().arenas;
					if !entity_arenas.contains(&arena.name) {
						entity_arenas.push(arena.name);
						entity_arenas.sort_unstable();
						assert!(!cmi_arena.entities.contains(mesh_name));
						cmi_arena.entities.push(mesh_name);
					}
				}
				let dup = all_meshes.insert(arena.name, &arena.bsp.mesh);
				assert!(dup.is_none(), "duplicate arena mesh {}", arena.name);
				assert!(
					cmi.entities
						.entry(arena.name)
						.or_default()
						.arenas
						.contains(&arena.name),
					"cmi arena {} missing in entities!",
					arena.name
				);

				// don't add arena sounds, do that later so we can organize them in folders

				let num_free_palette_bytes = dti.num_pal_free_pixels as usize * 3;
				let mut palette = dti.pal.to_vec();
				palette[..192].copy_from_slice(&sys_pal);
				palette[4 * 16 * 3..4 * 16 * 3 + num_free_palette_bytes]
					.copy_from_slice(&arena.palette[..num_free_palette_bytes]);
				if save_textures {
					palette_output.write_palette(arena.name, &palette);
				}
				palettes.insert(arena.name.to_owned(), palette);

				// add materials
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

		// add corridor bsps and assign to their parent arena so they get correct palettes
		for (corridor_name, bsp) in &sni_o.bsps {
			assert_eq!(corridor_name.as_bytes()[0], b'C');
			let arena_name = &corridor_name[1..];

			let entity = cmi.entities.entry(corridor_name).or_default();
			if entity.arenas.contains(corridor_name) {
				// referenced in cmi, add a new palette
				let pal = &palettes[arena_name];
				palettes.insert(corridor_name.to_string(), pal.clone());
			} else {
				// not referenced anywhere, add to parent arena
				let dup = all_meshes.insert(*corridor_name, &bsp.mesh);
				assert!(dup.is_none(), "duplicate corridor mesh {corridor_name}");
				entity.arenas.push(arena_name);
				cmi.arenas
					.iter_mut()
					.find(|arena| arena.name == arena_name)
					.unwrap()
					.entities
					.push(corridor_name);
			}
		}
		assert!(sni_s.bsps.is_empty(), "unexpected bsps in sni_s");

		// add cmi meshes (including arena meshes added above)
		for (name, entity) in &cmi.entities {
			let Some(ref mesh) = entity.mesh else {
				continue;
			};
			let dup = all_meshes.insert(name, mesh);
			assert!(dup.is_none(), "duplicate cmi mesh {name}");
		}

		// save level info
		dti.save_info_as("Level Info", &mut output);

		// save scripts
		cmi.save_scripts(&mut output.push_dir("Scripts"));

		// save sounds
		if save_sounds {
			let output = output.push_dir("Sounds");
			for arena in &mto.arenas {
				let song = cmi
					.arenas
					.iter()
					.find(|a| a.name == arena.name)
					.and_then(|a| all_sounds.get(a.song).map(|song| (a.song, *song)));

				if song.is_none() && arena.sounds.is_empty() {
					continue;
				}
				let mut arena_output = output.push_dir(arena.name);
				if let Some((song_name, song)) = song {
					song.save_as(song_name, &mut arena_output);
				}

				for (name, sound) in &arena.sounds {
					sound.save_as(name, &mut arena_output);
				}
			}

			let mut shared_output = output.push_dir("Shared");
			for (name, sound) in all_sounds.iter() {
				sound.save_as(name, &mut shared_output);
			}
		}

		let mut used_textures = HashMap::<&str, Vec<(&str, &str)>>::new();

		// save meshes/textures
		{
			let mut output = output.push_dir("Meshes");
			let mut anim_output = output.push_dir("Animations");

			// most of the following nonsense is just deduplicating textures used by meshes

			// gather materials
			for (&name, mesh) in all_meshes.iter() {
				let mesh_arenas = cmi
					.entities
					.get(name)
					.map(|entity| entity.arenas.as_slice())
					.filter(|a| !a.is_empty());

				if mesh_arenas.is_none() {
					//println!("level {level_index} mesh {name} not in any arenas"); // todo
				}

				for &tex_name in mesh.materials.iter() {
					if all_textures.contains_key(tex_name) {
						let used = used_textures.entry(tex_name).or_default();
						if let Some(mesh_arenas) = mesh_arenas {
							used.extend(mesh_arenas.iter().map(|&arena| (arena, arena)));
						} else {
							used.extend(
								palettes
									.keys()
									.map(String::as_str)
									.map(|arena| (arena, arena)),
							);
						}
					} else if !all_pens.contains_key(tex_name) {
						// the ramp to the boss room in LEVEL3 (really level2) is missing a texture
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
						//println!("level {level_index} splitting mesh texture {name}");
						for &(arena_src, arena_dest) in arenas.iter() {
							if arena_src == arena_dest {
								Texture::save_animated(
									tex,
									&format!("{name}_{arena_src}"),
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
						assert!(
							tex[1..]
								.iter()
								.all(|t| t.width == width && t.height == height)
						);
						let masked = tex
							.iter()
							.any(|frames| frames.pixels.iter().any(|p| *p == 0));
						return TextureResult::SaveRef {
							width,
							height,
							path,
							masked,
						};
					}
					if let Some(pen) = self.pens.get(name) {
						return TextureResult::Pen(*pen);
					}

					// missing
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
						// shouldn't happen, textures are already split by arena
						// (maybe if the mesh is only flat coloured and different between arenas)
						//println!("level {level_index} mesh {name} cant find arenas");
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
						//println!("level {level_index} splitting mesh {name}");
						for (src, dest) in &mesh_arenas {
							if src != dest {
								continue;
							}
							textures.current_arena = src;
							textures.palette = &palettes[textures.current_arena];
							mesh.save_textured_as(
								&format!("{name}_{src}"),
								&mut output,
								&mut textures,
							);
						}
					}
				}

				// save 3d animations
				// todo save animations inside meshes

				// save mto animations
				for arena in &mto.arenas {
					for (name, anim) in &arena.animations {
						anim.save_as(name, &mut anim_output);
					}
				}
				// save unnamed cmi animations
				for (mesh_name, mesh) in cmi.entities.iter() {
					for anim_offset in &mesh.animations {
						let anim = &cmi.animations[anim_offset];
						anim.save_as(&format!("{mesh_name}_{anim_offset:08X}"), &mut anim_output);
					}
				}
			} // end save_meshes
		} // end save_meshes/textures

		// save unused/other textures
		if save_textures {
			let mut temp_arenas: Vec<(&str, &str)> = Vec::new();
			let mut tex_output = output.push_dir("Textures");
			let mut anim_output = output.push_dir("Animations");

			mti.save_report(&mut tex_output);

			dti.skybox.save_as("Sky", &mut tex_output, Some(dti.pal));
			if let Some(sky) = &dti.reflected_skybox {
				sky.save_as("Reflection", &mut tex_output, Some(dti.pal));
			}

			for (&name, tex) in all_textures.iter() {
				if used_textures.contains_key(name) {
					continue;
				}
				// find all arenas that use this texture so we can split its palettes
				temp_arenas.clear();

				// find source mtos and use their palette
				'outer: for arena in &mto.arenas {
					for (mti_name, _) in &arena.mti.materials {
						if name == *mti_name {
							temp_arenas.push((arena.name, arena.name));
							break 'outer;
						}
					}
				}

				if temp_arenas.is_empty() {
					// try all palettes
					//println!("level {level_index} texture {name} can't find arena");
					temp_arenas.extend(
						palettes
							.keys()
							.map(|arena| (arena.as_str(), arena.as_str())),
					);
					temp_arenas.sort_unstable();
					filter_textures(tex, &palettes, &mut temp_arenas);
					temp_arenas.retain(|(a, b)| a == b);
				}

				let output = if tex.len() == 1 {
					&mut tex_output
				} else {
					&mut anim_output
				};

				let unique_pal = temp_arenas.len() == 1;
				let fps = 24;
				if unique_pal {
					Texture::save_animated(
						tex,
						name,
						fps,
						output,
						Some(&palettes[temp_arenas[0].0]),
					);
				} else {
					// save all copies
					//println!("level {level_index} splitting other texture {name}");
					for &(arena, _) in &temp_arenas {
						Texture::save_animated(
							tex,
							&format!("{name}_{arena}"),
							fps,
							output,
							Some(&palettes[arena]),
						);
					}
				}
			}
		}

		all_palettes.extend(palettes);
	}

	// finished exporting each level, now export stuff from the shared files

	assert!(trav_bni.strings.is_empty());
	assert!(trav_bni.sounds.is_empty());
	assert!(trav_bni.coloured_textures.is_empty());
	assert!(trav_bni.palettes.is_empty());

	let shared_output = OutputWriter::new("assets/TRAVERSE/Shared/", true);
	if save_sounds {
		let trav_sni = std::fs::read("assets/TRAVERSE/TRAVERSE.SNI").unwrap();
		let trav_sni = Sni::parse(Reader::new(&trav_sni));
		let mut output = shared_output.push_dir("Sounds");
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

	if save_textures {
		let mut tex_output = shared_output.push_dir("Textures");
		let mut anim_output = shared_output.push_dir("Animations");

		for (name, frames) in trav_bni
			.textures
			.iter()
			.map(|(name, tex)| (*name, std::slice::from_ref(tex)))
			.chain(
				trav_bni
					.animations_2d
					.iter()
					.map(|(name, frames)| (*name, frames.as_slice())),
			) {
			if name == "PICKUPS" {
				// all pickups except the last (gunter snack / bones) use the same colours,
				// so we can just use that last palette to make sure they all export correctly.
				let pal = all_palettes["GUNT_10"].as_slice();
				for (i, tex) in frames.iter().enumerate() {
					// save as separate images instead of an animation
					tex.save_as(&format!("PICKUPS_{i}"), &mut tex_output, Some(pal));
				}
				continue;
			}

			if frames.len() == 1 {
				frames[0].save_as(name, &mut tex_output, Some(&sys_pal));
			} else {
				Texture::save_animated(frames, name, 24, &mut anim_output, Some(&sys_pal))
			};
		}
	}

	if save_meshes {
		// save animations
		// todo move into individual level meshes
		let mut anim_output = shared_output.push_dir("Meshes/Animations");
		for (name, anim) in &trav_bni.animations_3d {
			anim.save_as(name, &mut anim_output);
		}
	}
}

/// Determines how many unique palettes a texture uses
fn filter_textures<'a>(
	frames: &[Texture], palettes: &HashMap<String, Vec<u8>>, arenas: &mut Vec<(&'a str, &'a str)>,
) -> usize {
	if arenas.len() == 1 {
		return 1;
	}
	let colour_map = ColourMap::from_frames(frames);
	filter_colours(colour_map, palettes, arenas)
}
fn filter_colours<'a>(
	colour_map: ColourMap, palettes: &HashMap<String, Vec<u8>>,
	arenas: &mut Vec<(&'a str, &'a str)>,
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
			let pal2 = &palettes[*arena2_src];
			if colour_map.compare(pal1, pal2) {
				arenas[i].1 = *arena2_src;
				num_unique -= 1;
				break;
			}
		}
	}
	num_unique
}
