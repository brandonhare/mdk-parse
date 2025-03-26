use crate::{OutputWriter, Reader, Vec3, data_formats::Texture};

/// DTI files contain a lot of level metadata
pub struct Dti<'a> {
	pub filename: &'a str,

	pub player_start_arena_index: u32,
	pub player_start_pos: Vec3,
	pub player_start_angle: f32,

	pub ceiling_colour: i32,
	pub floor_colour: i32,
	pub reflected_ceiling_colour: i32,
	pub reflected_floor_colour: i32,

	pub skybox: Texture<'a>,
	pub reflected_skybox: Option<Texture<'a>>,

	pub translucent_colours: [[u8; 4]; 4],

	pub arenas: Vec<DtiArena<'a>>,

	pub num_pal_free_pixels: u32,
	pub pal: &'a [u8],
}

#[derive(Debug)]
pub struct DtiArena<'a> {
	pub name: &'a str,
	pub num: f32, // todo what is this
	pub entities: Vec<DtiEntity<'a>>,
	pub teleports: Vec<Teleport>, // todo check these
}

/// These are not actually real game entities, more like special map zones.
/// Most actual gameplay entities come from CMI data.
#[derive(Debug)]
pub struct DtiEntity<'a> {
	pub id: i32,
	pub bbox: [Vec3; 2],
	pub data: DtiEntityData<'a>,
}
#[derive(Debug)]
pub enum DtiEntityData<'a> {
	ArenaShowZone,
	Hotgen { name: &'a str, value: i32 },
	ArenaActivateZone,
	Hotpick(&'a str), // todo what are these
	HidingSpot,
	ArenaConnectZone(i32),
	Fan,
	JumpPoint,
	Slidething,
}
#[derive(Debug)]
pub struct Teleport {
	pub index: i32,
	pub pos: Vec3,
	pub angle: f32,
}

impl Dti<'_> {
	pub fn parse(mut data: Reader) -> Dti {
		let filesize = data.u32() + 4;
		data.rebase();

		let filename = data.str(12);
		let filesize2 = data.u32();
		assert_eq!(filesize, filesize2 + 12);

		let player_and_sky_offset = data.u32() as usize;
		let teleports_offset = data.u32() as usize;
		let entities_offset = data.u32() as usize;
		let pal_offset = data.u32() as usize;
		let skybox_offset = data.u32() as usize;

		// player and skybox info
		let player_start_arena_index;
		let player_start_pos;
		let player_start_angle;
		let ceiling_colour;
		let floor_colour;
		let reflected_ceiling_colour;
		let reflected_floor_colour;
		let translucent_colours;
		let sky_width;
		let sky_height;
		let sky_x;
		let sky_y;
		{
			data.set_position(player_and_sky_offset);
			player_start_arena_index = data.u32();
			player_start_pos = data.vec3();
			player_start_angle = data.f32();

			ceiling_colour = data.i32();
			floor_colour = data.i32();
			sky_y = data.i32();
			sky_x = data.i32();
			sky_width = data.u32();
			sky_height = data.u32();
			reflected_ceiling_colour = data.i32();
			reflected_floor_colour = data.i32();

			// 4 sets of rgba colours, each component stored in 4 bytes
			let colours = data.get::<[[u32; 4]; 4]>();
			translucent_colours = colours.map(|c| c.map(|n| n as u8));

			assert_eq!(data.position(), teleports_offset);
		}

		// arenas/entities
		let mut arenas;
		{
			data.set_position(entities_offset);

			let num_arenas = data.u32();

			arenas = Vec::with_capacity(num_arenas as usize);
			for _arena_index in 0..num_arenas {
				let arena_name = data.str(8);
				let arena_offset = data.u32();
				let arena_num = data.f32();

				let mut arena_data = data.clone_at(arena_offset as usize);
				let num_entities = arena_data.u32() as usize;
				let mut entities = Vec::new();
				entities.reserve_exact(num_entities);

				for _entity_index in 0..num_entities {
					let kind = arena_data.i32();
					let id = arena_data.i32();
					let value = arena_data.i32();
					let pos_min = arena_data.vec3();
					let mut pos_max = pos_min;

					if kind != 2 && kind != 6 {
						assert_eq!(value, 0);
					}

					let data = match kind {
						2 => DtiEntityData::Hotgen {
							name: arena_data.str(12),
							value,
						},
						4 => DtiEntityData::Hotpick(arena_data.str(12)),
						kind => {
							pos_max = arena_data.vec3();
							if pos_max == Default::default() {
								pos_max = pos_min;
							}
							match kind {
								1 => DtiEntityData::ArenaShowZone,
								3 => DtiEntityData::ArenaActivateZone,
								5 => DtiEntityData::HidingSpot,
								6 => DtiEntityData::ArenaConnectZone(value),
								7 => {
									assert_eq!(value, 0);
									DtiEntityData::Fan
								}
								8 => {
									assert_eq!(value, 0);
									DtiEntityData::JumpPoint
								}
								9 => DtiEntityData::Slidething,
								n => panic!("unknown dti entity kind {n}"),
							}
						}
					};

					assert!(
						pos_min.x <= pos_max.x && pos_min.y <= pos_max.y && pos_min.z <= pos_max.z,
						"invalid bbox for entity {id} ({data:?}): [{pos_min}, {pos_max}]"
					);

					entities.push(DtiEntity {
						id,
						bbox: [pos_min, pos_max],
						data,
					});
				}

				arenas.push(DtiArena {
					name: arena_name,
					num: arena_num,
					entities,
					teleports: Vec::new(),
				});
			}
		}

		// teleport locations
		{
			data.set_position(teleports_offset);
			let num_teleports = data.u32();
			for i in 0..num_teleports {
				let index = data.i32();
				let arena_index = data.i32();
				let pos = data.vec3();
				let angle = data.f32();
				assert_eq!(index, (i as i32 + 1) % 10);
				arenas[arena_index as usize]
					.teleports
					.push(Teleport { index, pos, angle });
			}
			assert_eq!(data.position(), entities_offset);
		}

		// pal
		let pal;
		let num_pal_free_pixels;
		{
			data.set_position(pal_offset);
			num_pal_free_pixels = data.u32();
			pal = data.slice(0x300);

			assert_eq!(num_pal_free_pixels % 16, 0);
			assert_eq!(data.position(), skybox_offset);
		}

		// skybox
		let (skybox, reflected_skybox) = {
			data.set_position(skybox_offset);
			let src_width = sky_width as usize + 4;
			let src_height = sky_height as usize;
			let sky_pixels = data.slice(src_width * src_height);

			// trim extra 4 pixels
			let mut pixels = Vec::with_capacity(sky_width as usize * src_height);
			for row in sky_pixels.chunks_exact(src_width) {
				pixels.extend(&row[..sky_width as usize]);
			}

			let mut skybox = Texture::new(sky_width as u16, src_height as u16, pixels);
			skybox.position = (sky_x as i16, sky_y as i16);

			let reflected_skybox = if reflected_ceiling_colour >= 0 {
				let sky_pixels = data.slice(src_width * src_height);

				// trim extra 4 pixels
				let mut pixels = Vec::with_capacity(sky_width as usize * src_height);
				for row in sky_pixels.chunks_exact(src_width) {
					pixels.extend(&row[..sky_width as usize]);
				}

				let mut skybox = Texture::new(sky_width as u16, src_height as u16, pixels);
				skybox.position = (sky_x as i16, sky_y as i16);
				Some(skybox)
			} else {
				None
			};

			(skybox, reflected_skybox)
		};

		let filename_footer = data.str(12);
		assert_eq!(filename, filename_footer);
		assert!(data.is_empty());

		Dti {
			filename,
			player_start_arena_index,
			player_start_pos,
			player_start_angle,
			ceiling_colour,
			floor_colour,
			reflected_ceiling_colour,
			reflected_floor_colour,
			skybox,
			reflected_skybox,
			translucent_colours,
			arenas,
			num_pal_free_pixels,
			pal,
		}
	}

	pub fn save(&self, output: &mut OutputWriter) {
		output.write_palette("palette", self.pal);
		self.skybox.save_as("skybox", output, Some(self.pal));
		self.save_info_as("info", output);
	}

	pub fn save_info_as(&self, info_filename: &str, output: &mut OutputWriter) {
		use std::fmt::Write;
		let mut info = format!(
			"name: {}\n\nplayer start arena: {}, pos: {}, angle: {}\npalette free rows: {}\ntranslucent colours: ",
			self.filename,
			self.player_start_arena_index,
			self.player_start_pos,
			self.player_start_angle,
			self.num_pal_free_pixels / 16,
		);

		for c in self.translucent_colours {
			let value = u32::from_be_bytes(c);
			write!(&mut info, " #{value:08X}").unwrap();
		}
		info.push('\n');

		let mut print_colour = |label, colour_index| {
			if !(0..=255).contains(&colour_index) {
				writeln!(&mut info, "{label}: ({colour_index})").unwrap();
				return;
			}
			let offset = colour_index as usize * 3;
			let rgb = &self.pal[offset..offset + 3];
			let value = u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
			writeln!(&mut info, "{label}: #{value:06X}").unwrap();
		};
		print_colour("floor colour", self.floor_colour);
		print_colour("ceiling colour", self.ceiling_colour);
		print_colour("reflected floor colour", self.reflected_floor_colour);
		print_colour("reflected ceiling colour", self.reflected_ceiling_colour);

		writeln!(&mut info, "\narenas ({}):", self.arenas.len()).unwrap();
		for (arena_index, arena) in self.arenas.iter().enumerate() {
			writeln!(
				info,
				"\t[{arena_index}] {}\n\t\tnum: {}",
				arena.name, arena.num
			)
			.unwrap();

			for tele in arena.teleports.iter() {
				writeln!(info, "\t\t{tele:?}").unwrap();
			}

			writeln!(info, "\t\tentities ({}):", arena.entities.len()).unwrap();

			for (entity_index, entity) in arena.entities.iter().enumerate() {
				if entity.bbox[0] == entity.bbox[1] {
					writeln!(
						info,
						"\t\t\t[{entity_index:3}] id: {:4}, kind: {:?}, position: {}",
						entity.id, entity.data, entity.bbox[0]
					)
					.unwrap();
				} else {
					writeln!(
						info,
						"\t\t\t[{entity_index:3}] id: {:4}, kind: {:?}, bbox: [{}, {}]",
						entity.id, entity.data, entity.bbox[0], entity.bbox[1]
					)
					.unwrap();
				}
			}
			info.push('\n');
		}

		output.write(info_filename, "txt", info.as_bytes());
	}
}
