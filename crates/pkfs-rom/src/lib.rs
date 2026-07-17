//! Nintendo DS ROM filesystem, NARC archives, format detection and per-game
//! manifests for Gen 4/5 Pokemon games.

pub mod blz;
pub mod extract;
pub mod map;
pub mod mapping;
pub mod narc;
pub mod rom;
pub mod signature;

pub use extract::{walk_narcs, walk_rom, DumpStats};
pub use mapping::{all_manifests, manifest_for_gamecode, GameManifest, KnownFile};
pub use narc::Narc;
pub use rom::{Rom, RomFile, RomHeader};
pub use signature::{detect_signature, is_narc, signature_extension, signature_label, Signature};

/// Little-endian scalar reads with bounds checking.
pub(crate) fn u8_at(d: &[u8], at: usize) -> anyhow::Result<u8> {
    d.get(at)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("read past end at {at}"))
}

pub(crate) fn u16_at(d: &[u8], at: usize) -> anyhow::Result<u16> {
    let b = d
        .get(at..at + 2)
        .ok_or_else(|| anyhow::anyhow!("u16 read past end at {at}"))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_at(d: &[u8], at: usize) -> anyhow::Result<u32> {
    let b = d
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("u32 read past end at {at}"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A fixed-length, NUL-padded ASCII string.
pub(crate) fn str_at(d: &[u8], at: usize, len: usize) -> anyhow::Result<String> {
    let b = d
        .get(at..at + len)
        .ok_or_else(|| anyhow::anyhow!("string read past end at {at}"))?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(len);
    Ok(String::from_utf8_lossy(&b[..end]).into_owned())
}
