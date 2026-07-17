//! Gen 4 (DPPt/HGSS) field map formats: the map matrix (grid of chunk ids) and
//! land-data chunks (each embedding a terrain NSBMD model plus permissions,
//! building placements and BDHC height data).

/// A parsed map matrix: a `width` x `height` grid referencing land-data chunk
/// ids (0xFFFF marks an empty cell).
#[derive(Clone, Debug)]
pub struct MapMatrix {
    pub width: u32,
    pub height: u32,
    pub name: String,
    /// Row-major land-data chunk id per cell (0xFFFF = empty).
    pub land_ids: Vec<u16>,
    /// Row-major per-cell altitude (base height level). Empty if the matrix
    /// carries no altitude layer.
    pub altitudes: Vec<u8>,
    /// Row-major per-cell map-header id. Empty if the matrix carries no header
    /// layer. Used with the arm9 map-header table to resolve textures.
    pub headers: Vec<u16>,
}

impl MapMatrix {
    pub fn at(&self, x: u32, y: u32) -> Option<u16> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let id = self.land_ids[(y * self.width + x) as usize];
        (id != 0xFFFF).then_some(id)
    }
}

/// Parse a map matrix entry (as found in the `map_matrix` NARC).
pub fn parse_map_matrix(d: &[u8]) -> Option<MapMatrix> {
    if d.len() < 5 {
        return None;
    }
    let width = d[0] as usize;
    let height = d[1] as usize;
    let has_headers = d[2] != 0;
    let has_altitude = d[3] != 0;
    let cells = width * height;

    let name_len = d[4] as usize;
    let mut pos = 5 + name_len;
    let name = String::from_utf8_lossy(d.get(5..5 + name_len)?).into_owned();

    let headers = if has_headers {
        let hdr = d.get(pos..pos + cells * 2)?;
        let out = (0..cells)
            .map(|i| u16::from_le_bytes([hdr[i * 2], hdr[i * 2 + 1]]))
            .collect();
        pos += cells * 2;
        out
    } else {
        Vec::new()
    };
    let altitudes = if has_altitude {
        let alt = d.get(pos..pos + cells)?.to_vec();
        pos += cells;
        alt
    } else {
        Vec::new()
    };

    let ids = d.get(pos..pos + cells * 2)?;
    let land_ids = (0..cells)
        .map(|i| u16::from_le_bytes([ids[i * 2], ids[i * 2 + 1]]))
        .collect();

    Some(MapMatrix {
        width: width as u32,
        height: height as u32,
        name,
        land_ids,
        altitudes,
        headers,
    })
}

/// Layout of a game's map-header struct within the arm9 table.
#[derive(Copy, Clone)]
pub struct MapHeaderLayout {
    pub stride: usize,
    pub area_off: usize,
    pub matrix_off: usize,
}

/// Locate the map-header table inside a decompressed arm9 and return the
/// area-data id for each map-header id. `overworld_header_ids` are the distinct
/// header ids used by the overworld matrix (all map to matrix 0); they pin the
/// exact, 4-aligned table start. Returns `area_by_header[header_id]`.
pub fn map_header_areas(
    arm9: &[u8],
    overworld_header_ids: &[u16],
    layout: MapHeaderLayout,
) -> Option<Vec<u8>> {
    if overworld_header_ids.is_empty() {
        return None;
    }
    let max_id = *overworld_header_ids.iter().max()? as usize;
    let stride = layout.stride;

    let entry = |base: usize, id: usize| -> Option<(u8, u16)> {
        let o = base + id * stride;
        let area = *arm9.get(o + layout.area_off)?;
        let mtx = u16::from_le_bytes([
            *arm9.get(o + layout.matrix_off)?,
            *arm9.get(o + layout.matrix_off + 1)?,
        ]);
        Some((area, mtx))
    };

    // A 4-aligned base where every overworld header maps to matrix 0 with a
    // valid (< 106) area is the table start.
    let mut base = 0usize;
    while base + (max_id + 1) * stride <= arm9.len() {
        let good = overworld_header_ids
            .iter()
            .all(|&id| matches!(entry(base, id as usize), Some((area, 0)) if area < 106));
        if good {
            let areas = (0..=max_id)
                .map(|id| entry(base, id).map(|(a, _)| a).unwrap_or(0))
                .collect();
            return Some(areas);
        }
        base += 4;
    }
    None
}

/// Extract the terrain NSBMD model embedded in a land-data chunk. The chunk
/// header is four u32 section sizes (attributes, buildings, model, BDHC); the
/// model section is a "BMD0" container.
pub fn terrain_model(chunk: &[u8]) -> Option<&[u8]> {
    if chunk.len() < 16 {
        return None;
    }
    let model_size = u32::from_le_bytes(chunk[8..12].try_into().ok()?) as usize;
    let off = chunk.windows(4).position(|w| w == b"BMD0")?;
    chunk.get(off..off + model_size)
}

/// A building/prop placed on a map: which build-model to draw and where.
/// Positions/rotations/scales are in world units (fx32 converted to float).
#[derive(Clone, Debug)]
pub struct Building {
    pub model_id: u16,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

/// Size of one on-disk building entry (`MapPropFile`): model id + three
/// `VecFx32`s (position, rotation, scale) + 8 bytes padding.
const BUILDING_ENTRY: usize = 48;

fn fx32(v: u32) -> f32 {
    (v as i32) as f32 / 4096.0
}

/// Parse the buildings placed on a land-data chunk. The building entries sit
/// immediately before the terrain model: in DPPt they fill the whole buildings
/// section, while HGSS prefixes them with a second permission grid. Both are
/// handled by walking fixed 48-byte entries backwards from the model, stopping
/// at the first slot that isn't a plausible entry.
pub fn parse_buildings(chunk: &[u8]) -> Vec<Building> {
    if chunk.len() < 16 {
        return Vec::new();
    }
    let attrs = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
    let lo = 16 + attrs;
    let Some(model_off) = chunk.windows(4).position(|w| w == b"BMD0") else {
        return Vec::new();
    };

    let u32a = |o: usize| u32::from_le_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
    let vec3 = |o: usize| [fx32(u32a(o)), fx32(u32a(o + 4)), fx32(u32a(o + 8))];

    let mut out = Vec::new();
    let mut off = model_off;
    while off >= lo + BUILDING_ENTRY {
        let e = off - BUILDING_ENTRY;
        let model_id = u32a(e) as i32;
        let scale = vec3(e + 28);
        // A real entry has a small model id and a sane, positive uniform-ish
        // scale; permission-grid bytes decode to a huge/negative id and stop us.
        let scale_ok = scale.iter().all(|&s| s > 0.0 && s <= 64.0);
        if !(0..4096).contains(&model_id) || !scale_ok {
            break;
        }
        out.push(Building {
            model_id: model_id as u16,
            position: vec3(e + 4),
            rotation: vec3(e + 16),
            scale,
        });
        off = e;
    }
    out.reverse();
    out
}
