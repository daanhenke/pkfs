//! Nitro 2D graphics decoding for Gen 4/5 Pokemon: NCLR palettes, NCGR tiles
//! (with pokegra XOR decryption), and NCER cell assembly into sprites.

mod cg;
mod ncer;

pub use cg::{
    decrypt_char_data, parse_ncgr, parse_nclr, render_ncgr_sheet, Ncgr, Nclr, SpriteCipher,
};
pub use ncer::{assemble_cell, parse_ncer, Ncer};

/// A decoded RGBA8 image (row-major, top-down).
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // width * height * 4
}

impl Image {
    pub fn new(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            data: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
            self.data[i..i + 4].copy_from_slice(&rgba);
        }
    }
}

/// Locate a block by stamp in a 2D NTR file. These have no offset table: blocks
/// follow the 16-byte header back-to-back, each carrying its own size.
pub(crate) fn find_ntr_block<'a>(file: &'a [u8], stamp: &[u8; 4]) -> Option<&'a [u8]> {
    let mut off = 0x10usize;
    while off + 8 <= file.len() {
        let size = u32::from_le_bytes(file[off + 4..off + 8].try_into().ok()?) as usize;
        if size < 8 || off + size > file.len() {
            break;
        }
        if &file[off..off + 4] == stamp {
            return Some(&file[off..off + size]);
        }
        off += size;
    }
    None
}

pub(crate) fn u16le(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}
pub(crate) fn u32le(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}
pub(crate) fn extend5to8(x: u8) -> u8 {
    (x << 3) | (x >> 2)
}
