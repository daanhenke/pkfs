//! High-level facade tying together the ROM filesystem, 2D graphics and Nitro
//! model decoders, plus the full-ROM asset dump.

pub mod dump;
pub mod map;

pub use dump::{assets_at_path, dump_rom, Assets, DumpReport};
pub use map::{detect_map, load_map_matrix, load_overworld, MapSource, Overworld};
pub use pkfs_2d as gfx2d;
pub use pkfs_nitro as nitro;
pub use pkfs_rom as rom;
pub use pkfs_rom::map::MapMatrix;

pub use pkfs_rom::{manifest_for_gamecode, Rom};
