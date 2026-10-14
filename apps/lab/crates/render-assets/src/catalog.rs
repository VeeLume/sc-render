//! Items as render scenes, from the DataCore: [`Catalog`] parses it once and
//! turns an item (and what is attached to it) into a [`SceneItem`], the tree
//! the cell renderer draws. The recipe comes from sc-holotable
//! (`sc_items_visuals`); this module resolves `.cdf` assemblies and applies
//! the presentation choices that are ours, not the game's (the fallback pose).

use std::collections::HashMap;

use sc_extract::{
    AssetConfig, AssetData, AttachmentKind, CharacterDefinition, CrcIndex, Datacore, Guid,
    RecordCollection,
};
use sc_items::Items;
use sc_items_visuals::{InventoryView, LoadoutEntry, Palette, Palettes, Visual, Visuals};
use serde::{Deserialize, Serialize};

use crate::{Assets, Error, Result};

/// Pose for an item no inventory orientation entry matches: the game then
/// shows a prop unturned, i.e. its back (Bosco rack). We show the front
/// three-quarter instead, seen from slightly above (sc-item-render decision
/// "Item icons aim to be recognisable"). The elevation turns about the
/// screen's horizontal axis; a pitch in the offset would make the item lean.
pub const FALLBACK_OFFSET: [f32; 3] = [0.0, 205.0, 0.0];
pub const FALLBACK_ELEVATION: f32 = 8.0;

/// The character body an item is shown for. Armor and clothing select their
/// geometry variant by it (`Female` / `Male`), then by
/// `inventoryStoredEntity`: the item as stored, a static `_prop.cgf` instead
/// of the packaging crate. The game binds the tag at runtime from the
/// player's character; tags only pick variants that ask for them, so other
/// items are unaffected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Body {
    #[default]
    Female,
    Male,
}

impl Body {
    /// The geometry tags an inventory cell selects variants by.
    pub fn tags(self) -> &'static [&'static str] {
        match self {
            Body::Female => &["Female", "inventoryStoredEntity"],
            Body::Male => &["Male", "inventoryStoredEntity"],
        }
    }
}

/// Nesting limit for attached items (rack → rifle → scope is three).
const MAX_DEPTH: usize = 6;

/// Everything the scenes are built from.
pub struct Catalog {
    items: Items,
    visuals: Visuals,
    palettes: Palettes,
    view: InventoryView,
    crcs: CrcIndex,
    /// Display name, else record name, per item.
    labels: HashMap<Guid, String>,
    /// Record name (without the `EntityClassDefinition.` prefix) per item.
    records: HashMap<Guid, String>,
}

/// A search hit.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ItemSummary {
    pub guid: String,
    pub label: String,
    pub record: String,
    /// `Type/SubType`.
    pub kind: String,
}

/// An item to draw in one cell.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SceneItem {
    pub id: String,
    pub label: String,
    pub root: ScenePart,
    /// Category orientation offset, degrees (on top of the default 90, 0, 0).
    pub offset: [f32; 3],
    /// Extra viewing elevation, degrees (the fallback pose only).
    pub elevation: f32,
    pub rig: Vec<SceneLight>,
    /// Cell width / height.
    pub aspect: f32,
    /// What could not be resolved (shown in the log).
    pub notes: Vec<String>,
}

/// One item's geometry and what hangs on it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ScenePart {
    pub record: String,
    /// Drawn at the item origin.
    pub pieces: Vec<ScenePiece>,
    /// `.chr` whose bones the attachments name; without one they name
    /// geometry nodes of the pieces.
    pub skeleton: Option<String>,
    pub attachments: Vec<SceneAttachment>,
    pub palette: Option<[ScenePaletteEntry; 3]>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ScenePiece {
    pub geometry: String,
    /// `None`: the geometry's own `MtlName`.
    pub material: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SceneAttachment {
    /// Bone or geometry node.
    pub bone: String,
    pub part: ScenePart,
}

/// A palette entry, sRGB bytes; gloss 0..=255.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ScenePaletteEntry {
    pub tint: [u8; 3],
    pub spec: [u8; 3],
    pub gloss: u8,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SceneLight {
    pub name: String,
    pub pos: [f32; 3],
    /// `Ang3` degrees; projectors shine along local +X.
    pub rot: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
    pub fov: f32,
    pub casts_shadows: bool,
}

/// An attached item, as an inventory dump reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ChildSpec {
    pub port: String,
    pub guid: String,
    /// `None`: the item's own default loadout.
    pub children: Option<Vec<ChildSpec>>,
}

/// One item of an inventory dump, resolved.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct InventoryNode {
    pub geid: String,
    pub guid: Option<String>,
    pub label: String,
    /// The port on the parent, by name.
    pub port: Option<String>,
    pub attached: Vec<InventoryNode>,
    pub contents: Vec<InventoryNode>,
}

impl Catalog {
    /// Parse the DataCore and build the recipes (seconds in an optimised
    /// build).
    pub fn load(assets: &Assets) -> Result<Self> {
        let source = assets.source();
        let data = AssetData::extract(source, &AssetConfig::standard())?;
        let datacore = Datacore::parse(source, &data)?;
        let items = Items::build(datacore.records());
        let visuals = Visuals::build(&datacore, &items);
        let palettes = Palettes::build(&datacore);
        let view = InventoryView::build(&datacore);
        let crcs = CrcIndex::from_datacore(&datacore);
        let mut records = HashMap::new();
        for r in datacore.db().all_records() {
            if visuals.get(&r.id()).is_some() {
                let name = r.name().unwrap_or_default();
                let short = name.split_once('.').map_or(name, |(_, n)| n);
                records.insert(r.id(), short.to_string());
            }
        }
        let labels = records
            .iter()
            .map(|(g, record)| {
                let label = items
                    .get(g)
                    .and_then(|i| i.display_name(&data.locale))
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(record);
                (*g, label.to_string())
            })
            .collect();
        Ok(Self {
            items,
            visuals,
            palettes,
            view,
            crcs,
            labels,
            records,
        })
    }

    /// Every item with a recipe.
    pub fn all(&self) -> Vec<ItemSummary> {
        self.labels
            .iter()
            .map(|(g, l)| self.summary(g, l))
            .collect()
    }

    /// Items whose display or record name contains every word of `query`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ItemSummary> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<ItemSummary> = self
            .labels
            .iter()
            .filter(|(g, label)| {
                let record = self
                    .records
                    .get(*g)
                    .map(|r| r.to_lowercase())
                    .unwrap_or_default();
                let label = label.to_lowercase();
                words
                    .iter()
                    .all(|w| label.contains(w) || record.contains(w))
            })
            .map(|(g, label)| self.summary(g, label))
            .collect();
        hits.sort_by(|a, b| a.label.cmp(&b.label).then(a.record.cmp(&b.record)));
        hits.truncate(limit);
        hits
    }

    fn summary(&self, guid: &Guid, label: &str) -> ItemSummary {
        let kind = self.items.get(guid).map_or_else(String::new, |i| {
            format!(
                "{}/{}",
                i.item_type.as_dcb_str(),
                i.item_sub_type.as_dcb_str()
            )
        });
        ItemSummary {
            guid: guid.to_string(),
            label: label.to_string(),
            record: self.records.get(guid).cloned().unwrap_or_default(),
            kind,
        }
    }

    fn label(&self, guid: &Guid) -> String {
        self.labels
            .get(guid)
            .cloned()
            .unwrap_or_else(|| guid.to_string())
    }

    /// The scene for an item. `children` replaces its default loadout (an
    /// inventory dump's `attached`); `None` keeps the default.
    pub fn scene(
        &self,
        assets: &Assets,
        guid: &str,
        children: Option<&[ChildSpec]>,
        body: Body,
    ) -> Result<SceneItem> {
        let guid = parse_guid(guid)?;
        let item = self
            .items
            .get(&guid)
            .ok_or_else(|| Error::Catalog(format!("{guid} is not an item")))?;
        let visual = self
            .visuals
            .get(&guid)
            .ok_or_else(|| Error::Catalog(format!("{guid} has no geometry")))?;
        let mut notes = Vec::new();
        let root = self.part(assets, &guid, children, body, 0, &mut notes)?;
        let (offset, elevation) = match self.view.orientation(item, visual) {
            Some(o) => (o, 0.0),
            None => {
                notes.push("no inventory orientation entry: fallback pose".into());
                (FALLBACK_OFFSET, FALLBACK_ELEVATION)
            }
        };
        let rig = self
            .view
            .light_group(item, visual)
            .and_then(|g| self.view.rig(&g))
            .map(|r| {
                r.lights
                    .iter()
                    .filter(|l| l.active)
                    .map(|l| SceneLight {
                        name: l.name.trim_start_matches("SLightItem_").to_string(),
                        pos: l.position,
                        rot: l.rotation,
                        color: l.color,
                        intensity: l.intensity,
                        radius: l.radius,
                        fov: l.fov,
                        casts_shadows: l.casts_shadows,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let aspect = visual.grid.map_or(1.0, |[w, h]| w as f32 / h as f32);
        Ok(SceneItem {
            id: guid.to_string(),
            label: self.label(&guid),
            root,
            offset,
            elevation,
            rig,
            aspect,
            notes,
        })
    }

    fn part(
        &self,
        assets: &Assets,
        guid: &Guid,
        children: Option<&[ChildSpec]>,
        body: Body,
        depth: usize,
        notes: &mut Vec<String>,
    ) -> Result<ScenePart> {
        let record = self.records.get(guid).cloned().unwrap_or_default();
        let visual = self
            .visuals
            .get(guid)
            .ok_or_else(|| Error::Catalog(format!("{record}: no geometry")))?;
        let look = visual
            .look(body.tags())
            .ok_or_else(|| Error::Catalog(format!("{record}: no geometry path")))?;
        let palette = look
            .palette
            .as_ref()
            .and_then(|p| self.palettes.resolve(p))
            .map(palette_entries);

        let mut part = ScenePart {
            record: record.clone(),
            pieces: Vec::new(),
            skeleton: None,
            attachments: Vec::new(),
            palette,
        };
        if look.geometry.to_ascii_lowercase().ends_with(".cdf") {
            let cdf = CharacterDefinition::read(assets.source(), &look.geometry)?;
            // A skin's material override replaces the weapon's own material
            // (the one most of its attachments use), not a part's own.
            let own = most_common(cdf.attachments.iter().filter_map(|a| a.material.as_deref()));
            let material = |m: &Option<String>| match (&look.material, m) {
                (Some(skin), None) => Some(skin.clone()),
                (Some(skin), Some(m)) if Some(m.as_str()) == own => Some(skin.clone()),
                _ => m.clone(),
            };
            if cdf.model.to_ascii_lowercase().ends_with(".chr") {
                part.skeleton = Some(cdf.model.clone());
            } else {
                part.pieces.push(ScenePiece {
                    geometry: cdf.model.clone(),
                    material: look.material.clone(),
                });
            }
            for a in &cdf.attachments {
                let Some(binding) = &a.binding else { continue };
                let piece = ScenePiece {
                    geometry: binding.clone(),
                    material: material(&a.material),
                };
                match (&a.kind, &a.bone) {
                    (AttachmentKind::Skin, _) => part.pieces.push(piece),
                    (AttachmentKind::Bone, Some(bone)) => part.attachments.push(SceneAttachment {
                        bone: bone.clone(),
                        part: ScenePart {
                            record: format!("{record} / {}", a.name),
                            pieces: vec![piece],
                            skeleton: None,
                            attachments: Vec::new(),
                            palette: part.palette,
                        },
                    }),
                    _ => notes.push(format!(
                        "{record}: attachment {} ({:?}) skipped",
                        a.name, a.kind
                    )),
                }
            }
        } else {
            part.pieces.push(ScenePiece {
                geometry: look.geometry.clone(),
                material: look.material.clone(),
            });
        }

        if depth >= MAX_DEPTH {
            notes.push(format!(
                "{record}: nesting deeper than {MAX_DEPTH}, children dropped"
            ));
            return Ok(part);
        }
        let defaults;
        let children: &[ChildSpec] = match children {
            Some(c) => c,
            None => {
                defaults = default_children(&visual.default_loadout);
                &defaults
            }
        };
        for c in children {
            let child = match parse_guid(&c.guid) {
                Ok(g) if self.visuals.get(&g).is_some() => g,
                _ => continue, // not drawn (sensors, controllers)
            };
            let Some(helper) = visual.port(&c.port).and_then(|p| p.helper.clone()) else {
                notes.push(format!("{record}: no port {} with a helper", c.port));
                continue;
            };
            match self.part(
                assets,
                &child,
                c.children.as_deref(),
                body,
                depth + 1,
                notes,
            ) {
                Ok(p) => part.attachments.push(SceneAttachment {
                    bone: helper,
                    part: p,
                }),
                Err(e) => notes.push(format!("{record}: {} skipped: {e}", c.port)),
            }
        }
        Ok(part)
    }

    /// Resolve a Starlume `inventory.json` dump: class CRCs to items, port
    /// CRCs (crc32c of the lower-cased port name) to the parent's ports.
    pub fn inventory(&self, json: &str) -> Result<Vec<InventoryNode>> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| Error::Catalog(e.to_string()))?;
        let mut out = Vec::new();
        for stash in v["stashes"].as_array().into_iter().flatten() {
            for item in stash["items"].as_array().into_iter().flatten() {
                out.push(self.inventory_node(item, None));
            }
        }
        Ok(out)
    }

    fn inventory_node(&self, v: &serde_json::Value, parent: Option<&Visual>) -> InventoryNode {
        let guid = v["class_crc"]
            .as_u64()
            .and_then(|c| self.crcs.guid(c as u32));
        let port = v["port_crc"].as_u64().map(|crc| {
            parent
                .and_then(|p| {
                    p.ports.iter().find(|port| {
                        sc_extract::svarog_common::crc::hash_bytes(
                            port.name.to_ascii_lowercase().as_bytes(),
                        ) == crc as u32
                    })
                })
                .map_or_else(|| format!("crc {crc}"), |p| p.name.clone())
        });
        let visual = guid.as_ref().and_then(|g| self.visuals.get(g));
        let nodes = |key: &str, parent: Option<&Visual>| {
            v[key]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| self.inventory_node(c, parent))
                .collect()
        };
        InventoryNode {
            geid: v["geid"].to_string(),
            guid: guid.map(|g| g.to_string()),
            label: guid.map_or_else(
                || format!("unknown class {}", v["class_crc"]),
                |g| {
                    self.labels
                        .get(&g)
                        .cloned()
                        .unwrap_or_else(|| g.to_string())
                },
            ),
            port,
            attached: nodes("attached", visual),
            contents: nodes("contents", None),
        }
    }
}

fn parse_guid(s: &str) -> Result<Guid> {
    s.parse()
        .map_err(|_| Error::Catalog(format!("not a GUID: {s}")))
}

fn palette_entries(p: &Palette) -> [ScenePaletteEntry; 3] {
    p.entries.map(|e| ScenePaletteEntry {
        tint: e.tint,
        spec: e.spec,
        gloss: e.glossiness,
    })
}

fn default_children(loadout: &[LoadoutEntry]) -> Vec<ChildSpec> {
    loadout
        .iter()
        .filter_map(|e| {
            Some(ChildSpec {
                port: e.port.clone(),
                guid: e.item?.to_string(),
                children: (!e.children.is_empty()).then(|| default_children(&e.children)),
            })
        })
        .collect()
}

fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v)
}
