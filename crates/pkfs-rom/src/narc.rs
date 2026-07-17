//! Nitro Archive (NARC): the container Gen 4/5 games use to bundle related
//! resources. Composed of BTAF (allocation), BTNF (optional names) and GMIF
//! (packed data) sub-blocks.

use crate::{str_at, u16_at, u32_at};
use anyhow::{bail, Result};

pub struct Narc<'a> {
    image: &'a [u8],
    entries: Vec<(u32, u32)>,
    names: Vec<Option<String>>,
}

impl<'a> Narc<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Narc<'a>> {
        if data.len() < 0x10 || &data[..4] != b"NARC" {
            bail!("not a NARC archive (bad magic)");
        }
        let num_blocks = u16_at(data, 0x0E)?;

        let mut entries = Vec::new();
        let mut image: Option<&[u8]> = None;
        let mut names_span: Option<(usize, usize)> = None;

        let mut offset = 0x10usize;
        for _ in 0..num_blocks {
            if offset + 8 > data.len() {
                break;
            }
            let magic = &data[offset..offset + 4];
            let block_size = u32_at(data, offset + 4)? as usize;
            if block_size < 8 || offset + block_size > data.len() {
                bail!("NARC block has an invalid size");
            }
            match magic {
                b"BTAF" => {
                    let file_count = u16_at(data, offset + 8)?;
                    let table = offset + 12;
                    for i in 0..file_count as usize {
                        entries.push((
                            u32_at(data, table + i * 8)?,
                            u32_at(data, table + i * 8 + 4)?,
                        ));
                    }
                }
                b"BTNF" => names_span = Some((offset + 8, block_size - 8)),
                b"GMIF" => image = Some(&data[offset + 8..offset + block_size]),
                _ => {}
            }
            offset += block_size;
        }

        let image = image.ok_or_else(|| anyhow::anyhow!("NARC missing GMIF block"))?;
        if entries.is_empty() && image.is_empty() {
            // Still valid (empty archive).
        }
        let names = names_span
            .and_then(|(off, size)| parse_names(data, off, size, entries.len()))
            .unwrap_or_default();

        Ok(Narc {
            image,
            entries,
            names,
        })
    }

    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    pub fn file(&self, index: usize) -> Result<&'a [u8]> {
        let (start, end) = *self
            .entries
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("NARC sub-file index out of range: {index}"))?;
        let (start, end) = (start as usize, end as usize);
        if start > self.image.len() || end > self.image.len() || end < start {
            bail!("NARC entry {index} points outside the image block");
        }
        Ok(&self.image[start..end])
    }

    pub fn name(&self, index: usize) -> Option<&str> {
        self.names.get(index).and_then(|n| n.as_deref())
    }
}

/// Reconstruct names for the common flat, single-directory NARC layout.
fn parse_names(
    data: &[u8],
    offset: usize,
    size: usize,
    count: usize,
) -> Option<Vec<Option<String>>> {
    if size < 8 {
        return None;
    }
    let root_subtable = u32_at(data, offset).ok()? as usize;
    if u16_at(data, offset + 6).ok()? != 1 {
        return None; // Nested dirs: don't guess.
    }
    let mut names = Vec::new();
    let mut pos = offset + root_subtable;
    while pos < data.len() {
        let control = data[pos];
        pos += 1;
        if control == 0 {
            break;
        }
        let name_len = (control & 0x7F) as usize;
        if control & 0x80 != 0 || pos + name_len > data.len() {
            return None;
        }
        names.push(Some(str_at(data, pos, name_len).ok()?));
        pos += name_len;
    }
    if names.len() == count {
        Some(names)
    } else {
        None
    }
}
