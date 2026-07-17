//! NCER cell banks: OAM assembly of NCGR tiles into sprites.

use crate::cg::{Ncgr, Nclr};
use crate::{find_ntr_block, u16le, Image};
use anyhow::{anyhow, Result};

/// One OAM object: a block of tiles placed at a signed offset, optionally flipped.
pub struct Oam {
    pub x: i32,
    pub y: i32,
    pub width_tiles: u32,
    pub height_tiles: u32,
    pub char_name: u32,
    pub palette: u32,
    pub h_flip: bool,
    pub v_flip: bool,
    pub color256: bool,
}

pub struct NcerCell {
    pub oams: Vec<Oam>,
    pub bounds: Option<(i32, i32, i32, i32)>, // min_x, min_y, max_x, max_y
}

pub struct Ncer {
    pub mapping_type: u32,
    pub cells: Vec<NcerCell>,
}

fn oam_dimensions(shape: u32, size: u32) -> (u32, u32) {
    // [shape][size] -> (width, height) in tiles.
    const DIMS: [[(u32, u32); 4]; 3] = [
        [(1, 1), (2, 2), (4, 4), (8, 8)], // square
        [(2, 1), (4, 1), (4, 2), (8, 4)], // wide
        [(1, 2), (1, 4), (2, 4), (4, 8)], // tall
    ];
    if shape > 2 || size > 3 {
        (1, 1)
    } else {
        DIMS[shape as usize][size as usize]
    }
}

pub fn parse_ncer(file: &[u8]) -> Result<Ncer> {
    let block = find_ntr_block(file, b"KBEC").ok_or_else(|| anyhow!("NCER: no KBEC block"))?;
    let cell_count = u16le(block, 0x08) as usize;
    let extended = block[0x0A] == 1;
    let cell_size = if extended { 0x10 } else { 0x08 };
    let mapping_type = block[0x10] as u32;

    let cells_at = 0x20;
    let oam_base = cells_at + cell_count * cell_size;
    let mut oam_cursor = oam_base;

    let mut cells = Vec::with_capacity(cell_count);
    for i in 0..cell_count {
        let base = cells_at + i * cell_size;
        let oam_count = u16le(block, base) as usize;
        let bounds = if extended {
            Some((
                u16le(block, base + 12) as i16 as i32,
                u16le(block, base + 14) as i16 as i32,
                u16le(block, base + 8) as i16 as i32,
                u16le(block, base + 10) as i16 as i32,
            ))
        } else {
            None
        };

        let mut oams = Vec::with_capacity(oam_count);
        for j in 0..oam_count {
            let o = oam_cursor + j * 6;
            if o + 6 > block.len() {
                break;
            }
            let a0 = u16le(block, o);
            let a1 = u16le(block, o + 2);
            let a2 = u16le(block, o + 4);

            let y = (a0 & 0xFF) as u8 as i8 as i32;
            let color256 = (a0 >> 13) & 1 != 0;
            let shape = ((a0 >> 14) & 3) as u32;
            let x9 = (a1 & 0x1FF) as i32;
            let x = if x9 & 0x100 != 0 { x9 - 0x200 } else { x9 };
            let h_flip = (a1 >> 12) & 1 != 0;
            let v_flip = (a1 >> 13) & 1 != 0;
            let size = ((a1 >> 14) & 3) as u32;
            let char_name = (a2 & 0x3FF) as u32;
            let palette = ((a2 >> 12) & 0xF) as u32;
            let (w, h) = oam_dimensions(shape, size);
            oams.push(Oam {
                x,
                y,
                width_tiles: w,
                height_tiles: h,
                char_name,
                palette,
                h_flip,
                v_flip,
                color256,
            });
        }
        oam_cursor += oam_count * 6;
        cells.push(NcerCell { oams, bounds });
    }
    Ok(Ncer {
        mapping_type,
        cells,
    })
}

/// Assemble one cell into a sprite image using the character set and palette.
pub fn assemble_cell(cell: &NcerCell, ncgr: &Ncgr, nclr: &Nclr, mapping_type: u32) -> Image {
    if cell.oams.is_empty() {
        return Image::new(0, 0);
    }
    let (min_x, min_y, max_x, max_y) = cell.bounds.unwrap_or_else(|| {
        let mut mnx = i32::MAX;
        let mut mny = i32::MAX;
        let mut mxx = i32::MIN;
        let mut mxy = i32::MIN;
        for o in &cell.oams {
            mnx = mnx.min(o.x);
            mny = mny.min(o.y);
            mxx = mxx.max(o.x + o.width_tiles as i32 * 8);
            mxy = mxy.max(o.y + o.height_tiles as i32 * 8);
        }
        (mnx, mny, mxx, mxy)
    });

    let width = max_x - min_x;
    let height = max_y - min_y;
    if width <= 0 || height <= 0 {
        return Image::new(0, 0);
    }
    let mut img = Image::new(width as u32, height as u32);

    let boundary = 32usize << mapping_type;
    for oam in &cell.oams {
        let start_tile = (oam.char_name as usize * boundary) / ncgr.bytes_per_tile();
        for ty in 0..oam.height_tiles {
            for tx in 0..oam.width_tiles {
                let tile = start_tile + (ty * oam.width_tiles + tx) as usize;
                for py in 0..8 {
                    for px in 0..8 {
                        let idx = ncgr.tile_pixel(tile, px, py);
                        if idx == 0 {
                            continue; // colour 0 transparent for sprites
                        }
                        let mut lx = tx * 8 + px;
                        let mut ly = ty * 8 + py;
                        if oam.h_flip {
                            lx = oam.width_tiles * 8 - 1 - lx;
                        }
                        if oam.v_flip {
                            ly = oam.height_tiles * 8 - 1 - ly;
                        }
                        let dx = oam.x - min_x + lx as i32;
                        let dy = oam.y - min_y + ly as i32;
                        if dx < 0 || dy < 0 || dx >= width || dy >= height {
                            continue;
                        }
                        let rgba = if oam.color256 {
                            nclr.color(0, idx as usize)
                        } else {
                            nclr.color(oam.palette as usize, idx as usize)
                        };
                        img.set(dx as u32, dy as u32, rgba);
                    }
                }
            }
        }
    }
    img
}
