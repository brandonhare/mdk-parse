# MDK-PARSE

## About
A tool to extract all the game assets from [MDK](https://en.wikipedia.org/wiki/MDK) (1997) by Shiny Enertainment.

Warning: This project is unfinished, mostly-undocumented, and disorganized!  It came together organically as I was learning Rust and slowly reverse-engineering the MDK data formats, so it's quite messy in places.

This project is probably only interesting to like 5 people on the planet, so if you get some value from it please let me know!

## How to run
1. [Install Rust](https://www.rust-lang.org/learn/get-started)
2. Create a folder named `assets` in the project's root directory (i.e. adjacent to `Cargo.toml`)
3. Copy the MDK data folders into the `assets` folder.  It should look like:
	* `assets/FALL3D/...`
	* `assets/MISC/...`
	* `assets/STREAM/...`
	* `assets/TRAVERSE/...`
4. (Optional) Install `ffmpeg`.  On windows you should be get it by running `winget install ffmpeg`
5. Run the project with `cargo run -r`
6. The game assets should be exported to a folder named `output`
	* Images/textures/colour-palettes are saved as PNGs
	* Sounds are saved as WAVs
	* 2D Animated sprites are saved as animated PNGs.  Try opening them in different programs if you don't see them animate.  (I should probably change them to gif some day)
	* 3D Models are saved as GLTFs.  You should be able to open them in blender or something.
	* 3D Animations are saved as GLTFs full of purple dots inside the `Meshes/Animations` folders.  MDK uses vertex animation and I haven't finished mapping them onto their actual models.
	* Videos are converted to MP4 files.
	* Gameplay scripts and some metadata is exported as TXT or TSV files


## MDK Data Format
If you're just interested in the MDK data file formats themselves, check out the parsing code in `src/file_formats` and `src/data_formats`.  Hopefully it's not too difficult to pick apart the file-reading code but I do hope to actually document them properly one day.

The export code is a lot more complicated since all the files depend on each other in non-intuitive ways and since I try to do a ton of duduplication.
