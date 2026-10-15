//! Star Citizen render inputs, read from the local install and shaped for a
//! GPU renderer: models as one packed vertex/index buffer plus a description
//! ([`ModelInfo`]), materials as plain data ([`MaterialInfo`]), textures as
//! decoded RGBA8 ([`Rgba`]).
//!
//! Nothing here ships game data: everything is read from the user's own
//! `Data.p4k` at runtime.

mod catalog;
mod look;
mod model;
mod texture;

pub use catalog::{
    Body, Catalog, ChildSpec, FALLBACK_ELEVATION, FALLBACK_OFFSET, InventoryNode, ItemSummary,
    SceneAttachment, SceneItem, SceneLight, ScenePaletteEntry, ScenePart, ScenePiece,
};
pub use look::{CharacterFile, CharacterLook, PartLook, character_files, character_look};
pub use model::{Model, ModelInfo, NodeInfo, SubmeshInfo};
pub use texture::{Mip, Rgba, TextureKind, Volume};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sc_extract::{AssetSource, Material};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Star Citizen install found: {0}")]
    Discovery(String),
    #[error(transparent)]
    Extract(#[from] sc_extract::Error),
    #[error("texture decode failed: {0}")]
    Decode(String),
    #[error("{0}")]
    Catalog(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// An open install plus decode caches.
pub struct Assets {
    source: AssetSource,
    textures: Mutex<HashMap<(String, TextureKind, u32), Arc<Rgba>>>,
}

impl Assets {
    /// Open the primary install (LIVE first).
    pub fn open() -> Result<Self> {
        let install =
            sc_discovery::discover_primary().map_err(|e| Error::Discovery(e.to_string()))?;
        Ok(Self {
            source: AssetSource::from_install(&install)?,
            textures: Mutex::default(),
        })
    }

    /// The install's archive.
    pub fn source(&self) -> &AssetSource {
        &self.source
    }

    /// A geometry file (`.cgf` / `.cga` / `.skin`) packed for upload.
    pub fn model(&self, path: &str) -> Result<Model> {
        let g = sc_extract::MeshGeometry::read(&self.source, path)?;
        Ok(Model::pack(path, &g))
    }

    /// A `.chr` skeleton's bones, model space.
    pub fn skeleton(&self, path: &str) -> Result<Vec<BoneInfo>> {
        let s = sc_extract::Skeleton::read(&self.source, path)?;
        Ok(s.bones
            .into_iter()
            .map(|b| BoneInfo {
                name: b.name,
                parent: b.parent.map(|p| p as u32),
                position: b.position,
                rotation: b.rotation,
            })
            .collect())
    }

    /// A `.mtl` file (authored path, extension optional).
    pub fn material(&self, path: &str) -> Result<MaterialInfo> {
        let path = if path.to_ascii_lowercase().ends_with(".mtl") {
            path.to_string()
        } else {
            format!("{path}.mtl")
        };
        let m = Material::read(&self.source, &path)?;
        Ok(MaterialInfo::from_material(&path, m))
    }

    /// A texture decoded to RGBA8, its largest mip no wider or taller than
    /// `max_size`.
    pub fn texture(&self, path: &str, kind: TextureKind, max_size: u32) -> Result<Arc<Rgba>> {
        let key = (path.to_ascii_lowercase(), kind, max_size);
        if let Some(hit) = self.textures.lock().expect("cache lock").get(&key) {
            return Ok(hit.clone());
        }
        let t = sc_extract::Texture::read(&self.source, path)?;
        let rgba = Arc::new(texture::decode(&t, kind, max_size)?);
        self.textures
            .lock()
            .expect("cache lock")
            .insert(key, rgba.clone());
        Ok(rgba)
    }

    /// The LayerBlend hue-shift lookup table (8 × 8 × 16 RGB): two 8³ colour
    /// cubes stacked in depth, the first for positive, the second for
    /// negative shifts. The HueAoLum map's `.x` blends towards it.
    pub fn hue_lut(&self) -> Result<Volume> {
        let bytes = self.source.read(HUE_LUT)?;
        texture::decode_volume(&bytes)
    }
}

const HUE_LUT: &str = r"Engine\EngineAssets\Textures\layerblendHueLUT.dds";

/// A skeleton bone in the model's space.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BoneInfo {
    pub name: String,
    pub parent: Option<u32>,
    pub position: [f32; 3],
    /// Quaternion `(x, y, z, w)`.
    pub rotation: [f32; 4],
}

/// A `.mtl` file as the renderer consumes it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MaterialInfo {
    pub path: String,
    pub submaterials: Vec<SubMaterialInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SubMaterialInfo {
    pub name: String,
    pub shader: String,
    pub features: Vec<String>,
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub opacity: f32,
    pub shininess: f32,
    pub alpha_test: f32,
    pub textures: Vec<TextureSlot>,
    pub layers: Vec<LayerInfo>,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextureSlot {
    pub slot: String,
    pub file: String,
    pub tiling: [f32; 2],
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LayerInfo {
    pub name: String,
    pub path: String,
    pub submaterial: String,
    pub tint: [f32; 3],
    pub wear_tint: [f32; 3],
    pub gloss_mult: f32,
    pub wear_gloss: f32,
    pub uv_tiling: f32,
    pub palette_tint: u8,
}

impl MaterialInfo {
    fn from_material(path: &str, m: Material) -> Self {
        Self {
            path: path.to_string(),
            submaterials: m
                .submaterials
                .into_iter()
                .map(|s| SubMaterialInfo {
                    name: s.name,
                    shader: s.shader,
                    features: s.features,
                    diffuse: s.diffuse,
                    specular: s.specular,
                    emissive: s.emissive,
                    opacity: s.opacity,
                    shininess: s.shininess,
                    alpha_test: s.alpha_test,
                    textures: s
                        .textures
                        .into_iter()
                        .map(|t| TextureSlot {
                            slot: t.slot,
                            file: t.file,
                            tiling: t.tiling,
                        })
                        .collect(),
                    layers: s
                        .layers
                        .into_iter()
                        .map(|l| LayerInfo {
                            name: l.name,
                            path: l.path,
                            submaterial: l.submaterial,
                            tint: l.tint,
                            wear_tint: l.wear_tint,
                            gloss_mult: l.gloss_mult,
                            wear_gloss: l.wear_gloss,
                            uv_tiling: l.uv_tiling,
                            palette_tint: l.palette_tint,
                        })
                        .collect(),
                    params: s.params,
                })
                .collect(),
        }
    }
}
