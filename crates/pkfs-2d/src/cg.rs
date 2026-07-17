//! NCLR palettes and NCGR character (tile) graphics.

use crate::{extend5to8, find_ntr_block, u16le, u32le, Image};
use anyhow::{anyhow, Result};

/// A decoded NCLR palette, split into fixed-size sub-palettes.
pub struct Nclr {
    pub colors_per_palette: usize,
    pub colors: Vec<[u8; 4]>,
}

impl Nclr {
    pub fn color(&self, bank: usize, index: usize) -> [u8; 4] {
        self.colors
            .get(bank * self.colors_per_palette + index)
            .copied()
            .unwrap_or([0, 0, 0, 0])
    }
}

fn bgr555(c: u16) -> [u8; 4] {
    [
        extend5to8((c & 0x1F) as u8),
        extend5to8(((c >> 5) & 0x1F) as u8),
        extend5to8(((c >> 10) & 0x1F) as u8),
        255,
    ]
}

pub fn parse_nclr(file: &[u8]) -> Result<Nclr> {
    let block = find_ntr_block(file, b"TTLP").ok_or_else(|| anyhow!("NCLR: no TTLP block"))?;
    let depth = u32le(block, 0x08);
    let colors_per_palette = if depth == 4 { 256 } else { 16 };

    let data_off = 0x18;
    let mut data_len = block.len().saturating_sub(data_off);
    let declared = u32le(block, 0x10) as usize;
    if declared > 0 && declared <= data_len {
        data_len = declared;
    }
    let count = data_len / 2;
    let mut colors = Vec::with_capacity(count);
    for i in 0..count {
        colors.push(bgr555(u16le(block, data_off + i * 2)));
    }
    Ok(Nclr {
        colors_per_palette,
        colors,
    })
}

/// A decoded NCGR character set, keeping the raw tiled data so NCER assembly can
/// address individual tiles by number.
pub struct Ncgr {
    pub bit_depth: u8, // 4 or 8
    pub scanned: bool,
    pub tiles_x: i32,
    pub tiles_y: i32,
    pub char_data: Vec<u8>,
}

impl Ncgr {
    pub fn bytes_per_tile(&self) -> usize {
        if self.bit_depth == 4 {
            32
        } else {
            64
        }
    }

    /// Palette index of pixel (px, py) within 8x8 tile `tile`.
    pub fn tile_pixel(&self, tile: usize, px: u32, py: u32) -> u8 {
        if self.bit_depth == 4 {
            let at = tile * 32 + (py as usize) * 4 + (px as usize) / 2;
            match self.char_data.get(at) {
                Some(&b) if px & 1 == 1 => b >> 4,
                Some(&b) => b & 0xF,
                None => 0,
            }
        } else {
            let at = tile * 64 + (py as usize) * 8 + px as usize;
            self.char_data.get(at).copied().unwrap_or(0)
        }
    }
}

pub fn parse_ncgr(file: &[u8]) -> Result<Ncgr> {
    let block = find_ntr_block(file, b"RAHC").ok_or_else(|| anyhow!("NCGR: no RAHC block"))?;
    let tiles_y = u16le(block, 0x08) as i16 as i32;
    let tiles_x = u16le(block, 0x0A) as i16 as i32;
    let bit_depth = if u32le(block, 0x0C) == 3 { 4 } else { 8 };
    let scanned = block[0x14] != 0;

    let data_off = 0x20;
    let data_size = (u32le(block, 0x18) as usize).min(block.len() - data_off);
    let char_data = block[data_off..data_off + data_size].to_vec();
    Ok(Ncgr {
        bit_depth,
        scanned,
        tiles_x,
        tiles_y,
        char_data,
    })
}

/// The XOR-LCG cipher the games apply to pokegra sprite tiles. Diamond/Pearl
/// encode back-to-front; Platinum/HGSS (and Gen 5) front-to-back.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SpriteCipher {
    None,
    FrontToBack,
    BackToFront,
}

pub fn decrypt_char_data(d: &mut [u8], cipher: SpriteCipher) {
    if cipher == SpriteCipher::None || d.len() < 2 {
        return;
    }
    let n = d.len() & !1;
    let rd = |d: &[u8], i: usize| -> u16 { u16::from_le_bytes([d[i], d[i + 1]]) };
    let mut key: u32;
    match cipher {
        SpriteCipher::FrontToBack => {
            key = rd(d, 0) as u32;
            let mut i = 0;
            while i + 1 < n {
                let v = rd(d, i) ^ (key & 0xFFFF) as u16;
                d[i..i + 2].copy_from_slice(&v.to_le_bytes());
                key = key.wrapping_mul(1103515245).wrapping_add(24691);
                i += 2;
            }
        }
        SpriteCipher::BackToFront => {
            key = rd(d, n - 2) as u32;
            let mut i = n;
            while i >= 2 {
                let v = rd(d, i - 2) ^ (key & 0xFFFF) as u16;
                d[i - 2..i].copy_from_slice(&v.to_le_bytes());
                key = key.wrapping_mul(1103515245).wrapping_add(24691);
                i -= 2;
            }
        }
        SpriteCipher::None => {}
    }
}

/// Render an NCGR as a flat tile sheet. A near-square layout is used when the
/// header does not declare a width.
pub fn render_ncgr_sheet(
    ncgr: &Ncgr,
    nclr: &Nclr,
    palette_bank: usize,
    color0_transparent: bool,
) -> Image {
    let bytes_per_tile = ncgr.bytes_per_tile();
    let px_per_byte = if ncgr.bit_depth == 4 { 2 } else { 1 };
    let num_tiles = ncgr.char_data.len() / bytes_per_tile;

    let mut tw = ncgr.tiles_x;
    let mut th = ncgr.tiles_y;
    if tw <= 0 {
        tw = ((num_tiles as f64).sqrt().round() as i32).max(1);
    }
    if th <= 0 {
        th = num_tiles.div_ceil(tw as usize) as i32;
    }
    let width = (tw as u32) * 8;
    let height = (th as u32) * 8;
    let mut img = Image::new(width, height);

    let emit = |img: &mut Image, x: u32, y: u32, idx: u8| {
        let rgba = if idx == 0 && color0_transparent {
            [0, 0, 0, 0]
        } else if ncgr.bit_depth == 4 {
            nclr.color(palette_bank, idx as usize)
        } else {
            nclr.color(0, idx as usize)
        };
        img.set(x, y, rgba);
    };

    if ncgr.scanned {
        for (i, &byte) in ncgr.char_data.iter().enumerate() {
            let base = i * px_per_byte;
            if ncgr.bit_depth == 4 {
                emit(
                    &mut img,
                    (base as u32) % width,
                    (base as u32) / width,
                    byte & 0xF,
                );
                emit(
                    &mut img,
                    ((base + 1) as u32) % width,
                    ((base + 1) as u32) / width,
                    byte >> 4,
                );
            } else {
                emit(&mut img, (base as u32) % width, (base as u32) / width, byte);
            }
        }
        return img;
    }

    for t in 0..num_tiles {
        let tile_x = (t % tw as usize) as u32;
        let tile_y = (t / tw as usize) as u32;
        for py in 0..8 {
            for px in 0..8 {
                emit(
                    &mut img,
                    tile_x * 8 + px,
                    tile_y * 8 + py,
                    ncgr.tile_pixel(t, px, py),
                );
            }
        }
    }
    img
}
