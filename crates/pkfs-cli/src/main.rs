use anyhow::Result;
use clap::{Parser, Subcommand};
use pkfs::{dump_rom, manifest_for_gamecode, Rom};
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "pkfs",
    about = "Texture, sprite and model dumper for NDS-era Pokemon games"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print ROM header and recognised game.
    Info { rom: String },
    /// List files in the ROM filesystem, annotated with known roles.
    Ls {
        rom: String,
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Decode every recognised asset (models, sprites, textures) to a directory.
    Dump {
        rom: String,
        out: String,
        #[arg(long)]
        raw: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Info { rom } => info(&rom),
        Command::Ls { rom, filter } => ls(&rom, filter.as_deref()),
        Command::Dump { rom, out, raw } => {
            let r = dump_rom(&Rom::from_file(&rom)?, Path::new(&out), raw)?;
            println!(
                "Dumped {} models, {} textures, {} sprites, {} raw files to {out}",
                r.models, r.textures, r.sprites, r.bins
            );
            Ok(())
        }
    }
}

fn info(rom_path: &str) -> Result<()> {
    let rom = Rom::from_file(rom_path)?;
    let h = rom.header();
    println!("Title      : {}", h.title);
    println!("Game code  : {}", h.gamecode);
    println!("Maker code : {}", h.makercode);
    println!("ROM used   : {} bytes", h.rom_size_used);
    println!("FAT files  : {}", rom.file_count());
    println!("Named files: {}", rom.files().len());
    println!(
        "Recognised : {}",
        manifest_for_gamecode(&h.gamecode).map_or("unknown game", |m| m.display_name.as_str())
    );
    Ok(())
}

fn ls(rom_path: &str, filter: Option<&str>) -> Result<()> {
    let rom = Rom::from_file(rom_path)?;
    let manifest = manifest_for_gamecode(&rom.header().gamecode);
    let mut shown = 0;
    for f in rom.files() {
        if let Some(sub) = filter {
            if !f.path.contains(sub) {
                continue;
            }
        }
        match manifest.and_then(|m| m.find(&f.path)) {
            Some(kf) => println!(
                "{:>5}  {:<24}  [{}] {}",
                f.id, f.path, kf.kind, kf.description
            ),
            None => println!("{:>5}  {}", f.id, f.path),
        }
        shown += 1;
    }
    println!("({shown} files)");
    Ok(())
}
