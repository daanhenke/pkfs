# pkfs

Texture, sprite and model dumper for the NDS-era Pokémon games — Generation 4
(Diamond, Pearl, Platinum, HeartGold, SoulSilver) and Generation 5 (Black,
White, Black 2, White 2) — with a Godot 4 extension and a live overworld viewer.

Assets are decoded straight from the ROM:

- **Models** (NSBMD) → self-contained **GLB**, with textures and **animations**
- **Textures** (NSBTX) → PNG
- **Sprites** (NCGR/NCLR/NCER) → PNG, including pokégra decryption and
  normal/shiny variants
- **Field maps** (Gen 4) → the whole overworld, stitched from land-data chunks

> pkfs ships no game data. Supply your own legally-obtained ROMs.

## Workspace

| Crate | Role |
| --- | --- |
| `pkfs-nitro` | Nitro model/texture/animation decoding + GLB export (vendored from [apicula](https://github.com/scurest/apicula), 0BSD) |
| `pkfs-rom` | ROM filesystem (FAT/FNT), NARC, format detection, map formats, per-game path mapping |
| `pkfs-2d` | NCLR palettes, NCGR tiles, NCER cell assembly, pokégra decryption |
| `pkfs` | Facade: the full asset dump and per-path/overworld helpers |
| `pkfs-cli` | `pkfs` command-line tool |
| `pkfs-gdext` | Godot 4 GDExtension |

## CLI

```bash
cargo run --release -p pkfs-cli -- info  <rom.nds>
cargo run --release -p pkfs-cli -- ls    <rom.nds> [--filter STR]
cargo run --release -p pkfs-cli -- dump  <rom.nds> <out> [--raw]
```

`dump` decodes every recognised asset: models to GLB, textures/sprites to PNG,
and any unrecognised NARC sub-files to `.bin`. Output collapses to a single file
when a source yields one asset and nests into a directory only when there are
several.

## Godot extension + map viewer

`crates/pkfs-gdext` builds a `cdylib` exposing a `PkfsRom` object (open a ROM;
pull models as GLB, textures/sprites as PNG, run a dump, or load the overworld).
`game/` is a sample Godot 4 project that renders a game's field map by stitching
land-data terrain chunks into a grid.

```bash
cargo build --release -p pkfs-gdext
cp target/release/*pkfs_gdext.* game/bin/
godot --path game
```

Press **N/P** to cycle between the Gen 4 games found in the ROM folder; hold
**right mouse** to look and **WASD** to fly.

## Building

Standard Rust workspace (no external SDKs — the Godot API is bundled by
`godot-rust`):

```bash
cargo build --workspace --release
cargo test --workspace
```

## License

MIT for pkfs' own code. `pkfs-nitro` is 0BSD (vendored from apicula). Map
mappings under `data/mappings/` are community-sourced (see comments therein).
