//! What a ship view draws: [`ShipScene`], a vehicle and everything installed
//! on it, in the ship frame (x right, y forward, z up, metres). Built from
//! sc-holotable's assembled vehicle (`sc_vehicles::scene`), which takes any
//! loadout, so a host draws what the player has fitted and not only stock.
//! The scene needs no DataCore once the host holds its `Visuals` and
//! `Palettes`; the `.cdf` reader `sc_vehicles::scene` asks for can read from
//! [`Assets::source`](crate::Assets::source).

use sc_extract::Guid;
use sc_items_visuals::Scene;
use serde::Serialize;

use crate::ScenePart;
use crate::scene::note;

/// A ship to draw.
///
/// [`node_parts`](Self::node_parts) and [`interior`](Self::interior) are
/// optional: empty, the view has no per-damage-part colouring and no
/// interior.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShipScene {
    pub id: String,
    pub label: String,
    pub root: ScenePart,
    /// What could not be resolved (shown in the log).
    pub notes: Vec<String>,
    /// Hull mesh node → damage part, for colouring the drawn hull.
    pub node_parts: Vec<NodePart>,
    /// The interior's static geometry (walls, floors, fixtures).
    pub interior: Vec<InteriorBrush>,
}

/// A hull mesh node and the damage part it belongs to.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NodePart {
    pub node: String,
    pub part: String,
}

/// One placed interior geometry file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct InteriorBrush {
    pub geometry: String,
    /// Ship frame, 3 × 4 row-major.
    pub transform: [f32; 12],
}

impl ShipScene {
    /// The view of an assembled vehicle: `label` its title, `names` how the
    /// log names a record.
    ///
    /// ```no_run
    /// # fn demo(scene: &sc_items_visuals::Scene) {
    /// let ship = sc_render::ShipScene::new(scene, "Aurora MR".into(), |g| g.to_string());
    /// println!("{} attachments", ship.root.attachments.len());
    /// # }
    /// ```
    pub fn new(scene: &Scene, label: String, names: impl Fn(&Guid) -> String) -> Self {
        Self {
            id: scene.root.item.map(|g| g.to_string()).unwrap_or_default(),
            label,
            root: ScenePart::new(&scene.root, "", &names),
            notes: scene.notes.iter().map(|n| note(n, &names)).collect(),
            node_parts: Vec::new(),
            interior: Vec::new(),
        }
    }
}
