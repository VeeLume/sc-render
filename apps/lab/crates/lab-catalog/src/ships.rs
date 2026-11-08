//! Ships for the Ships page: what to draw ([`ShipScene`]) plus the damage
//! tooling (damage parts, collision shells, component boxes) and the hit
//! queries (`Target::hit`, `Target::exposure`). The model is sc-holotable's;
//! this module only shapes it for IPC. The tooling is the lab's, not the
//! library's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sc_extract::{AssetSource, Datacore, Guid, LocaleMap, RecordCollection};
use sc_items::Items;
use sc_items_ship_components::{PerDamageType, ShipComponents};
use sc_render::{InteriorBrush, NodePart, ShipScene};
use sc_vehicles::{
    ComponentHealths, Condition, DamageRules, Defense, GeometryPaths, HullGeometry, Interior,
    PortContainers, Raycaster, Shot, Target, Vehicle, Vehicles,
};
use sc_weapons::{ShipWeapon, Weapons};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A ship in the list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShipSummary {
    pub guid: String,
    pub label: String,
    pub record: String,
    /// `Base` or `Edition` (the player-ship listing).
    pub listing: String,
}

/// A ship's stock loadout, drawable, with its damage tooling. Positions are
/// in the ship frame (x right, y forward, z up, metres).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HangarShip {
    pub scene: ShipScene,
    /// Damage parts with their HP and collision shell (what shots hit).
    pub parts: Vec<HullPart>,
    /// Mounted items with their boxes.
    pub components: Vec<ShipComponent>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HullPart {
    pub name: String,
    pub hp: f32,
    /// Collision shell triangles, ship frame.
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShipComponent {
    /// Index for [`Hangar::exposure`] and in [`ShotResult::components`].
    pub index: u32,
    /// Port path from the hull (`turret/gimbal/gun`).
    pub path: String,
    pub guid: String,
    pub label: String,
    /// `Type/SubType`.
    pub kind: String,
    /// Its frame in the ship frame, 3 × 4 row-major.
    pub transform: [f32; 12],
    /// Its mesh bounds in its own frame; `None` without geometry.
    pub min: Option<[f32; 3]>,
    pub max: Option<[f32; 3]>,
    /// HP when penetration can damage it.
    pub health: Option<f32>,
}

/// A gun in the weapon list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WeaponSummary {
    pub record: String,
    pub label: String,
    pub size: i32,
    /// Per shot: physical, energy, distortion, thermal, biochemical, stun.
    pub damage: [f32; 6],
    /// Penetration depth, m, for projectiles that have one.
    pub penetration: Option<f32>,
}

/// One shot at a ship.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShotSpec {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    /// The gun's record name.
    pub weapon: String,
    /// Armour HP ratio, 0..=1.
    pub armor: f32,
    /// Struck shield face HP ratio, 0..=1 (0 = down).
    pub shield: f32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShotResult {
    /// Where it lands on the collision shell; `None` for a miss.
    pub point: Option<[f32; 3]>,
    pub part: Option<String>,
    pub shield_taken: f32,
    /// Total damage passed through the shield.
    pub passed: f32,
    /// Total damage past the armour.
    pub after_armor: f32,
    pub armor_wear: f32,
    pub hull_damage: f32,
    /// Penetration depth, m, from `point` along the shot.
    pub depth: f32,
    pub components: Vec<ComponentDamage>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ComponentDamage {
    pub index: u32,
    pub label: String,
    pub depth: f32,
    pub damage: f32,
}

/// One attack direction on a component.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExposureDirection {
    /// The shot's direction (unit, ship frame).
    pub direction: [f32; 3],
    /// Hull surface to the component's centre, m.
    pub depth: f32,
    pub part: Option<String>,
}

/// The ship data, built with the catalog.
pub struct Hangar {
    vehicles: Vehicles,
    components: ShipComponents,
    healths: ComponentHealths,
    paths: GeometryPaths,
    weapons: Vec<ShipWeapon>,
    labels: HashMap<Guid, String>,
    prepared: Mutex<HashMap<Guid, Arc<Prepared>>>,
}

/// What hits on one ship share (the hull read once).
struct Prepared {
    geometry: HullGeometry,
    raycaster: Raycaster,
    defense: Defense,
}

impl Hangar {
    pub(crate) fn build(
        datacore: &Datacore,
        items: &Items,
        source: &AssetSource,
        locale: &LocaleMap,
    ) -> Self {
        let containers = PortContainers::build(datacore, source);
        let vehicles = Vehicles::build(datacore, items, &containers, source);
        let labels = vehicles
            .iter()
            .filter_map(|(g, v)| {
                let name = locale.resolve(v.name_key.as_ref()?)?;
                (!name.trim().is_empty()).then(|| (*g, name.to_string()))
            })
            .collect();
        let weapons = Weapons::build(datacore, items)
            .ships
            .into_iter()
            .filter(|w| w.damage.is_some())
            .collect();
        Self {
            components: ShipComponents::build(datacore, items),
            healths: ComponentHealths::build(datacore),
            paths: GeometryPaths::build(datacore),
            vehicles,
            weapons,
            labels,
            prepared: Mutex::default(),
        }
    }

    /// The listed player ships and vehicles, by label.
    pub(crate) fn list(&self, record: impl Fn(&Guid) -> String) -> Vec<ShipSummary> {
        let mut out: Vec<ShipSummary> = self
            .vehicles
            .listed()
            .map(|(g, v)| ShipSummary {
                guid: g.to_string(),
                label: self.label(g, &record),
                record: record(g),
                listing: format!("{:?}", v.listing),
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label).then(a.record.cmp(&b.record)));
        out
    }

    pub(crate) fn label(&self, guid: &Guid, record: impl Fn(&Guid) -> String) -> String {
        self.labels
            .get(guid)
            .cloned()
            .unwrap_or_else(|| record(guid))
    }

    pub(crate) fn vehicle(&self, guid: &Guid) -> Result<&Vehicle> {
        self.vehicles
            .get(guid)
            .ok_or_else(|| Error::Catalog(format!("{guid} is not a vehicle")))
    }

    /// The hull read and its hit model set up, once per ship.
    fn prepared(&self, source: &AssetSource, guid: &Guid) -> Result<Arc<Prepared>> {
        if let Some(p) = self.prepared.lock().unwrap().get(guid) {
            return Ok(p.clone());
        }
        let ship = self.vehicle(guid)?;
        let geometry =
            HullGeometry::read(source, ship, &ship.stock, &self.vehicles, &self.paths)
                .ok_or_else(|| Error::Catalog(format!("{guid}: hull geometry unreadable")))?;
        let raycaster = Raycaster::new(&geometry.collision);
        let defense = Defense::compute(ship, &ship.stock, &self.vehicles, &self.components);
        let p = Arc::new(Prepared {
            geometry,
            raycaster,
            defense,
        });
        self.prepared.lock().unwrap().insert(*guid, p.clone());
        Ok(p)
    }

    /// The hull's node → damage part map and the damage tooling of a ship.
    pub(crate) fn overlays(
        &self,
        source: &AssetSource,
        guid: &Guid,
        items: &Items,
        label: impl Fn(&Guid) -> String,
    ) -> Result<(Vec<NodePart>, Vec<HullPart>, Vec<ShipComponent>)> {
        let ship = self.vehicle(guid)?;
        let p = self.prepared(source, guid)?;
        let node_parts = p
            .geometry
            .node_parts
            .iter()
            .map(|(node, part)| NodePart {
                node: node.clone(),
                part: part.clone(),
            })
            .collect();
        let parts = p
            .geometry
            .collision
            .iter()
            .map(|m| HullPart {
                hp: ship
                    .hull
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case(&m.part))
                    .map_or(0.0, |h| h.damage_max),
                name: m.part.clone(),
                positions: m.positions.clone(),
                indices: m.indices.clone(),
            })
            .collect();
        let components = p
            .geometry
            .placements
            .iter()
            .enumerate()
            .map(|(i, pl)| ShipComponent {
                index: i as u32,
                path: pl.path.clone(),
                guid: pl.item.to_string(),
                label: label(&pl.item),
                kind: items.get(&pl.item).map_or_else(String::new, |it| {
                    format!(
                        "{}/{}",
                        it.item_type.as_dcb_str(),
                        it.item_sub_type.as_dcb_str()
                    )
                }),
                transform: std::array::from_fn(|k| pl.transform[k / 4][k % 4]),
                min: pl.bounds.map(|b| b.min),
                max: pl.bounds.map(|b| b.max),
                health: self.healths.get(&pl.item).map(|h| h.health),
            })
            .collect();
        Ok((node_parts, parts, components))
    }

    /// The interior brushes of a ship (read per call; the view caches it).
    pub(crate) fn interior(&self, source: &AssetSource, guid: &Guid) -> Result<Vec<InteriorBrush>> {
        let interior = Interior::read(source, self.vehicle(guid)?);
        Ok(interior
            .brushes
            .into_iter()
            .map(|b| InteriorBrush {
                geometry: b.geometry,
                transform: std::array::from_fn(|k| b.transform[k / 4][k % 4]),
            })
            .collect())
    }

    /// Every gun with damage, by label.
    pub(crate) fn weapons(&self, label: impl Fn(&Guid) -> String) -> Vec<WeaponSummary> {
        let mut out: Vec<WeaponSummary> = self
            .weapons
            .iter()
            .map(|w| {
                let d = w.damage.as_ref();
                WeaponSummary {
                    record: w.record_name.clone(),
                    label: label(&w.guid),
                    size: w.size,
                    damage: [
                        d.map_or(0.0, |d| d.physical),
                        d.map_or(0.0, |d| d.energy),
                        d.map_or(0.0, |d| d.distortion),
                        d.map_or(0.0, |d| d.thermal),
                        d.map_or(0.0, |d| d.biochemical),
                        d.map_or(0.0, |d| d.stun),
                    ],
                    penetration: w.penetration_m,
                }
            })
            .collect();
        out.sort_by(|a, b| a.size.cmp(&b.size).then(a.label.cmp(&b.label)));
        out
    }

    fn target<'a>(&'a self, ship: &'a Vehicle, p: &'a Prepared, items: &'a Items) -> Target<'a> {
        Target {
            vehicle: ship,
            geometry: &p.geometry,
            raycaster: &p.raycaster,
            defense: &p.defense,
            items,
            healths: &self.healths,
            rules: DamageRules::default(),
        }
    }

    /// One shot (sc-vehicles `Target::hit`).
    pub(crate) fn hit(
        &self,
        source: &AssetSource,
        guid: &Guid,
        items: &Items,
        spec: &ShotSpec,
        label: impl Fn(&Guid) -> String,
    ) -> Result<ShotResult> {
        let ship = self.vehicle(guid)?;
        let p = self.prepared(source, guid)?;
        let w = self
            .weapons
            .iter()
            .find(|w| w.record_name == spec.weapon)
            .ok_or_else(|| Error::Catalog(format!("no gun {}", spec.weapon)))?;
        let d = w.damage.as_ref();
        let shot = Shot {
            origin: spec.origin,
            direction: spec.direction,
            damage: PerDamageType {
                physical: d.map_or(0.0, |d| d.physical),
                energy: d.map_or(0.0, |d| d.energy),
                distortion: d.map_or(0.0, |d| d.distortion),
                thermal: d.map_or(0.0, |d| d.thermal),
                biochemical: d.map_or(0.0, |d| d.biochemical),
                stun: d.map_or(0.0, |d| d.stun),
            },
            penetration: w
                .penetration_m
                .map(|m| (m, w.penetration_radius_m.unwrap_or([0.0; 2]))),
        };
        let condition = Condition {
            armor_ratio: spec.armor.clamp(0.0, 1.0),
            shield_ratio: spec.shield.clamp(0.0, 1.0),
            ..Condition::default()
        };
        let r = self.target(ship, &p, items).hit(&shot, &condition);
        let total = |d: &PerDamageType<f32>| {
            d.physical + d.energy + d.distortion + d.thermal + d.biochemical + d.stun
        };
        Ok(ShotResult {
            point: r.point,
            part: r.part.map(|(_, name)| name),
            shield_taken: r.shield_taken,
            passed: total(&r.passed),
            after_armor: total(&r.after_armor),
            armor_wear: r.armor_wear,
            hull_damage: r.hull_damage,
            depth: r.depth,
            components: r
                .components
                .iter()
                .map(|c| ComponentDamage {
                    index: c.placement as u32,
                    label: label(&c.item),
                    depth: c.depth,
                    damage: c.damage,
                })
                .collect(),
        })
    }

    /// Attack directions on component `index`, shallowest first.
    pub(crate) fn exposure(
        &self,
        source: &AssetSource,
        guid: &Guid,
        items: &Items,
        index: u32,
        samples: u32,
    ) -> Result<Vec<ExposureDirection>> {
        let ship = self.vehicle(guid)?;
        let p = self.prepared(source, guid)?;
        Ok(self
            .target(ship, &p, items)
            .exposure(index as usize, samples.clamp(16, 20_000) as usize)
            .into_iter()
            .map(|e| ExposureDirection {
                direction: e.direction,
                depth: e.depth,
                part: e.part,
            })
            .collect())
    }
}
