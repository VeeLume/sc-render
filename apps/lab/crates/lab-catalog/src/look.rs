//! Character colours from the game's saved characters (`.chf`, in the
//! install's `user/client/0/customcharacters`): hair and brow dye, eye and
//! skin colour, keyed by the item they apply to.
//!
//! A `.chf` lists the character's items (a port tree of item GUIDs) and,
//! separately, material overrides by name. Nothing in the file links a
//! material to an item, so they pair by CIG's own naming (`hair_itemport` ↔
//! `HairDyeMaterial`); see [`PORT_MATERIALS`]. Parsing is StarBreaker's
//! `starbreaker-chf`, whose name tables turn the hashed names into strings.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use serde_json::Value;

use sc_render::PartLook;

use crate::{Error, Result};

/// A saved character file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CharacterFile {
    /// File name without `.chf`.
    pub name: String,
    pub path: String,
    /// Last modified, seconds since the Unix epoch.
    pub modified: f64,
}

/// A character's colour overrides, one entry per item that has any.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CharacterLook {
    pub parts: Vec<PartLook>,
}

/// Which `.chf` material colours the item on which port. The pairing is by
/// name because the file has no other link; unknown ports are skipped.
const PORT_MATERIALS: &[(&str, &str)] = &[
    ("body_itemport", "BodyMaterial"),
    ("head_itemport", "Head Material"),
    ("eyes_itemport", "EyeMaterial"),
    ("hair_itemport", "HairDyeMaterial"),
    ("eyebrow_itemport", "EyebrowDyeMaterial"),
    ("beard_itemport", "BeardDyeMaterial"),
];

/// The install's saved characters, newest first.
pub fn character_files() -> Result<Vec<CharacterFile>> {
    let install = sc_discovery::discover_primary().map_err(|e| Error::Discovery(e.to_string()))?;
    let dir = install.root.join("user/client/0/customcharacters");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<CharacterFile> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("chf")))
        .map(|p| CharacterFile {
            name: p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            modified: modified(&p),
            path: p.to_string_lossy().into_owned(),
        })
        .collect();
    files.sort_by(|a, b| b.modified.total_cmp(&a.modified));
    Ok(files)
}

fn modified(p: &Path) -> f64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Read a `.chf` file's colour overrides.
pub fn character_look(path: impl Into<PathBuf>) -> Result<CharacterLook> {
    let path = path.into();
    let bytes =
        std::fs::read(&path).map_err(|e| Error::Catalog(format!("{}: {e}", path.display())))?;
    let json = starbreaker_chf::chf_to_json(&bytes)
        .map_err(|e| Error::Catalog(format!("{}: {e}", path.display())))?;
    let v: Value = serde_json::from_str(&json).map_err(|e| Error::Catalog(e.to_string()))?;
    let mut items = Vec::new();
    collect_ports(&v["itemport"], &mut items);
    let parts = items
        .into_iter()
        .filter_map(|(port, item)| {
            let material = PORT_MATERIALS.iter().find(|(p, _)| *p == port)?.1;
            let m = v["materials"]
                .as_array()?
                .iter()
                .find(|m| m["name"].as_str() == Some(material))?;
            let subs = m["sub_materials"].as_array()?;
            let params = subs
                .iter()
                .flat_map(|s| s["material_params"].as_array().into_iter().flatten())
                .filter_map(|p| {
                    Some((p["name"].as_str()?.to_string(), p["value"].as_f64()? as f32))
                })
                .collect();
            let colors = subs
                .iter()
                .flat_map(|s| s["material_colors"].as_array().into_iter().flatten())
                .filter_map(|c| {
                    Some((
                        c["name"].as_str()?.to_string(),
                        hex_rgb(c["value"].as_str()?)?,
                    ))
                })
                .collect();
            Some(PartLook {
                item,
                material: material.to_string(),
                params,
                colors,
            })
        })
        .collect();
    Ok(CharacterLook { parts })
}

/// `(port name, item GUID)` for every node of the port tree.
fn collect_ports(node: &Value, out: &mut Vec<(String, String)>) {
    if let (Some(name), Some(id)) = (node["name"].as_str(), node["id"].as_str()) {
        out.push((name.to_string(), id.to_string()));
    }
    for c in node["children"].as_array().into_iter().flatten() {
        collect_ports(c, out);
    }
}

/// `#rrggbb` or `#rrggbbaa` to sRGB bytes.
fn hex_rgb(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours() {
        assert_eq!(hex_rgb("#d69b85ff"), Some([0xd6, 0x9b, 0x85]));
        assert_eq!(hex_rgb("#010203"), Some([1, 2, 3]));
        assert_eq!(hex_rgb("d69b85"), None);
    }
}
