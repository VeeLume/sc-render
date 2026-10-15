//! What a cell draws: [`SceneItem`], an item and what hangs on it, posed and
//! lit the way the game's inventory shows it. Built from sc-holotable's
//! assembled item (`Visuals::scene`) and its inventory view; the host chooses
//! how to pose what the view has no orientation for ([`Fallback`]) and how
//! records are named in the log.

use sc_extract::Guid;
use sc_items::Item;
use sc_items_visuals::{InventoryView, Palette, Part, Scene, SceneNote, Visual};
use serde::Serialize;

/// An item to draw in one cell.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SceneItem {
    pub id: String,
    pub label: String,
    pub root: ScenePart,
    /// Category orientation offset, degrees (on top of the default 90, 0, 0).
    pub offset: [f32; 3],
    /// Extra viewing elevation, degrees (fallback poses only).
    pub elevation: f32,
    pub rig: Vec<SceneLight>,
    /// Cell width / height.
    pub aspect: f32,
    /// The inventory cell in grid units (width, height), when the item has one.
    pub grid: Option<[u32; 2]>,
    /// What could not be resolved (shown in the log).
    pub notes: Vec<String>,
}

/// One item's geometry and what hangs on it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ScenePart {
    /// The name the log shows.
    pub record: String,
    /// The item's record GUID; empty for a `.cdf` part that is no item.
    pub guid: String,
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

/// Colour overrides for one item of a character (a saved character's hair
/// dye, eye and skin colour), applied by the materials that read them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PartLook {
    /// The item's record GUID (as in [`ScenePart::guid`]).
    pub item: String,
    /// The material the values come from (`HairDyeMaterial`).
    pub material: String,
    /// Named numbers (`DyeAmount`, `BaseMelanin`).
    pub params: Vec<(String, f32)>,
    /// Named colours, sRGB bytes (`HairDyeColor1`, `EyeColor`).
    pub colors: Vec<(String, [u8; 3])>,
}

/// A pose: the orientation offset on top of the default (90, 0, 0), and a
/// viewing elevation that turns the posed item about the screen's horizontal
/// axis (a pitch in the offset would make the item lean instead).
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    pub offset: [f32; 3],
    pub elevation: f32,
}

/// The poses for items the inventory view has no orientation entry for. The
/// game then shows them unturned; how to show them instead is the host's
/// choice.
#[derive(Debug, Clone, Copy)]
pub struct Fallback {
    pub item: Pose,
    /// An actor (a bare `.chr` with the body skinned on).
    pub character: Pose,
}

impl SceneItem {
    /// The cell for an assembled item: `item` and `visual` are the root's,
    /// `label` its title, `names` how the log names a record.
    pub fn new(
        scene: &Scene,
        item: &Item,
        visual: &Visual,
        view: &InventoryView,
        fallback: &Fallback,
        label: String,
        names: impl Fn(&Guid) -> String,
    ) -> Self {
        let mut notes: Vec<String> = scene.notes.iter().map(|n| note(n, &names)).collect();
        let pose = match view.orientation(item, visual) {
            Some(offset) => Pose {
                offset,
                elevation: 0.0,
            },
            None if scene.is_character() => fallback.character,
            None => {
                notes.push("no inventory orientation entry: fallback pose".into());
                fallback.item
            }
        };
        let rig = view
            .light_group(item, visual)
            .and_then(|g| view.rig(&g))
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
        Self {
            id: scene.root.item.map(|g| g.to_string()).unwrap_or_default(),
            label,
            root: ScenePart::new(&scene.root, "", &names),
            offset: pose.offset,
            elevation: pose.elevation,
            rig,
            aspect: visual.grid.map_or(1.0, |[w, h]| w as f32 / h as f32),
            grid: visual.grid,
            notes,
        }
    }
}

impl ScenePart {
    /// The renderer's view of a part: names for the log, palette as bytes,
    /// a skinned attachment's bone empty.
    fn new(part: &Part, parent: &str, names: &impl Fn(&Guid) -> String) -> Self {
        let record = match (&part.item, &part.cdf_attachment) {
            (Some(g), _) => names(g),
            (None, Some(a)) => format!("{parent} / {a}"),
            (None, None) => parent.to_string(),
        };
        Self {
            guid: part.item.map(|g| g.to_string()).unwrap_or_default(),
            pieces: part
                .pieces
                .iter()
                .map(|p| ScenePiece {
                    geometry: p.geometry.clone(),
                    material: p.material.clone(),
                })
                .collect(),
            skeleton: part.skeleton.clone(),
            attachments: part
                .attachments
                .iter()
                .map(|a| SceneAttachment {
                    bone: a.bone.clone().unwrap_or_default(),
                    offset_position: a.offset.position,
                    offset_rotation: a.offset.rotation,
                    item_helper: a.item_helper.clone(),
                    part: Self::new(&a.part, &record, names),
                })
                .collect(),
            palette: part.palette.as_ref().map(palette_entries),
            record,
        }
    }
}

fn note(n: &SceneNote, name: &impl Fn(&Guid) -> String) -> String {
    match n {
        SceneNote::NoGeometry { item } => format!("{}: no geometry for these tags", name(item)),
        SceneNote::CdfUnreadable { path, reason } => format!("{path}: {reason}"),
        SceneNote::CdfAttachmentSkipped { path, attachment } => {
            format!("{path}: attachment {attachment} skipped")
        }
        SceneNote::NoPort { item, port } => {
            format!("{}: no port {port} with a helper", name(item))
        }
        SceneNote::TooDeep { item } => {
            format!("{}: nested too deep, children dropped", name(item))
        }
        other => format!("{other:?}"),
    }
}

fn palette_entries(p: &Palette) -> [ScenePaletteEntry; 3] {
    p.entries.map(|e| ScenePaletteEntry {
        tint: e.tint,
        spec: e.spec,
        gloss: e.glossiness,
    })
}
