use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write;

use crate::data_formats::Animation;
use crate::data_formats::{
	image_formats::ColourMap, Materials, Mesh, ResolvedMaterial, SoundInfo, Texture,
};
use crate::file_formats::{
	mti::{Material, Mti, Pen},
	Bni, Cmi, Dti, Fti, Mto, Sni,
};
use crate::{output_writer::OutputWriter, reader::Reader};

fn filter_textures<'a>(
	frames: &[Texture], palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<(&'a str, &'a str)>,
	reduce_result: bool,
) -> usize {
	if arenas.len() == 1 {
		return 1;
	}
	let colour_map = ColourMap::from_frames(frames);
	filter_colours(colour_map, palettes, arenas, reduce_result)
}
fn filter_colours<'a>(
	colour_map: ColourMap, palettes: &HashMap<&str, Vec<u8>>, arenas: &mut Vec<(&'a str, &'a str)>,
	reduce_result: bool,
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

	if reduce_result {
		arenas.retain(|&(src, dest)| src == dest);
	}

	num_unique
}

pub fn parse_traverse(save_sounds: bool, save_textures: bool, save_meshes: bool, save_anims: bool) {
	let sys_pal = {
		let fti = std::fs::read("assets/MISC/mdkfont.fti").unwrap();
		let fti = Fti::parse(Reader::new(&fti));
		fti.palette.to_owned()
	};

	let trav_bni = std::fs::read("assets/TRAVERSE/TRAVSPRT.BNI").unwrap();
	let trav_bni = Bni::parse(Reader::new(&trav_bni));

	let mut shared_palette: Option<Vec<u8>> = None;
	let mut offset_names = HashMap::<u32, String>::new();
	let mut temp_filename = String::new();

	for level_index in 3usize..=8 {
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
		let mut all_anims = Vec::<(&str, &Animation)>::new();

		let mut palettes = HashMap::<&str, Vec<u8>>::new();

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
				for (mesh_name, mesh) in &arena.meshes {
					all_meshes.insert(mesh_name, mesh);
					// register mesh as belonging to this arena
					let entity_arenas = &mut cmi.entities.entry(mesh_name).or_default().arenas;
					if !entity_arenas.contains(&arena.name) {
						entity_arenas.push(arena.name);
						entity_arenas.sort_unstable();
					}
				}
				all_meshes.insert(arena.name, &arena.bsp.mesh);
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

				// add palette
				let num_free_palette_bytes = dti.num_pal_free_pixels as usize * 3;
				let mut palette = dti.pal.to_vec();
				palette[..192].copy_from_slice(&sys_pal);
				palette[4 * 16 * 3..4 * 16 * 3 + num_free_palette_bytes]
					.copy_from_slice(&arena.palette[..num_free_palette_bytes]);
				if save_textures {
					palette_output.write_palette(arena.name, &palette);
				}
				palettes.insert(arena.name, palette);

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

				if save_anims {
					all_anims.extend(arena.animations.iter().map(|(name, anim)| (*name, anim)));
				}
			}
		}

		// add corridor bsps and assign to their parent arena so they get correct palettes
		for (corridor_name, bsp) in &sni_o.bsps {
			assert_eq!(corridor_name.as_bytes()[0], b'C');
			let arena_name = &corridor_name[1..];

			let entity = cmi.entities.entry(corridor_name).or_default();
			if entity.arenas == [*corridor_name] {
				// referenced in cmi, add a new palette
				let pal = palettes.get(arena_name).unwrap();
				palettes.insert(corridor_name, pal.clone());
			} else {
				// not referenced anywhere, add to parent arena
				all_meshes.insert(*corridor_name, &bsp.mesh);
				entity.arenas.push(arena_name);
			}
		}
		assert!(sni_s.bsps.is_empty(), "unexpected bsps in sni_s");

		if save_anims {
			// add bni anims
			all_anims.extend(
				trav_bni
					.animations_3d
					.iter()
					.map(|(name, anim)| (*name, anim)),
			);

			all_anims.sort_unstable_by_key(|(name, _)| *name);
			let named_anim_count = all_anims.len();

			// add cmi numberic anims
			offset_names.clear();
			offset_names.extend(
				cmi.animations
					.keys()
					.map(|&offset| (offset, format!("{offset:06X}"))),
			);
			all_anims.extend(
				offset_names
					.iter()
					.map(|(offset, name)| (name.as_str(), &cmi.animations[offset])),
			);

			all_anims[named_anim_count..].sort_unstable_by_key(|(name, _)| *name);
		}

		all_meshes.extend(
			cmi.entities
				.iter()
				.filter_map(|(&name, entity)| entity.mesh.as_ref().map(|mesh| (name, mesh))),
		);

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

		// save meshes/textures
		if save_textures || save_meshes {
			#[derive(Default)]
			struct TexInfo<'a> {
				colours: ColourMap,
				arenas: Vec<(&'a str, &'a str)>,
				filenames: Vec<String>,
			}
			struct MeshInfo<'a> {
				mesh: &'a Mesh<'a>,
				arenas: Vec<(&'a str, &'a str)>,
			}

			// gather material info
			let mut texture_infos = HashMap::<&str, TexInfo>::new();
			let mut mesh_infos: HashMap<&str, MeshInfo> = all_meshes
				.iter()
				.map(|(&mesh_name, &mesh)| {
					let mut mesh_arenas = Vec::new();
					if let Some(entity) = cmi.entities.get(mesh_name) {
						mesh_arenas.extend(entity.arenas.iter().map(|&arena| (arena, arena)));
					}
					if mesh_arenas.is_empty() {
						// not used anywhere, try every palette
						mesh_arenas.extend(palettes.keys().map(|&arena| (arena, arena)));
					}

					for mat_name in &mesh.materials {
						if all_textures.contains_key(mat_name) {
							texture_infos
								.entry(mat_name)
								.or_default()
								.arenas
								.extend(&mesh_arenas);
						}
					}

					(
						mesh_name,
						MeshInfo {
							arenas: mesh_arenas,
							mesh,
						},
					)
				})
				.collect();

			// dedup texture palettes
			for (&tex_name, tex_info) in &mut texture_infos {
				let frames = all_textures[tex_name];
				tex_info.colours = ColourMap::from_frames(frames);
				let num_unique =
					filter_colours(tex_info.colours, &palettes, &mut tex_info.arenas, false);
				if num_unique == 1 {
					tex_info.filenames = vec![format!("Textures/{tex_name}.png")];
				} else {
					tex_info.filenames = tex_info
						.arenas
						.iter()
						.filter(|(a1, a2)| *a1 == *a2)
						.map(|(arena, _)| format!("Textures/{tex_name}_{arena}.png"))
						.collect();
				}
			}

			let mut mesh_output = output.push_dir("Meshes");
			if save_meshes {
				let mut mesh_materials = Vec::new();
				let mut mesh_anims = Vec::new();
				for (&mesh_name, mesh_info) in mesh_infos.iter_mut() {
					let mesh = mesh_info.mesh;

					if save_anims {
						mesh_anims.clear();
						mesh_anims.extend(
							all_anims
								.iter()
								.filter(|(_, anim)| mesh.is_anim_compatible(anim)),
						);

						// dedup anims
						let mut cleared_any = false;
						for i in 1..mesh_anims.len() {
							let (anims, target) = mesh_anims.split_at_mut(i);
							let (target_name, target_anim) = target.first_mut().unwrap();
							assert!(!target_name.is_empty(), "empty animation name");
							if anims.iter().any(|(_, anim)| anim == target_anim) {
								*target_name = "";
								cleared_any = true;
							}
						}
						if cleared_any {
							mesh_anims.retain(|(name, _)| !name.is_empty());
						}
					}

					let mut colours = mesh.get_used_vertex_colours();
					for &mat_name in &mesh.materials {
						if let Some(tex_info) = texture_infos.get(mat_name) {
							colours |= tex_info.colours;
						} else if let Some(Pen::Colour(n)) = all_pens.get(mat_name) {
							colours.push(*n);
						}
					}
					let num_arenas =
						filter_colours(colours, &palettes, &mut mesh_info.arenas, true);
					let is_unique = num_arenas == 1;

					for (mesh_arena, _) in &mesh_info.arenas {
						let filename = if is_unique {
							mesh_name
						} else {
							temp_filename.clear();
							write!(temp_filename, "{mesh_name}_{mesh_arena}");
							&temp_filename
						};

						mesh_materials.clear();
						mesh_materials.extend(mesh.materials.iter().map(|&mat_name| {
							if let Some(tex_info) = texture_infos.get(mat_name) {
								let tex = &all_textures[mat_name][0];

								let tex_filename = if tex_info.filenames.len() == 1 {
									tex_info.filenames[0].as_str()
								} else {
									let tex_saved_arena = tex_info
										.arenas
										.iter()
										.find(|(src_arena, _)| mesh_arena == src_arena)
										.unwrap()
										.1;
									let tex_arena_index = tex_info
										.arenas
										.iter()
										.filter(|(a1, a2)| a1 == a2)
										.position(|(src_arena, _)| tex_saved_arena == *src_arena)
										.unwrap();
									tex_info.filenames[tex_arena_index].as_str()
								};

								ResolvedMaterial::TextureRef {
									width: tex.width,
									height: tex.height,
									path: tex_filename,
									masked: tex_info.colours.contains(0),
								}
							} else if let Some(pen) = all_pens.get(mat_name) {
								ResolvedMaterial::Pen(*pen)
							} else {
								ResolvedMaterial::Missing
							}
						}));

						mesh.save_as(
							filename,
							&mut mesh_output,
							save_textures.then_some(&Materials {
								materials: &mesh_materials,
								palette: &palettes[mesh_arena],
								translucent_colours: dti.translucent_colours,
							}),
							&mesh_anims,
						);
					}
				}
			}

			if save_textures {
				let mut mesh_tex_output = mesh_output.push_dir("Textures"); // inside mesh folder
				let mut other_tex_output = output.push_dir("Other Textures");
				let mut other_anim_output = output.push_dir("Animations");
				let mut temp_arenas: Vec<(&str, &str)> = Vec::new();

				dti.skybox
					.save_as("Sky", &mut other_tex_output, Some(dti.pal));
				if let Some(sky) = &dti.reflected_skybox {
					sky.save_as("Reflection", &mut other_tex_output, Some(dti.pal));
				}

				for (&tex_name, &frames) in &all_textures {
					if let Some(tex_info) = texture_infos.get(tex_name) {
						let is_unique = tex_info.filenames.len() == 1;
						for &(arena_src, arena_dest) in &tex_info.arenas {
							if arena_src != arena_dest {
								continue;
							}
							let filename = if is_unique {
								tex_name
							} else {
								temp_filename.clear();
								write!(temp_filename, "{tex_name}_{arena_src}").unwrap();
								&temp_filename
							};
							Texture::save_animated(
								frames,
								filename,
								24,
								&mut mesh_tex_output,
								palettes.get(arena_src).map(Vec::as_slice),
							);
							if is_unique {
								break;
							}
						}
					} else {
						// not used by any mesh

						// find source mtos and use their palette
						temp_arenas.clear();
						'outer: for arena in &mto.arenas {
							for (mti_name, _) in &arena.mti.materials {
								if tex_name == *mti_name {
									temp_arenas.push((arena.name, arena.name));
									break 'outer;
								}
							}
						}
						if temp_arenas.is_empty() {
							// try all palettes
							//println!("level {level_index} texture {name} can't find arena");
							temp_arenas.extend(palettes.keys().map(|&arena| (arena, arena)));
							filter_textures(frames, &palettes, &mut temp_arenas, true);
						}

						let output = if frames.len() == 1 {
							&mut other_tex_output
						} else {
							&mut other_anim_output
						};

						let unique_pal = temp_arenas.len() == 1;
						let fps = 24;
						for &(arena, _) in &temp_arenas {
							let filename = if unique_pal {
								tex_name
							} else {
								temp_filename.clear();
								write!(temp_filename, "{tex_name}_{arena}").unwrap();
								&temp_filename
							};
							Texture::save_animated(
								frames,
								filename,
								fps,
								output,
								Some(&palettes[arena]),
							);
						}
					}
				}
			}
			/*
				// save mesh textures
				if save_textures {
					let mut tex_output = mesh_output.push_dir("Textures"); // inside mesh folder
					for (&name, arenas) in arenas_containing_texture.iter_mut() {
						let tex = all_textures[name];
						let num_unique = filter_textures(tex, &palettes, arenas, false);

						if num_unique == 1 {
							Texture::save_animated(
								tex,
								name,
								24,
								&mut tex_output,
								Some(&palettes[arenas[0].0]),
							);
						} else {
							//println!("level {level_index} splitting mesh texture {name}");
							for &(arena_src, arena_dest) in arenas.iter() {
								if arena_src == arena_dest {
									temp_filename.clear();
									write!(temp_filename, "{name}_{arena_src}").unwrap();
									Texture::save_animated(
										tex,
										&temp_filename,
										24,
										&mut tex_output,
										Some(&palettes[arena_src]),
									);
								}
							}
						}
					}
				}

				if save_meshes {
					let all_texture_colours: HashMap<&str, ColourMap> = all_textures
						.iter()
						.filter(|(name, _)| arenas_containing_texture.contains(name))
						.map(|(name, frames)| (*name, ColourMap::from_frames(frames)))
						.collect();

					let mut mesh_arenas = Vec::new();
					let mut mesh_animations = Vec::<(&str, &Animation)>::new();
					let mut mesh_materials = Vec::new();

					for (&mesh_name, &mesh) in all_meshes.iter() {
						if save_anims {
							mesh_animations.clear();
							mesh_animations.extend(
								all_anims
									.iter()
									.filter(|(_anim_name, anim)| mesh.is_anim_compatible(anim)),
							);
						}

						mesh_arenas.clear();
						// find which arenas contain this mesh
						if let Some(cmi_entity) = cmi.entities.get(mesh_name) {
							mesh_arenas.extend(cmi_entity.arenas.iter().map(|arena| (*arena, *arena)));
						}
						if mesh_arenas.is_empty() {
							// no arenas directly listed, try and figure it out from shared materials
							for matname in &mesh.materials {
								if let Some(tex_arenas) = arenas_containing_texture.get(matname) {
									mesh_arenas
										.extend(tex_arenas.iter().copied().filter(|&(a, b)| a == b));
								}
							}
						}
						if mesh_arenas.is_empty() {
							// mesh has no hints or textures at all, try every palette
							mesh_arenas.extend(
								palettes
									.keys()
									.map(String::as_str)
									.map(|arena| (arena, arena)),
							);
						}

						// get all unique palette colour cominations
						let mut used_colours = mesh.get_used_vertex_colours();
						for mat in &mesh.materials {
							if let Some(col) = all_texture_colours.get(mat) {
								used_colours |= *col;
							}
						}
						let num_unique_arenas =
							filter_colours(used_colours, &palettes, &mut mesh_arenas, true);

						for (target_arena, _) in &mesh_arenas {
							let filename = if num_unique_arenas == 1 {
								mesh_name
							} else {
								temp_filename.clear();
								write!(temp_filename, "{mesh_name}_{target_arena}").unwrap();
								&temp_filename
							};

							mesh_materials.clear();
							mesh_materials.extend(mesh.materials.iter().map(|&mat_name| {
								if let Some(frames) = all_textures.get(mat_name) {
									arenas_containing_texture[mat_name];

									let tex = &frames[0];
									ResolvedMaterial::TextureRef {
										width: tex.width,
										height: tex.height,
										path: (),
										masked: used_colours[mat_name].contains(0),
									}
								} else if let Some(pen) = all_pens.get(mat_name) {
									ResolvedMaterial::Pen(pen)
								} else {
									ResolvedMaterial::Missing
								}
							}));

							mesh.save_as(
								&filename,
								&mut mesh_output,
								Some(Materials {
									materials: &mesh_materials,
									palette: &palettes[target_arena],
									translucent_colours: dti.translucent_colours,
								}),
								&mesh_animations,
							);
						}
					}
				}

				// save unused/other textures
				if save_textures {
					let mut temp_arenas: Vec<(&str, &str)> = Vec::new();
					let mut tex_output = output.push_dir("Other Textures");
					let mut anim_output = output.push_dir("Animations");

					dti.skybox.save_as("Sky", &mut tex_output, Some(dti.pal));
					if let Some(sky) = &dti.reflected_skybox {
						sky.save_as("Reflection", &mut tex_output, Some(dti.pal));
					}

					for (&name, tex) in all_textures.iter() {
						if arenas_containing_texture.contains_key(name) {
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
							filter_textures(tex, &palettes, &mut temp_arenas, true);
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
								temp_filename.clear();
								write!(temp_filename, "{name}_{arena}").unwrap();
								Texture::save_animated(
									tex,
									&temp_filename,
									fps,
									output,
									Some(&palettes[arena]),
								);
							}
						}
					}
				}

			*/

			// debug anim part report
			if save_anims {
				let mut report = String::new();
				for &(name, anim) in all_anims.iter() {
					anim.check_joints(name, &mut report);
				}
				mesh_output.write("anim_report", "txt", &report);
			}
		}

		if let Some(gunt_pal) = palettes.remove("GUNT_10") {
			shared_palette = Some(gunt_pal);
		} else if shared_palette.is_none() {
			shared_palette = palettes.drain().next().map(|(_, pal)| pal);
		}
	}

	// save shared stuff
	let shared_output = OutputWriter::new("assets/TRAVERSE/Shared/", save_sounds || save_textures);
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
				for (i, tex) in frames.iter().enumerate() {
					temp_filename.clear();
					write!(temp_filename, "PICKUPS_{i}").unwrap();
					tex.save_as(&temp_filename, &mut tex_output, shared_palette.as_deref());
				}
				continue;
			}

			let output = if frames.len() == 1 {
				&mut tex_output
			} else {
				&mut anim_output
			};

			Texture::save_animated(frames, name, 24, output, shared_palette.as_deref());
		}
	}
}
