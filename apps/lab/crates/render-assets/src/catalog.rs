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

    /// The tags of an item worn on a body: the body-fitted variant, not the
    /// stored prop.
    pub fn worn_tags(self) -> &'static [&'static str] {
        match self {
            Body::Female => &["Female"],
            Body::Male => &["Male"],
        }
    }
}

/// How a part is shown: for which body, and whether it is worn (skinned
/// onto a body, directly or through a parent) and so needs the body-fitted
/// geometry rather than the stored prop.
#[derive(Debug, Clone, Copy)]
struct Fit {
    body: Body,
    worn: bool,
}

impl Fit {
    fn tags(self) -> &'static [&'static str] {
        if self.worn {
            self.body.worn_tags()
        } else {
            self.body.tags()
        }
    }
}

/// Give every skinned part its parent's skeleton when it has none of its
/// own: a worn armor core's ports name the body's bones
/// (`magazine_attach_1_override`), and the body takes the player actor's.
fn share_skeleton(part: &mut ScenePart, inherited: Option<&str>) {
    if part.skeleton.is_none() {
        part.skeleton = inherited.map(str::to_string);
    }
    let skeleton = part.skeleton.clone();
    for a in &mut part.attachments {
        // Only skinned children (empty bone) share the skeleton.
        let pass = if a.bone.is_empty() {
            skeleton.as_deref()
        } else {
            None
        };
        share_skeleton(&mut a.part, pass);
    }
}

/// Nesting limit for attached items (player → body → armor core → backpack
/// → rifle → scope is five).
const MAX_DEPTH: usize = 10;

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
    /// Bone or geometry node; empty for a part skinned to this one's
    /// skeleton, which sits at its origin (a body's head, a head's hair).
    pub bone: String,
    /// Offset from the bone: position, then rotation as a quaternion
    /// `(x, y, z, w)` (a port helper's `Offset`, a `.cdf` part's
    /// `RelPosition` / `RelRotation`).
    pub offset_position: [f32; 3],
    pub offset_rotation: [f32; 4],
    /// The attached item's own helper (bone or node) that meets the bone,
    /// instead of its origin (`ItemOffsetHelperName`).
    pub item_helper: Option<String>,
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
    /// Record name (`Player`, `body_01_noMagicPocket`); empty when the
    /// class is unknown.
    pub record: String,
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
        let fit = Fit { body, worn: false };
        let mut root = self.part(assets, &guid, children, fit, 0, &mut notes)?;
        share_skeleton(&mut root, None);
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
        fit: Fit,
        depth: usize,
        notes: &mut Vec<String>,
    ) -> Result<ScenePart> {
        let record = self.records.get(guid).cloned().unwrap_or_default();
        let visual = self
            .visuals
            .get(guid)
            .ok_or_else(|| Error::Catalog(format!("{record}: no geometry")))?;
        let look = visual
            .look(fit.tags())
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
            // A skin's material override replaces the weapon's own material,
            // not a part's own. The weapon's own parts carry a material from the .cdf's folder;
            // a part from elsewhere (a cartridge from `ammo/`) keeps its own,
            // as does a part without one (its geometry's MtlName).
            let folder = parent_folder(&look.geometry);
            let material = |m: &Option<String>| match (&look.material, m) {
                (Some(skin), Some(m)) if parent_folder(m) == folder => Some(skin.clone()),
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
                        offset_position: a.position,
                        // `.cdf` RelRotation is (w, x, y, z).
                        offset_rotation: [
                            a.rotation[1],
                            a.rotation[2],
                            a.rotation[3],
                            a.rotation[0],
                        ],
                        item_helper: None,
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
        } else if look.geometry.to_ascii_lowercase().ends_with(".chr") {
            // A bare skeleton (the player actor): nothing to draw itself;
            // its skinned children (the body) take its bones.
            part.skeleton = Some(look.geometry.clone());
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
            // A skinned child shares this item's skeleton: in bind pose it
            // sits at this item's origin (empty bone).
            let Some((port, helper)) = visual.port(&c.port).and_then(|p| {
                let helper = if p.skinned {
                    String::new()
                } else {
                    p.helper.clone()?
                };
                Some((p, helper))
            }) else {
                notes.push(format!("{record}: no port {} with a helper", c.port));
                continue;
            };
            match self.part(
                assets,
                &child,
                c.children.as_deref(),
                Fit {
                    worn: fit.worn || port.skinned,
                    ..fit
                },
                depth + 1,
                notes,
            ) {
                Ok(p) => part.attachments.push(SceneAttachment {
                    bone: helper,
                    offset_position: port.offset.position,
                    offset_rotation: ang3_quaternion(port.offset.rotation),
                    item_helper: port.item_offset_helper.clone(),
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

    /// The player's character from a Starlume `inventory.json` dump: the
    /// item at the top of the stash whose context is the player (the actor,
    /// with the body and everything worn attached). `None` when the dump has
    /// no such stash.
    pub fn character(&self, json: &str) -> Result<Option<InventoryNode>> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| Error::Catalog(e.to_string()))?;
        Ok(v["stashes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["location"]["context"]["kind"] == "Player")
            .flat_map(|s| s["items"].as_array().into_iter().flatten())
            .next()
            .map(|item| self.inventory_node(item, None)))
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
            record: guid
                .and_then(|g| self.records.get(&g).cloned())
                .unwrap_or_default(),
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

/// CryEngine `Ang3` degrees (rotation = Rz · Ry · Rx) as a quaternion
/// `(x, y, z, w)`.
fn ang3_quaternion([x, y, z]: [f32; 3]) -> [f32; 4] {
    let (sx, cx) = (x.to_radians() / 2.0).sin_cos();
    let (sy, cy) = (y.to_radians() / 2.0).sin_cos();
    let (sz, cz) = (z.to_radians() / 2.0).sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

/// A p4k path's folder, lower-cased, without a `Data/` prefix.
fn parent_folder(path: &str) -> String {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let p = p.strip_prefix("data/").unwrap_or(&p);
    p.rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_string())
}
