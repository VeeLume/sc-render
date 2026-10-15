//! Items as render scenes, from the DataCore: [`Catalog`] parses it once and
//! turns an item (and what is attached to it) into a [`SceneItem`], the tree
//! the cell renderer draws. The assembly is sc-holotable's
//! (`Visuals::scene`), the cell sc-render's; this module adds the
//! presentation choices that are ours, not the game's (the fallback poses),
//! and the names the log shows.

use std::collections::HashMap;

use sc_extract::{
    AssetConfig, AssetData, CharacterDefinition, CrcIndex, Datacore, Guid, RecordCollection,
};
use sc_items::Items;
use sc_items_visuals::{Attached, InventoryView, Palettes, Visual, Visuals};
use sc_render::{Assets, Fallback, Pose, SceneItem};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Poses for what the inventory view has no orientation entry for.
///
/// An item: the game shows a prop unturned, i.e. its back (Bosco rack). We
/// show the front three-quarter instead, seen from slightly above
/// (sc-item-render decision "Item icons aim to be recognisable").
///
/// A character (an actor: a bare `.chr` with the body skinned on): facing
/// the viewer, turned a little, seen at eye level. From above, a standing
/// figure looks as if it leaned back.
pub const FALLBACK: Fallback = Fallback {
    item: Pose {
        offset: [0.0, 205.0, 0.0],
        elevation: 8.0,
    },
    character: Pose {
        offset: [0.0, 200.0, 0.0],
        elevation: 0.0,
    },
};

/// The character body an item is shown for (armor and clothing pick their
/// variant by it); `sc_items_visuals::Body` with the app's IPC types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Body {
    #[default]
    Female,
    Male,
}

impl From<Body> for sc_items_visuals::Body {
    fn from(b: Body) -> Self {
        match b {
            Body::Female => Self::Female,
            Body::Male => Self::Male,
        }
    }
}

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

    /// Every item with a recipe, by label then record name.
    pub fn all(&self) -> Vec<ItemSummary> {
        let mut all: Vec<ItemSummary> = self
            .labels
            .iter()
            .map(|(g, l)| self.summary(g, l))
            .collect();
        all.sort_by(|a, b| a.label.cmp(&b.label).then(a.record.cmp(&b.record)));
        all
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
        let attached = children.map(attached);
        let scene = self
            .visuals
            .scene(
                &self.palettes,
                &guid,
                attached.as_deref(),
                body.into(),
                |path| CharacterDefinition::read(assets.source(), path).map_err(|e| e.to_string()),
            )
            .ok_or_else(|| Error::Catalog(format!("{guid}: no geometry path")))?;
        Ok(SceneItem::new(
            &scene,
            item,
            visual,
            &self.view,
            &FALLBACK,
            self.label(&guid),
            |g| {
                self.records
                    .get(g)
                    .cloned()
                    .unwrap_or_else(|| g.to_string())
            },
        ))
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

/// An inventory dump's attachments as holotable's; entries whose GUID does
/// not parse are left out.
fn attached(children: &[ChildSpec]) -> Vec<Attached> {
    children
        .iter()
        .filter_map(|c| {
            Some(Attached {
                port: c.port.clone(),
                item: c.guid.parse().ok()?,
                children: c.children.as_deref().map(attached),
            })
        })
        .collect()
}
