//! Nitro model/texture/animation decoding, vendored from apicula (0BSD) and
//! exposed as a library. The CLI, OpenGL viewer, ROM stamp-scanner and logger
//! from upstream are omitted; file ingestion is byte-based (see
//! [`db::Database::from_buffers`]) so callers drive it with their own ROM/NARC
//! filesystem.

#![recursion_limit = "128"]
// Vendored from apicula; keep upstream code intact rather than pruning every
// unused helper or chasing its lint style.
#![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::all)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate json;

#[macro_use]
pub mod errors;
#[macro_use]
pub mod util;
pub mod connection;
pub mod convert;
pub mod db;
pub mod decompress;
pub mod nds;
pub mod nitro;
pub mod primitives;
pub mod skeleton;

use crate::connection::{Connection, ConnectionOptions};
use crate::convert::gltf;
use crate::convert::image_namer::ImageNamer;
use crate::db::Database;
use crate::errors::Result;

/// A converted asset: its name plus the encoded bytes.
pub struct Named {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Convert every model found in the given Nitro file buffers (an NSBMD blob and
/// any sibling texture/animation files) to binary glTF (`.glb`). Textures and
/// animations found in the same buffers are bound to the models. Returns one
/// entry (name + GLB bytes) per model that has geometry.
pub fn buffers_to_glbs(buffers: Vec<Vec<u8>>) -> anyhow::Result<Vec<Named>> {
    convert_glbs(buffers).map_err(|e| anyhow::anyhow!("{e}"))
}

fn convert_glbs(buffers: Vec<Vec<u8>>) -> Result<Vec<Named>> {
    let db = Database::from_buffers(buffers);
    let conn = Connection::build(&db, ConnectionOptions::default());
    let image_namer = ImageNamer::build(&db, &conn);

    let mut out = Vec::new();
    for model_id in 0..db.models.len() {
        let gltf = gltf::to_gltf(&db, &conn, &image_namer, model_id);
        let mut bytes = Vec::new();
        gltf.write_glb(&mut bytes)?;
        if bytes.is_empty() {
            continue;
        }
        out.push(Named {
            name: format!("{}", db.models[model_id].name.print_safe()),
            bytes,
        });
    }
    Ok(out)
}

/// Decode every texture found in the given buffers to a PNG, pairing each with a
/// palette by name. Returns (image name, PNG bytes) pairs.
pub fn buffers_to_texture_pngs(buffers: Vec<Vec<u8>>) -> anyhow::Result<Vec<Named>> {
    convert_texture_pngs(buffers).map_err(|e| anyhow::anyhow!("{e}"))
}

fn convert_texture_pngs(buffers: Vec<Vec<u8>>) -> Result<Vec<Named>> {
    let db = Database::from_buffers(buffers);
    let conn = Connection::build(&db, ConnectionOptions::default());
    let mut image_namer = ImageNamer::build(&db, &conn);
    image_namer.add_more_images(&db);

    let mut out = Vec::new();
    for ((texture_id, palette_id), image_name) in image_namer.names.iter() {
        let texture = &db.textures[*texture_id];
        let palette = palette_id.map(|id| &db.palettes[id]);
        let rgba = match crate::nds::decode_texture(texture, palette) {
            Ok(rgba) => rgba,
            Err(_) => continue,
        };
        let (w, h) = (texture.params.width(), texture.params.height());
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = encoder.write_header() {
                if writer.write_image_data(&rgba.0).is_err() {
                    continue;
                }
            } else {
                continue;
            }
        }
        out.push(Named {
            name: image_name.clone(),
            bytes: png,
        });
    }
    Ok(out)
}
