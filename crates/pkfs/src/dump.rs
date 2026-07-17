//! Full asset dump: models -> GLB, textures/sprites -> PNG, unknown NARC
//! sub-files -> .bin. Output collapses to a single file when a source yields
//! one asset, nesting into a directory only when there are several.

use anyhow::Result;
use pkfs_2d::{
    assemble_cell, decrypt_char_data, parse_ncer, parse_ncgr, parse_nclr, render_ncgr_sheet, Image,
    Ncer, Ncgr, Nclr, SpriteCipher,
};
use pkfs_rom::signature::Signature;
use pkfs_rom::{detect_signature, manifest_for_gamecode, walk_narcs, Narc, Rom};
use std::collections::HashSet;
use std::path::Path;

/// Totals reported after a dump.
#[derive(Default, Debug, Clone, Copy)]
pub struct DumpReport {
    pub models: usize,
    pub textures: usize,
    pub sprites: usize,
    pub bins: usize,
}

/// Dump every recognised asset in `rom` to `out`.
pub fn dump_rom(rom: &Rom, out: &Path, raw: bool) -> Result<DumpReport> {
    let out = out.to_path_buf();
    let gamecode = rom.header().gamecode.clone();
    let mut r = DumpReport::default();

    walk_narcs(rom, &mut |path, narc| {
        let base = out.join(path);
        let cipher = sprite_cipher(&gamecode, path);
        if let Err(e) = process_narc(narc, &base, raw, cipher, &mut r) {
            eprintln!("warning: {path}: {e}");
        }
    });

    // Top-level nitro files that are not inside a NARC.
    let paths: std::collections::HashMap<u16, String> =
        rom.files().iter().map(|f| (f.id, f.path.clone())).collect();
    for id in 0..rom.file_count() as u16 {
        let Ok(data) = rom.file_by_id(id) else {
            continue;
        };
        if data.is_empty() || pkfs_rom::signature::is_narc(data) {
            continue;
        }
        if !matches!(detect_signature(data), Signature::Nsbmd | Signature::Nsbtx) {
            continue;
        }
        let rel = paths
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("unnamed/{id:05}"));
        let base = out.join(strip_ext(&rel));
        let _ = process_nitro3d(&[data.to_vec()], &base, &mut r);
    }
    Ok(r)
}

fn process_narc(
    narc: &Narc,
    base: &Path,
    raw: bool,
    cipher: SpriteCipher,
    r: &mut DumpReport,
) -> Result<()> {
    let mut nitro3d = Vec::new();
    let mut has_2d = false;
    let mut has_nsbtx = false;
    for i in 0..narc.file_count() {
        let Ok(f) = narc.file(i) else { continue };
        match detect_signature(f) {
            Signature::Nsbmd
            | Signature::Nsbca
            | Signature::Nsbtp
            | Signature::Nsbta
            | Signature::Nsbma
            | Signature::Nsbva => nitro3d.push(f.to_vec()),
            Signature::Nsbtx => {
                nitro3d.push(f.to_vec());
                has_nsbtx = true;
            }
            Signature::Nclr | Signature::Ncgr | Signature::Ncer => has_2d = true,
            Signature::Narc => {}
            Signature::Unknown => {
                write_file(&base.join(format!("{i:05}.bin")), f);
                r.bins += 1;
            }
            _ => {}
        }
    }

    if !nitro3d.is_empty() {
        let models = process_nitro3d(&nitro3d, base, r)?;
        if models == 0 && has_nsbtx {
            let pngs = pkfs_nitro::buffers_to_texture_pngs(nitro3d)?;
            let items: Vec<(String, Vec<u8>)> =
                pngs.into_iter().map(|n| (n.name, n.bytes)).collect();
            r.textures += write_group(&items, base, "png");
        }
    }
    if has_2d {
        let images = collect_sprites(narc, raw, cipher);
        r.sprites += write_images(&images, base);
    }
    Ok(())
}

fn process_nitro3d(buffers: &[Vec<u8>], base: &Path, r: &mut DumpReport) -> Result<usize> {
    let glbs = pkfs_nitro::buffers_to_glbs(buffers.to_vec())?;
    let items: Vec<(String, Vec<u8>)> = glbs.into_iter().map(|n| (n.name, n.bytes)).collect();
    let n = write_group(&items, base, "glb");
    r.models += n;
    Ok(n)
}

fn nearest<T>(items: &[(usize, T)], to: usize) -> Option<&T> {
    items
        .iter()
        .min_by_key(|(i, _)| if *i > to { *i - to } else { to - *i })
        .map(|(_, v)| v)
}

fn preceding_or_nearest<T>(items: &[(usize, T)], to: usize) -> Option<&T> {
    items
        .iter()
        .filter(|(i, _)| *i <= to)
        .max_by_key(|(i, _)| *i)
        .map(|(_, v)| v)
        .or_else(|| nearest(items, to))
}

/// Decode the 2D sprites in a NARC into named images (without writing).
pub fn collect_sprites(narc: &Narc, raw: bool, cipher: SpriteCipher) -> Vec<(String, Image)> {
    let mut pals: Vec<(usize, Nclr)> = Vec::new();
    let mut chars: Vec<(usize, Ncgr)> = Vec::new();
    let mut cells: Vec<(usize, Ncer)> = Vec::new();
    for i in 0..narc.file_count() {
        let Ok(f) = narc.file(i) else { continue };
        match detect_signature(f) {
            Signature::Nclr => {
                if let Ok(p) = parse_nclr(f) {
                    pals.push((i, p));
                }
            }
            Signature::Ncgr => {
                if let Ok(mut g) = parse_ncgr(f) {
                    decrypt_char_data(&mut g.char_data, cipher);
                    chars.push((i, g));
                }
            }
            Signature::Ncer => {
                if let Ok(c) = parse_ncer(f) {
                    cells.push((i, c));
                }
            }
            _ => {}
        }
    }
    if pals.is_empty() || chars.is_empty() {
        return Vec::new();
    }

    let mut images: Vec<(String, Image)> = Vec::new();

    if !raw && !cells.is_empty() {
        for (ci, ncer) in &cells {
            let Some(ncgr) = preceding_or_nearest(&chars, *ci) else {
                continue;
            };
            let Some(pal) = nearest(&pals, *ci) else {
                continue;
            };
            for (c, cell) in ncer.cells.iter().enumerate() {
                let img = assemble_cell(cell, ncgr, pal, ncer.mapping_type);
                if !img.is_empty() {
                    images.push((format!("{ci:05}_cell{c:03}"), img));
                }
            }
        }
        if !images.is_empty() {
            return images;
        }
    }

    if cipher != SpriteCipher::None && !raw {
        let mut ci = 0;
        let mut pi = 0;
        while ci < chars.len() {
            let group_start = ci;
            while ci < chars.len() && (pi >= pals.len() || chars[ci].0 < pals[pi].0) {
                ci += 1;
            }
            let pal_start = pi;
            while pi < pals.len() && (ci >= chars.len() || pals[pi].0 < chars[ci].0) {
                pi += 1;
            }
            let pal_count = pi - pal_start;
            if pal_count == 0 {
                continue;
            }
            for g in group_start..ci {
                let slot = (g - group_start) % pal_count;
                let suffix = if slot != 0 { "_shiny" } else { "" };
                let img = render_ncgr_sheet(&chars[g].1, &pals[pal_start + slot].1, 0, true);
                images.push((format!("{:05}{suffix}", chars[g].0), img));
            }
        }
        return images;
    }

    for (idx, ncgr) in &chars {
        let pal = pals
            .iter()
            .filter(|(i, _)| *i >= *idx)
            .min_by_key(|(i, _)| *i)
            .map(|(_, v)| v)
            .or_else(|| nearest(&pals, *idx));
        if let Some(pal) = pal {
            images.push((format!("{idx:05}"), render_ncgr_sheet(ncgr, pal, 0, true)));
        }
    }
    images
}

fn write_images(images: &[(String, Image)], base: &Path) -> usize {
    let items: Vec<(String, Vec<u8>)> = images
        .iter()
        .map(|(n, img)| (n.clone(), encode_png(img)))
        .collect();
    write_group(&items, base, "png")
}

/// Encode an [`Image`] to PNG bytes.
pub fn encode_png(img: &Image) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, img.width.max(1), img.height.max(1));
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(&img.data);
        }
    }
    buf
}

fn write_group(items: &[(String, Vec<u8>)], base: &Path, ext: &str) -> usize {
    match items.len() {
        0 => 0,
        1 => {
            let mut p = base.to_path_buf();
            p.set_extension(ext);
            write_file(&p, &items[0].1);
            1
        }
        _ => {
            let mut seen = HashSet::new();
            for (name, bytes) in items {
                let mut n = sanitize(name);
                while !seen.insert(n.clone()) {
                    n.push('_');
                }
                write_file(&base.join(format!("{n}.{ext}")), bytes);
            }
            items.len()
        }
    }
}

fn write_file(path: &Path, data: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, data) {
        eprintln!("warning: failed to write {}: {e}", path.display());
    }
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "unnamed".into()
    } else {
        s
    }
}

fn strip_ext(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => path.to_string(),
    }
}

/// In-memory assets decoded at a single ROM path.
#[derive(Default)]
pub struct Assets {
    /// (name, self-contained GLB bytes).
    pub models: Vec<(String, Vec<u8>)>,
    /// (name, PNG bytes) — textures and sprites.
    pub images: Vec<(String, Vec<u8>)>,
}

/// Decode every asset at `path` (a NARC, or a standalone Nitro file) in memory.
pub fn assets_at_path(rom: &Rom, path: &str, raw: bool) -> Assets {
    let mut a = Assets::default();
    let Ok(data) = rom.file_by_path(path) else {
        return a;
    };

    if pkfs_rom::signature::is_narc(data) {
        let Ok(narc) = Narc::parse(data) else {
            return a;
        };
        let mut nitro3d = Vec::new();
        let mut has_nsbtx = false;
        for i in 0..narc.file_count() {
            let Ok(f) = narc.file(i) else { continue };
            match detect_signature(f) {
                Signature::Nsbmd
                | Signature::Nsbca
                | Signature::Nsbtp
                | Signature::Nsbta
                | Signature::Nsbma
                | Signature::Nsbva => nitro3d.push(f.to_vec()),
                Signature::Nsbtx => {
                    nitro3d.push(f.to_vec());
                    has_nsbtx = true;
                }
                _ => {}
            }
        }
        if !nitro3d.is_empty() {
            if let Ok(glbs) = pkfs_nitro::buffers_to_glbs(nitro3d.clone()) {
                a.models = glbs.into_iter().map(|n| (n.name, n.bytes)).collect();
            }
            if a.models.is_empty() && has_nsbtx {
                if let Ok(pngs) = pkfs_nitro::buffers_to_texture_pngs(nitro3d) {
                    a.images.extend(pngs.into_iter().map(|n| (n.name, n.bytes)));
                }
            }
        }
        let cipher = sprite_cipher(&rom.header().gamecode, path);
        for (name, img) in collect_sprites(&narc, raw, cipher) {
            a.images.push((name, encode_png(&img)));
        }
    } else if matches!(detect_signature(data), Signature::Nsbmd | Signature::Nsbtx) {
        if let Ok(glbs) = pkfs_nitro::buffers_to_glbs(vec![data.to_vec()]) {
            a.models = glbs.into_iter().map(|n| (n.name, n.bytes)).collect();
        }
        if a.models.is_empty() {
            if let Ok(pngs) = pkfs_nitro::buffers_to_texture_pngs(vec![data.to_vec()]) {
                a.images.extend(pngs.into_iter().map(|n| (n.name, n.bytes)));
            }
        }
    }
    a
}

fn sprite_cipher(gamecode: &str, path: &str) -> SpriteCipher {
    let known_sprite = manifest_for_gamecode(gamecode)
        .and_then(|m| m.find(path))
        .map(|kf| kf.kind == "pokemon-sprite")
        .unwrap_or(false);
    if !(known_sprite || path.contains("pokegra")) {
        return SpriteCipher::None;
    }
    match &gamecode[..gamecode.len().min(3)] {
        "ADA" | "APA" => SpriteCipher::BackToFront,
        _ => SpriteCipher::FrontToBack,
    }
}
