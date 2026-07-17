//! Recursive traversal of a ROM filesystem, descending into NARC archives.

use crate::narc::Narc;
use crate::rom::Rom;
use crate::signature::{detect_signature, is_narc, signature_extension, Signature};
use std::collections::HashMap;

const MAX_DEPTH: u32 = 8;

/// Totals reported after a recursive dump.
#[derive(Default, Debug)]
pub struct DumpStats {
    pub files: usize,
    pub containers: usize,
    pub bytes: u64,
}

fn id_paths(rom: &Rom) -> HashMap<u16, String> {
    rom.files().iter().map(|f| (f.id, f.path.clone())).collect()
}

/// Visit every leaf (non-container) file, descending into NARCs. `visit`
/// receives the logical path and the bytes.
pub fn walk_rom(rom: &Rom, expand: bool, visit: &mut dyn FnMut(&str, &[u8])) {
    let paths = id_paths(rom);
    for id in 0..rom.file_count() as u16 {
        let Ok(data) = rom.file_by_id(id) else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        let rel = match paths.get(&id) {
            Some(p) => {
                if !expand || !is_narc(data) {
                    let sig = detect_signature(data);
                    if sig != Signature::Unknown {
                        format!("{p}.{}", signature_extension(sig))
                    } else {
                        p.clone()
                    }
                } else {
                    p.clone()
                }
            }
            None => format!(
                "unnamed/{id:05}.{}",
                signature_extension(detect_signature(data))
            ),
        };
        walk_blob(data, &rel, 0, expand, visit);
    }
}

fn walk_blob(data: &[u8], rel: &str, depth: u32, expand: bool, visit: &mut dyn FnMut(&str, &[u8])) {
    if expand && depth < MAX_DEPTH && is_narc(data) {
        if let Ok(narc) = Narc::parse(data) {
            for i in 0..narc.file_count() {
                let Ok(child) = narc.file(i) else { continue };
                let child_rel = match narc.name(i) {
                    Some(n) => format!("{rel}/{n}"),
                    None => format!(
                        "{rel}/{i:05}.{}",
                        signature_extension(detect_signature(child))
                    ),
                };
                walk_blob(child, &child_rel, depth + 1, expand, visit);
            }
            return;
        }
    }
    visit(rel, data);
}

/// Visit every NARC archive in the ROM (including nested ones) with its parsed
/// contents — needed for whole-archive decoding like 2D sprite pairing.
pub fn walk_narcs(rom: &Rom, visit: &mut dyn FnMut(&str, &Narc)) {
    let paths = id_paths(rom);
    for id in 0..rom.file_count() as u16 {
        let Ok(data) = rom.file_by_id(id) else {
            continue;
        };
        if !is_narc(data) {
            continue;
        }
        let rel = paths
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("unnamed/{id:05}"));
        walk_narc_blob(data, &rel, 0, visit);
    }
}

fn walk_narc_blob(data: &[u8], rel: &str, depth: u32, visit: &mut dyn FnMut(&str, &Narc)) {
    if depth >= MAX_DEPTH || !is_narc(data) {
        return;
    }
    let Ok(narc) = Narc::parse(data) else { return };
    visit(rel, &narc);
    for i in 0..narc.file_count() {
        let Ok(child) = narc.file(i) else { continue };
        if is_narc(child) {
            let child_rel = match narc.name(i) {
                Some(n) => format!("{rel}/{n}"),
                None => format!("{rel}/{i:05}"),
            };
            walk_narc_blob(child, &child_rel, depth + 1, visit);
        }
    }
}
