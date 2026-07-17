//! Per-game mapping of the stripped `a/X/Y/Z` NARC paths to meaningful
//! names/roles, loaded from editable TOML files under `data/mappings/`.

use serde::Deserialize;
use std::sync::OnceLock;

/// A documented file, recovering the meaning the ROM's stripped FNT no longer
/// provides. `kind` is a free-form role slug (e.g. "pokemon-sprite",
/// "field-model") that drives output organisation and sprite decryption.
#[derive(Clone, Deserialize)]
pub struct KnownFile {
    pub path: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
struct GameEntry {
    id: String,
    code_prefix: String,
    display_name: String,
}

#[derive(Deserialize)]
struct MappingFile {
    #[serde(default)]
    games: Vec<GameEntry>,
    #[serde(default)]
    files: Vec<KnownFile>,
}

/// Known-file mapping for a single game (regional variants share one, matched on
/// the 3-char game code prefix).
pub struct GameManifest {
    pub id: String,
    pub code_prefix: String,
    pub display_name: String,
    pub files: Vec<KnownFile>,
}

impl GameManifest {
    pub fn find(&self, path: &str) -> Option<&KnownFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

const SOURCES: &[&str] = &[
    include_str!("../../../data/mappings/hgss.toml"),
    include_str!("../../../data/mappings/bw.toml"),
    include_str!("../../../data/mappings/b2w2.toml"),
];

fn registry() -> &'static Vec<GameManifest> {
    static REGISTRY: OnceLock<Vec<GameManifest>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut out = Vec::new();
        for src in SOURCES {
            let mf: MappingFile = match toml::from_str(src) {
                Ok(mf) => mf,
                Err(e) => {
                    eprintln!("warning: bad mapping file: {e}");
                    continue;
                }
            };
            for g in &mf.games {
                out.push(GameManifest {
                    id: g.id.clone(),
                    code_prefix: g.code_prefix.clone(),
                    display_name: g.display_name.clone(),
                    files: mf.files.clone(),
                });
            }
        }
        out
    })
}

/// Resolve the mapping for a 4-character game code (region byte ignored).
pub fn manifest_for_gamecode(gamecode: &str) -> Option<&'static GameManifest> {
    if gamecode.len() < 3 {
        return None;
    }
    let prefix = &gamecode[..3];
    registry().iter().find(|m| m.code_prefix == prefix)
}

/// All loaded game manifests.
pub fn all_manifests() -> &'static [GameManifest] {
    registry()
}
