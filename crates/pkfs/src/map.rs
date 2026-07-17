//! Field-map assembly: turn Gen 4 map matrices and land-data chunks into
//! textured terrain GLBs. Each chunk is textured with the map texture set the
//! game actually assigns to it — resolved through the matrix header layer, the
//! arm9 map-header table, and the area-data NARC — so there are no cross-set
//! name collisions.

use pkfs_rom::blz::blz_decode;
use pkfs_rom::map::{
    map_header_areas, parse_buildings, parse_map_matrix, terrain_model, Building, MapHeaderLayout,
    MapMatrix,
};
use pkfs_rom::{Narc, Rom};
use std::collections::{BTreeMap, HashMap, HashSet};

struct MapPaths {
    matrix: &'static str,
    land: &'static str,
    texset: &'static str,
    area_data: &'static str,
    /// Build-model NARC: the NSBMD models for buildings/props.
    build: &'static str,
    /// Prop-texture NARC (`areabm_texset`): one BTX0 per area, indexed by the
    /// area's map-prop archive id.
    prop_texset: &'static str,
    layout: MapHeaderLayout,
}

fn map_paths(gamecode: &str) -> Option<MapPaths> {
    // HGSS `struct MapHeader`: wildEncounterBank(0), areaDataBank(1), ...,
    // matrixId(4). DP/Pt: areaData(0), moveModel(1), matrixId(2), ...
    let hgss = MapHeaderLayout {
        stride: 24,
        area_off: 1,
        matrix_off: 4,
    };
    let dppt = MapHeaderLayout {
        stride: 24,
        area_off: 0,
        matrix_off: 2,
    };
    match &gamecode[..gamecode.len().min(3)] {
        "IPK" | "IPG" => Some(MapPaths {
            matrix: "a/0/4/1",
            land: "a/0/6/5",
            texset: "a/0/4/4",
            area_data: "a/0/4/2",
            build: "a/0/4/0",
            prop_texset: "a/0/7/0",
            layout: hgss,
        }),
        "ADA" | "APA" => Some(MapPaths {
            matrix: "fielddata/mapmatrix/map_matrix.narc",
            land: "fielddata/land_data/land_data_release.narc",
            texset: "fielddata/areadata/area_map_tex/map_tex_set.narc",
            area_data: "fielddata/areadata/area_data.narc",
            build: "fielddata/build_model/build_model.narc",
            prop_texset: "fielddata/areadata/area_build_model/areabm_texset.narc",
            layout: dppt,
        }),
        "CPU" => Some(MapPaths {
            matrix: "fielddata/mapmatrix/map_matrix.narc",
            land: "fielddata/land_data/land_data.narc",
            texset: "fielddata/areadata/area_map_tex/map_tex_set.narc",
            area_data: "fielddata/areadata/area_data.narc",
            build: "fielddata/build_model/build_model.narc",
            prop_texset: "fielddata/areadata/area_build_model/areabm_texset.narc",
            layout: dppt,
        }),
        _ => None,
    }
}

/// The overworld NARCs and matrix index for a game.
pub struct MapSource {
    pub matrix_path: String,
    pub matrix_index: usize,
    pub land_path: String,
    pub texset_path: String,
    pub area_data_path: String,
    pub build_path: String,
    pub prop_texset_path: String,
    pub layout: MapHeaderLayout,
}

/// Locate a game's overworld. Restricted to the DPPt/HGSS map structure.
pub fn detect_map(rom: &Rom) -> Option<MapSource> {
    let p = map_paths(&rom.header().gamecode)?;
    let matrix_data = rom.file_by_path(p.matrix).ok()?;
    if !pkfs_rom::signature::is_narc(matrix_data) || rom.file_by_path(p.land).is_err() {
        return None;
    }
    let matrix_narc = Narc::parse(matrix_data).ok()?;

    let mut matrix_index = 0;
    let mut best_cells = 0;
    for i in 0..matrix_narc.file_count() {
        if let Ok(s) = matrix_narc.file(i) {
            if let Some(m) = parse_map_matrix(s) {
                let cells = (m.width * m.height) as usize;
                if cells > best_cells {
                    best_cells = cells;
                    matrix_index = i;
                }
            }
        }
    }
    if best_cells == 0 {
        return None;
    }

    Some(MapSource {
        matrix_path: p.matrix.to_string(),
        matrix_index,
        land_path: p.land.to_string(),
        texset_path: p.texset.to_string(),
        area_data_path: p.area_data.to_string(),
        build_path: p.build.to_string(),
        prop_texset_path: p.prop_texset.to_string(),
        layout: p.layout,
    })
}

/// One building placed on a chunk: its transform plus the key of the textured
/// GLB to draw (an index into [`Overworld::building_glbs`]).
#[derive(Clone, Debug)]
pub struct BuildingPlacement {
    pub glb_key: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

/// A fully assembled overworld: the region grid plus a textured GLB per chunk,
/// and the buildings placed on each chunk.
pub struct Overworld {
    pub matrix: MapMatrix,
    /// `(chunk_id, glb_bytes)`; `chunk_id` matches [`MapMatrix::land_ids`].
    pub chunks: Vec<(u32, Vec<u8>)>,
    /// `(chunk_id, placements)`: the buildings sitting on each land chunk.
    pub buildings: Vec<(u32, Vec<BuildingPlacement>)>,
    /// `(glb_key, glb_bytes)`: each distinct textured building model, keyed by
    /// [`BuildingPlacement::glb_key`].
    pub building_glbs: Vec<(u32, Vec<u8>)>,
}

/// Detect and assemble the main overworld of `rom`, if it has one.
pub fn load_overworld(rom: &Rom) -> Option<Overworld> {
    let src = detect_map(rom)?;
    let matrix = load_map_matrix(rom, &src.matrix_path, src.matrix_index)?;
    let chunks = build_chunks(rom, &src, &matrix);
    let (buildings, building_glbs) = build_buildings(rom, &src, &matrix);
    Some(Overworld {
        matrix,
        chunks,
        buildings,
        building_glbs,
    })
}

/// Load and parse map matrix `index` from the matrix NARC at `matrix_path`.
pub fn load_map_matrix(rom: &Rom, matrix_path: &str, index: usize) -> Option<MapMatrix> {
    let data = rom.file_by_path(matrix_path).ok()?;
    let narc = Narc::parse(data).ok()?;
    parse_map_matrix(narc.file(index).ok()?)
}

/// Resolve the area-data index each land-data chunk belongs to, via the matrix
/// header layer -> arm9 map-header table. Returns `land_id -> area_index`.
fn resolve_areas(rom: &Rom, src: &MapSource, matrix: &MapMatrix) -> Option<HashMap<u32, usize>> {
    if matrix.headers.is_empty() {
        return None;
    }
    let overworld_hids: Vec<u16> = matrix
        .headers
        .iter()
        .copied()
        .filter(|&h| h != 0xFFFF)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let arm9 = blz_decode(rom.arm9());
    let area_by_header = map_header_areas(&arm9, &overworld_hids, src.layout)?;

    let mut out = HashMap::new();
    let cells = (matrix.width * matrix.height) as usize;
    for cell in 0..cells {
        let hid = matrix.headers[cell];
        let lid = matrix.land_ids[cell];
        if hid == 0xFFFF || lid == 0xFFFF {
            continue;
        }
        let area = *area_by_header.get(hid as usize)? as usize;
        out.insert(lid as u32, area);
    }
    Some(out)
}

/// The map texture-set index each land-data chunk should use: the u16 at offset
/// 2 of its area-data entry. Returns `land_id -> texset_index`.
fn resolve_texsets(rom: &Rom, src: &MapSource, matrix: &MapMatrix) -> Option<HashMap<u32, u16>> {
    let area_by_land = resolve_areas(rom, src, matrix)?;
    let area_narc = Narc::parse(rom.file_by_path(&src.area_data_path).ok()?).ok()?;
    let mut out = HashMap::new();
    for (&lid, &area) in &area_by_land {
        if let Ok(entry) = area_narc.file(area) {
            if entry.len() >= 4 {
                out.insert(lid, u16::from_le_bytes([entry[2], entry[3]]));
            }
        }
    }
    Some(out)
}

fn build_chunks(rom: &Rom, src: &MapSource, matrix: &MapMatrix) -> Vec<(u32, Vec<u8>)> {
    let Ok(land) = rom
        .file_by_path(&src.land_path)
        .and_then(Narc::parse)
    else {
        return Vec::new();
    };
    let mut terrains: HashMap<u32, Vec<u8>> = HashMap::new();
    for i in 0..land.file_count() {
        if let Ok(chunk) = land.file(i) {
            if let Some(model) = terrain_model(chunk) {
                terrains.insert(i as u32, model.to_vec());
            }
        }
    }

    // Each chunk's own texture set (no cross-set fallback).
    let Some(texset_by_land) = resolve_texsets(rom, src, matrix) else {
        // Without a resolvable assignment we decode terrain untextured rather
        // than guess (which binds wrong textures).
        return decode_untextured(&terrains);
    };
    let Ok(tex_narc) = rom
        .file_by_path(&src.texset_path)
        .and_then(Narc::parse)
    else {
        return decode_untextured(&terrains);
    };

    // Group chunks by texture set so each set is decoded once.
    let mut groups: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    for (&lid, &ts) in &texset_by_land {
        if terrains.contains_key(&lid) {
            groups.entry(ts).or_default().push(lid);
        }
    }

    let mut out = Vec::new();
    for (ts, lids) in groups {
        let mut buffers: Vec<Vec<u8>> = lids.iter().map(|l| terrains[l].clone()).collect();
        if let Ok(f) = tex_narc.file(ts as usize) {
            if f.starts_with(b"BTX0") {
                buffers.push(f.to_vec());
            }
        }
        let glbs = pkfs_nitro::buffers_to_glbs(buffers).unwrap_or_default();
        for (lid, g) in lids.iter().zip(glbs) {
            out.push((*lid, g.bytes));
        }
    }

    // Border/backdrop chunks (the distant, unwalkable terrain) carry a map
    // header whose own texture set doesn't include their materials — in-engine
    // they're only ever drawn beside a walkable area, which binds *its* texture
    // to them. Reconstruct that by re-texturing any untextured chunk with the
    // set used by its spatial neighbours, most common first.
    let neighbours = neighbour_texsets(matrix, &texset_by_land);
    for (lid, glb) in out.iter_mut() {
        if is_textured(glb) {
            continue;
        }
        let Some(model) = terrains.get(lid) else {
            continue;
        };
        for ts in neighbours.get(lid).into_iter().flatten() {
            let Ok(f) = tex_narc.file(*ts as usize) else {
                continue;
            };
            if !f.starts_with(b"BTX0") {
                continue;
            }
            let decoded = pkfs_nitro::buffers_to_glbs(vec![model.clone(), f.to_vec()])
                .ok()
                .and_then(|g| g.into_iter().next());
            if let Some(g) = decoded {
                if is_textured(&g.bytes) {
                    *glb = g.bytes;
                    break;
                }
            }
        }
    }
    out
}

/// Whether a decoded GLB embeds any texture image.
fn is_textured(glb: &[u8]) -> bool {
    glb.windows(9).any(|w| w == b"image/png")
}

/// For each land id, the texture sets used by spatially adjacent cells that
/// hold a *different* land id, most frequent first. Used to texture border
/// chunks the way the engine does: with the neighbouring area's set.
fn neighbour_texsets(
    matrix: &MapMatrix,
    texset_by_land: &HashMap<u32, u16>,
) -> HashMap<u32, Vec<u16>> {
    let (w, h) = (matrix.width as i32, matrix.height as i32);
    let at = |x: i32, y: i32| matrix.land_ids[(y * w + x) as usize];
    let mut counts: HashMap<u32, HashMap<u16, u32>> = HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let lid = at(x, y);
            if lid == 0xFFFF {
                continue;
            }
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let nl = at(nx, ny);
                if nl == 0xFFFF || nl == lid {
                    continue;
                }
                if let Some(&ts) = texset_by_land.get(&(nl as u32)) {
                    *counts.entry(lid as u32).or_default().entry(ts).or_default() += 1;
                }
            }
        }
    }
    counts
        .into_iter()
        .map(|(lid, m)| {
            let mut v: Vec<(u16, u32)> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            (lid, v.into_iter().map(|(ts, _)| ts).collect())
        })
        .collect()
}

fn decode_untextured(terrains: &HashMap<u32, Vec<u8>>) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    for (&lid, model) in terrains {
        if let Ok(glbs) = pkfs_nitro::buffers_to_glbs(vec![model.clone()]) {
            if let Some(g) = glbs.into_iter().next() {
                out.push((lid, g.bytes));
            }
        }
    }
    out
}

/// A distinct textured building GLB is keyed by its (prop-archive, model id)
/// pair: the same build model textured by two areas' prop sets differs.
fn building_key(prop_arc: u16, model_id: u16) -> u32 {
    ((prop_arc as u32) << 16) | model_id as u32
}

/// The building placements per chunk plus the distinct textured building GLBs,
/// as returned by [`build_buildings`].
type Buildings = (Vec<(u32, Vec<BuildingPlacement>)>, Vec<(u32, Vec<u8>)>);

/// Assemble the buildings placed on every land chunk. Each chunk's placements
/// come from its land-data buildings section; the model comes from the build
/// NARC and is textured with the chunk area's prop texture set
/// (`areabm_texset`), mirroring the engine.
fn build_buildings(rom: &Rom, src: &MapSource, matrix: &MapMatrix) -> Buildings {
    let empty = (Vec::new(), Vec::new());
    let (Ok(land), Ok(build), Ok(prop_tex), Ok(area_narc), Some(area_by_land)) = (
        rom.file_by_path(&src.land_path).and_then(Narc::parse),
        rom.file_by_path(&src.build_path).and_then(Narc::parse),
        rom.file_by_path(&src.prop_texset_path).and_then(Narc::parse),
        rom.file_by_path(&src.area_data_path).and_then(Narc::parse),
        resolve_areas(rom, src, matrix),
    ) else {
        return empty;
    };

    // Placements per chunk + the set of (prop_arc, model_id) models to decode,
    // grouped by prop archive so each prop texture set is decoded once.
    let mut placements: Vec<(u32, Vec<BuildingPlacement>)> = Vec::new();
    let mut groups: BTreeMap<u16, HashSet<u16>> = BTreeMap::new();
    let mut seen_land: HashSet<u32> = HashSet::new();

    let cells = (matrix.width * matrix.height) as usize;
    for cell in 0..cells {
        let lid = matrix.land_ids[cell] as u32;
        if matrix.land_ids[cell] == 0xFFFF || !seen_land.insert(lid) {
            continue;
        }
        let Some(&area) = area_by_land.get(&lid) else {
            continue;
        };
        let prop_arc = match area_narc.file(area) {
            Ok(e) if e.len() >= 2 => u16::from_le_bytes([e[0], e[1]]),
            _ => continue,
        };
        let Ok(chunk) = land.file(lid as usize) else {
            continue;
        };
        let buildings: Vec<Building> = parse_buildings(chunk);
        if buildings.is_empty() {
            continue;
        }
        let mut list = Vec::with_capacity(buildings.len());
        for b in buildings {
            groups.entry(prop_arc).or_default().insert(b.model_id);
            list.push(BuildingPlacement {
                glb_key: building_key(prop_arc, b.model_id),
                position: b.position,
                rotation: b.rotation,
                scale: b.scale,
            });
        }
        placements.push((lid, list));
    }

    // Decode each prop archive's models in one pass, textured with its BTX0.
    let mut glbs: Vec<(u32, Vec<u8>)> = Vec::new();
    for (prop_arc, model_ids) in groups {
        let ids: Vec<u16> = model_ids.into_iter().collect();
        let mut buffers: Vec<Vec<u8>> = ids
            .iter()
            .filter_map(|&id| build.file(id as usize).ok().map(<[u8]>::to_vec))
            .collect();
        if buffers.len() != ids.len() {
            continue; // a model failed to read; keep keys aligned
        }
        if let Ok(f) = prop_tex.file(prop_arc as usize) {
            if f.starts_with(b"BTX0") {
                buffers.push(f.to_vec());
            }
        }
        let decoded = pkfs_nitro::buffers_to_glbs(buffers).unwrap_or_default();
        for (id, g) in ids.iter().zip(decoded) {
            glbs.push((building_key(prop_arc, *id), g.bytes));
        }
    }

    (placements, glbs)
}
