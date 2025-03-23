use mdk_parse::gamemode_formats::{fall3d, misc, stream, traverse};

fn main() {
	let start_time = std::time::Instant::now();

	let save_sounds = true;
	let save_textures = true;
	let save_meshes = true;
	let save_videos = true;

	println!("Parsing traverse data...");
	traverse::parse_traverse(save_sounds, save_textures, save_meshes);

	println!("Parsing stream data...");
	stream::parse_stream(save_sounds, save_textures, save_meshes);

	println!("Parsing fall3d data...");
	fall3d::parse_fall3d(save_sounds, save_textures, save_meshes);

	println!("Parsing misc data...");
	misc::parse_misc(save_videos);

	println!("Done in {:.2?}", start_time.elapsed());
}
