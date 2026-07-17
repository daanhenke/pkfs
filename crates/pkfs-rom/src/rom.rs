//! Nintendo DS ROM image: header, File Allocation Table and reconstructed File
//! Name Table.

use crate::{str_at, u16_at, u32_at, u8_at};
use anyhow::{bail, Result};
use std::path::Path;

/// Selected fields from the DS cartridge header.
#[derive(Clone, Debug, Default)]
pub struct RomHeader {
    pub title: String,
    pub gamecode: String,
    pub makercode: String,
    pub unit_code: u8,
    pub rom_size_used: u32,
    pub fnt_offset: u32,
    pub fnt_size: u32,
    pub fat_offset: u32,
    pub fat_size: u32,
    pub arm9_offset: u32,
    pub arm9_size: u32,
}

#[derive(Copy, Clone)]
struct FatEntry {
    start: u32,
    end: u32,
}

/// A file discovered in the ROM filesystem: numeric id plus reconstructed path.
#[derive(Clone, Debug)]
pub struct RomFile {
    pub id: u16,
    pub path: String,
}

/// A parsed DS ROM with access to its internal filesystem.
pub struct Rom {
    data: Vec<u8>,
    header: RomHeader,
    fat: Vec<FatEntry>,
    files: Vec<RomFile>,
}

const HEADER_SIZE: usize = 0x200;

impl Rom {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Rom> {
        Rom::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Rom> {
        let header = parse_header(&data)?;
        let fat = parse_fat(&data, &header);
        let files = parse_fnt(&data, &header);
        Ok(Rom {
            data,
            header,
            fat,
            files,
        })
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }
    pub fn file_count(&self) -> usize {
        self.fat.len()
    }
    pub fn files(&self) -> &[RomFile] {
        &self.files
    }

    /// The raw (possibly BLZ-compressed) arm9 binary.
    pub fn arm9(&self) -> &[u8] {
        let start = self.header.arm9_offset as usize;
        let end = start.saturating_add(self.header.arm9_size as usize);
        self.data
            .get(start..end.min(self.data.len()))
            .unwrap_or(&[])
    }

    pub fn file_by_id(&self, id: u16) -> Result<&[u8]> {
        let e = self
            .fat
            .get(id as usize)
            .ok_or_else(|| anyhow::anyhow!("file id out of range: {id}"))?;
        let (start, end) = (e.start as usize, e.end as usize);
        if start > self.data.len() || end > self.data.len() || end < start {
            bail!("FAT entry {id} points outside the ROM");
        }
        Ok(&self.data[start..end])
    }

    pub fn id_for_path(&self, path: &str) -> Option<u16> {
        self.files.iter().find(|f| f.path == path).map(|f| f.id)
    }

    pub fn file_by_path(&self, path: &str) -> Result<&[u8]> {
        let id = self
            .id_for_path(path)
            .ok_or_else(|| anyhow::anyhow!("no such file in ROM: {path}"))?;
        self.file_by_id(id)
    }
}

fn parse_header(data: &[u8]) -> Result<RomHeader> {
    if data.len() < HEADER_SIZE {
        bail!("file is too small to be a Nintendo DS ROM");
    }
    let h = RomHeader {
        title: str_at(data, 0x00, 12)?,
        gamecode: str_at(data, 0x0C, 4)?,
        makercode: str_at(data, 0x10, 2)?,
        unit_code: u8_at(data, 0x12)?,
        fnt_offset: u32_at(data, 0x40)?,
        fnt_size: u32_at(data, 0x44)?,
        fat_offset: u32_at(data, 0x48)?,
        fat_size: u32_at(data, 0x4C)?,
        rom_size_used: u32_at(data, 0x80)?,
        arm9_offset: u32_at(data, 0x20)?,
        arm9_size: u32_at(data, 0x2C)?,
    };
    let fat_ok = h.fat_offset as usize <= data.len()
        && h.fat_size as usize <= data.len() - h.fat_offset as usize;
    if !fat_ok || !h.fat_size.is_multiple_of(8) {
        bail!("ROM header has an invalid File Allocation Table");
    }
    Ok(h)
}

fn parse_fat(data: &[u8], h: &RomHeader) -> Vec<FatEntry> {
    let count = (h.fat_size / 8) as usize;
    let mut fat = Vec::with_capacity(count);
    for i in 0..count {
        let base = h.fat_offset as usize + i * 8;
        fat.push(FatEntry {
            start: u32_at(data, base).unwrap_or(0),
            end: u32_at(data, base + 4).unwrap_or(0),
        });
    }
    fat
}

#[derive(Default, Clone)]
struct DirRecord {
    subtable_offset: u32,
    first_file_id: u16,
    name: String,
    parent: i32,
}

fn parse_fnt(data: &[u8], h: &RomHeader) -> Vec<RomFile> {
    let fnt = h.fnt_offset as usize;
    if h.fnt_size < 8 || fnt > data.len() || h.fnt_size as usize > data.len() - fnt {
        return Vec::new(); // No usable name table; files accessible by id.
    }
    let total_dirs = match u16_at(data, fnt + 6) {
        Ok(n) if n != 0 && (n as usize) * 8 <= h.fnt_size as usize => n,
        _ => return Vec::new(),
    };

    let mut dirs = vec![
        DirRecord {
            parent: -1,
            ..Default::default()
        };
        total_dirs as usize
    ];
    for (i, dir) in dirs.iter_mut().enumerate() {
        let base = fnt + i * 8;
        dir.subtable_offset = u32_at(data, base).unwrap_or(0);
        dir.first_file_id = u16_at(data, base + 4).unwrap_or(0);
    }

    // First pass: establish sub-directory names and parentage.
    for i in 0..total_dirs as usize {
        let mut pos = fnt + dirs[i].subtable_offset as usize;
        while pos < data.len() {
            let control = data[pos];
            pos += 1;
            if control == 0 {
                break;
            }
            let name_len = (control & 0x7F) as usize;
            let is_dir = control & 0x80 != 0;
            if pos + name_len > data.len() {
                break;
            }
            let name = str_at(data, pos, name_len).unwrap_or_default();
            pos += name_len;
            if !is_dir {
                continue;
            }
            if pos + 2 > data.len() {
                break;
            }
            let sub_id = u16_at(data, pos).unwrap_or(0);
            pos += 2;
            let idx = (sub_id & 0x0FFF) as usize;
            if idx < dirs.len() {
                dirs[idx].name = name;
                dirs[idx].parent = i as i32;
            }
        }
    }

    let dir_path = |index: i32| -> String {
        let mut path = String::new();
        let mut cur = index;
        while cur > 0 && (cur as usize) < dirs.len() {
            let d = &dirs[cur as usize];
            path = if path.is_empty() {
                d.name.clone()
            } else {
                format!("{}/{}", d.name, path)
            };
            cur = d.parent;
        }
        path
    };

    // Second pass: emit every named file with its full path.
    let mut files = Vec::new();
    for (i, dir) in dirs.iter().enumerate() {
        let prefix = dir_path(i as i32);
        let mut pos = fnt + dir.subtable_offset as usize;
        let mut file_id = dir.first_file_id;
        while pos < data.len() {
            let control = data[pos];
            pos += 1;
            if control == 0 {
                break;
            }
            let name_len = (control & 0x7F) as usize;
            let is_dir = control & 0x80 != 0;
            if pos + name_len > data.len() {
                break;
            }
            let name = str_at(data, pos, name_len).unwrap_or_default();
            pos += name_len + if is_dir { 2 } else { 0 };
            if is_dir {
                continue;
            }
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            files.push(RomFile { id: file_id, path });
            file_id += 1;
        }
    }

    files.sort_by_key(|f| f.id);
    files
}
