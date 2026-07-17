//! Godot 4 GDExtension bindings for pkfs. Exposes a `PkfsRom` object that opens
//! a Gen 4/5 Pokemon ROM and hands assets to Godot: models as self-contained
//! GLB byte-arrays (load with `GLTFDocument`), textures/sprites as PNG
//! byte-arrays (load with `Image.load_png_from_buffer`), plus a full disk dump.

use godot::prelude::*;

struct PkfsExtension;

#[gdextension]
unsafe impl ExtensionLibrary for PkfsExtension {}

#[derive(GodotClass)]
#[class(base=RefCounted, init)]
struct PkfsRom {
    rom: Option<pkfs::Rom>,
    base: Base<RefCounted>,
}

#[godot_api]
impl PkfsRom {
    /// Open a ROM from an absolute path. Returns false on failure.
    #[func]
    fn open(&mut self, path: GString) -> bool {
        match pkfs::Rom::from_file(path.to_string()) {
            Ok(rom) => {
                self.rom = Some(rom);
                true
            }
            Err(e) => {
                godot_error!("pkfs: failed to open ROM: {e}");
                false
            }
        }
    }

    /// Human-readable game name (falls back to the ROM's internal title).
    #[func]
    fn game_name(&self) -> GString {
        match &self.rom {
            Some(r) => {
                let name = pkfs::manifest_for_gamecode(&r.header().gamecode)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| r.header().title.clone());
                GString::from(name.as_str())
            }
            None => GString::new(),
        }
    }

    /// Four-character game code.
    #[func]
    fn game_code(&self) -> GString {
        self.rom
            .as_ref()
            .map(|r| r.header().gamecode.as_str())
            .unwrap_or("")
            .into()
    }

    #[func]
    fn file_count(&self) -> i64 {
        self.rom.as_ref().map_or(0, |r| r.file_count() as i64)
    }

    /// Every named file path in the ROM filesystem.
    #[func]
    fn list_files(&self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if let Some(rom) = &self.rom {
            for f in rom.files() {
                out.push(&GString::from(f.path.as_str()));
            }
        }
        out
    }

    /// GLB byte-arrays for every model at a ROM path (a NARC or a single file).
    /// Each is a self-contained binary glTF; load with `GLTFDocument`.
    #[func]
    fn models_glb(&self, path: GString) -> Array<PackedByteArray> {
        let mut out = Array::new();
        if let Some(rom) = &self.rom {
            for (_, bytes) in pkfs::assets_at_path(rom, &path.to_string(), false).models {
                out.push(&PackedByteArray::from(bytes.as_slice()));
            }
        }
        out
    }

    /// PNG byte-arrays for every texture/sprite at a ROM path. Load each with
    /// `Image.load_png_from_buffer`.
    #[func]
    fn image_pngs(&self, path: GString) -> Array<PackedByteArray> {
        let mut out = Array::new();
        if let Some(rom) = &self.rom {
            for (_, bytes) in pkfs::assets_at_path(rom, &path.to_string(), false).images {
                out.push(&PackedByteArray::from(bytes.as_slice()));
            }
        }
        out
    }

    /// True if this game has a detectable Gen 4 overworld (DPPt/HGSS).
    #[func]
    fn has_overworld(&self) -> bool {
        self.rom
            .as_ref()
            .map(|r| pkfs::detect_map(r).is_some())
            .unwrap_or(false)
    }

    /// Detect and assemble the game's overworld. Returns
    /// { width, height, name, land_ids, altitudes: PackedInt32Array,
    ///   ids: PackedInt32Array, glbs: Array[PackedByteArray],
    ///   building_keys: PackedInt32Array, building_glbs: Array[PackedByteArray],
    ///   chunk_buildings: Dictionary }.
    /// `ids`/`glbs` align (chunk id -> textured terrain GLB); a matrix cell's
    /// `land_ids[i]` (65535 = empty) selects the GLB whose id matches.
    /// `building_keys`/`building_glbs` align (key -> textured building GLB);
    /// `chunk_buildings` maps a chunk id -> Array of { key, pos, rot, scale }
    /// placements (Vector3s in world units, relative to the chunk origin).
    #[func]
    fn load_overworld(&self) -> Dictionary {
        let mut d = Dictionary::new();
        let Some(rom) = &self.rom else { return d };
        let Some(ow) = pkfs::load_overworld(rom) else {
            return d;
        };

        d.set("width", ow.matrix.width as i64);
        d.set("height", ow.matrix.height as i64);
        d.set("name", GString::from(ow.matrix.name.as_str()));
        let land: PackedInt32Array = ow.matrix.land_ids.iter().map(|&x| x as i32).collect();
        d.set("land_ids", land);
        let alt: PackedInt32Array = ow.matrix.altitudes.iter().map(|&x| x as i32).collect();
        d.set("altitudes", alt);

        let ids: PackedInt32Array = ow.chunks.iter().map(|(id, _)| *id as i32).collect();
        let mut glbs: Array<PackedByteArray> = Array::new();
        for (_, bytes) in &ow.chunks {
            glbs.push(&PackedByteArray::from(bytes.as_slice()));
        }
        d.set("ids", ids);
        d.set("glbs", glbs);

        let bkeys: PackedInt32Array = ow.building_glbs.iter().map(|(k, _)| *k as i32).collect();
        let mut bglbs: Array<PackedByteArray> = Array::new();
        for (_, bytes) in &ow.building_glbs {
            bglbs.push(&PackedByteArray::from(bytes.as_slice()));
        }
        d.set("building_keys", bkeys);
        d.set("building_glbs", bglbs);

        let mut chunk_buildings = Dictionary::new();
        for (chunk_id, list) in &ow.buildings {
            let mut arr: Array<Dictionary> = Array::new();
            for p in list {
                let mut e = Dictionary::new();
                e.set("key", p.glb_key as i64);
                e.set("pos", Vector3::new(p.position[0], p.position[1], p.position[2]));
                e.set("rot", Vector3::new(p.rotation[0], p.rotation[1], p.rotation[2]));
                e.set("scale", Vector3::new(p.scale[0], p.scale[1], p.scale[2]));
                arr.push(&e);
            }
            chunk_buildings.set(*chunk_id as i64, arr);
        }
        d.set("chunk_buildings", chunk_buildings);
        d
    }

    /// Dump every recognised asset to `out_dir`. Returns a dictionary of totals.
    #[func]
    fn dump(&self, out_dir: GString, raw: bool) -> Dictionary {
        let mut d = Dictionary::new();
        if let Some(rom) = &self.rom {
            match pkfs::dump_rom(rom, std::path::Path::new(&out_dir.to_string()), raw) {
                Ok(r) => {
                    d.set("models", r.models as i64);
                    d.set("textures", r.textures as i64);
                    d.set("sprites", r.sprites as i64);
                    d.set("bins", r.bins as i64);
                }
                Err(e) => godot_error!("pkfs: dump failed: {e}"),
            }
        }
        d
    }
}
