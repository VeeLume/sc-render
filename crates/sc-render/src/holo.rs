//! A ship's hologram built in one go on every core: [`Assets::holo`] reads
//! the scene's files, places each drawn submesh, merges each port's into one
//! draw and finds the hull's crease lines, and hands back one buffer. The
//! hologram needs no textures, so nothing else has to cross to the webview.
//!
//! Buffer layout, per [`HoloPort`], every section 4-byte aligned and
//! little-endian: positions `i16 × 4` (normalised; `w` unused), normals
//! `i8 × 4` (normalised; `w` unused), then indices (`u16`, or `u32` when
//! [`HoloPort::wide_indices`]). Four-component formats on purpose: Direct3D
//! has no three-component 8- or 16-bit vertex format, so ANGLE would rewrite
//! the whole buffer on upload. A position decodes to the ship frame as
//! `centre + xyz * scale`.

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sc_extract::MeshGeometry;
use serde::Serialize;

use crate::{Assets, BoneInfo, MaterialInfo, ScenePart};

/// Above this many hull triangles a ship gets no crease lines: they would
/// merge into a solid mass at any size a ship that big is shown at.
const EDGE_LIMIT: usize = 1_500_000;
/// Two faces meeting at more than this draw a crease, degrees.
const CREASE_ANGLE: f64 = 30.0;
/// A port with more indices than this merges across every core.
const BIG_PORT_INDICES: usize = 300_000;

/// A built hologram and what is in it.
pub struct Holo {
    pub info: HoloInfo,
    pub buffer: Vec<u8>,
}

/// What a hologram's buffer holds. See the module docs for the layout.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HoloInfo {
    /// Where a host serves the buffer from; empty until it stores it.
    pub path: String,
    /// The host posted the buffer as a shared buffer, tagged with `path`,
    /// instead of serving it.
    pub shared: bool,
    /// One draw per port, the hull (path `""`) first when there is one.
    pub ports: Vec<HoloPort>,
    pub edges: Option<HoloEdges>,
    /// What could not be drawn, and where the time went (shown in the log).
    pub notes: Vec<String>,
    pub byte_length: u32,
}

/// One port's merged geometry.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HoloPort {
    /// Port names from the hull down, joined by `/`, lower case.
    pub path: String,
    /// Decodes a position: `centre + xyz * scale`.
    pub centre: [f32; 3],
    pub scale: f32,
    /// Bounds in the ship frame.
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub vertex_count: u32,
    pub index_count: u32,
    pub positions_offset: u32,
    pub normals_offset: u32,
    pub indices_offset: u32,
    pub wide_indices: bool,
}

/// The hull's crease lines: pairs of `i16 × 4` positions (normalised).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HoloEdges {
    pub centre: [f32; 3],
    pub scale: f32,
    pub vertex_count: u32,
    pub offset: u32,
}

impl Assets {
    /// `root`'s hologram. Files that cannot be read are left out and noted.
    ///
    /// ```no_run
    /// # fn demo(assets: &sc_render::Assets, ship: &sc_render::ShipScene) {
    /// let holo = assets.holo(&ship.root);
    /// println!("{} draws, {} bytes", holo.info.ports.len(), holo.buffer.len());
    /// # }
    /// ```
    pub fn holo(&self, root: &ScenePart) -> Holo {
        let started = Instant::now();
        let mut notes = Vec::new();

        let mut files = Files::default();
        scan(root, &mut files);
        let paths: Vec<(&String, &&str)> = files.geometry.iter().collect();
        let read = across_cores(&paths, |(_, path)| {
            MeshGeometry::read(self.source(), path).map(Arc::new)
        });
        let mut meshes = HashMap::with_capacity(paths.len());
        for ((key, path), result) in paths.iter().zip(read) {
            match result {
                Ok(g) => {
                    meshes.insert(key.to_string(), g);
                }
                Err(e) => notes.push(format!("{path}: not drawn ({e})")),
            }
        }

        let mut material_paths = BTreeMap::new();
        materials_of(root, &meshes, &mut material_paths);
        let paths: Vec<(&String, &String)> = material_paths.iter().collect();
        let read = across_cores(&paths, |(_, path)| self.material(path).ok());
        let materials: HashMap<String, MaterialInfo> = paths
            .iter()
            .zip(read)
            .filter_map(|((key, _), m)| Some((key.to_string(), m?)))
            .collect();

        let paths: Vec<(&String, &&str)> = files.skeletons.iter().collect();
        let read = across_cores(&paths, |(_, path)| self.skeleton(path).unwrap_or_default());
        let skeletons: HashMap<String, Vec<BoneInfo>> = paths
            .iter()
            .zip(read)
            .map(|((key, _), bones)| (key.to_string(), bones))
            .collect();
        let loaded = started.elapsed();

        let mut placer = Placer {
            meshes: &meshes,
            materials: &materials,
            skeletons: &skeletons,
            skins: &files.skins,
            ports: BTreeMap::new(),
            notes: &mut notes,
        };
        placer.place(root, IDENTITY, "");
        let ports: Vec<(String, Vec<Instance>)> = placer.ports.into_iter().collect();
        let meshes_drawn: usize = ports.iter().map(|(_, l)| l.len()).sum();

        // Big ports split across the cores themselves; small ones share them.
        let is_big = |list: &[Instance]| {
            list.iter()
                .map(|i| i.mesh.submeshes[i.submesh].index_count as usize)
                .sum::<usize>()
                > BIG_PORT_INDICES
        };
        let (big, small): (Vec<_>, Vec<_>) = ports.iter().partition(|(_, list)| is_big(list));
        let mut merged = across_cores(&small, |(path, list)| merge(path, list, false));
        merged.extend(big.iter().map(|(path, list)| merge(path, list, true)));
        merged.sort_by(|a, b| a.path.cmp(&b.path));

        let hull = ports.iter().find(|(path, _)| path.is_empty());
        let hull_triangles: usize = hull.map_or(0, |(_, list)| {
            list.iter()
                .map(|i| i.mesh.submeshes[i.submesh].index_count as usize / 3)
                .sum()
        });
        let edges = match hull {
            Some((_, list)) if hull_triangles <= EDGE_LIMIT => {
                let lines = across_cores(list, creases).concat();
                (!lines.is_empty()).then(|| quantise_lines(&lines))
            }
            Some(_) => {
                notes.push(format!(
                    "holo: {:.2} M hull triangles, no crease lines",
                    hull_triangles as f64 / 1e6
                ));
                None
            }
            None => None,
        };

        let mut size = 0;
        for p in &merged {
            size = align4(size) + p.positions.len();
            size = align4(size) + p.normals.len();
            size = align4(size) + p.indices.len();
        }
        if let Some(e) = &edges {
            size = align4(size) + e.bytes.len();
        }
        let mut buffer = Vec::with_capacity(size);
        let mut info = HoloInfo::default();
        for p in merged {
            let positions_offset = append(&mut buffer, &p.positions);
            let normals_offset = append(&mut buffer, &p.normals);
            let indices_offset = append(&mut buffer, &p.indices);
            info.ports.push(HoloPort {
                path: p.path,
                centre: p.centre,
                scale: p.scale,
                min: p.min,
                max: p.max,
                vertex_count: p.vertex_count,
                index_count: p.index_count,
                positions_offset,
                normals_offset,
                indices_offset,
                wide_indices: p.wide_indices,
            });
        }
        info.edges = edges.map(|e| HoloEdges {
            centre: e.centre,
            scale: e.scale,
            vertex_count: e.vertex_count,
            offset: append(&mut buffer, &e.bytes),
        });
        info.byte_length = buffer.len() as u32;
        notes.push(format!(
            "holo: {} draws from {meshes_drawn} meshes, {:.0} MB",
            info.ports.len(),
            buffer.len() as f64 / 1e6
        ));
        notes.push(format!(
            "holo: files {} ms, built {} ms",
            loaded.as_millis(),
            (started.elapsed() - loaded).as_millis()
        ));
        info.notes = notes;
        Holo { info, buffer }
    }
}

fn align4(n: usize) -> usize {
    n.next_multiple_of(4)
}

/// Append 4-byte aligned; returns where it landed.
fn append(buffer: &mut Vec<u8>, bytes: &[u8]) -> u32 {
    buffer.resize(align4(buffer.len()), 0);
    let at = buffer.len() as u32;
    buffer.extend_from_slice(bytes);
    at
}

/// `f` over `items` on every core; results in `items`' order.
fn across_cores<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(items.len());
    if workers < 2 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let result = f(item);
                    done.lock().unwrap().push((i, result));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap();
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// Every file a scene needs, by lower-case path.
#[derive(Default)]
struct Files<'a> {
    geometry: BTreeMap<String, &'a str>,
    skeletons: BTreeMap<String, &'a str>,
    /// Worn `.skin` files, whose bones can carry attachments.
    skins: Vec<String>,
}

fn scan<'a>(part: &'a ScenePart, files: &mut Files<'a>) {
    for piece in &part.pieces {
        let key = piece.geometry.to_ascii_lowercase();
        if key.ends_with(".skin") {
            files.skeletons.insert(key.clone(), &piece.geometry);
            files.skins.push(key.clone());
        }
        files.geometry.insert(key, &piece.geometry);
    }
    if let Some(s) = &part.skeleton {
        files.skeletons.insert(s.to_ascii_lowercase(), s);
    }
    for a in &part.attachments {
        scan(&a.part, files);
    }
}

fn materials_of(
    part: &ScenePart,
    meshes: &HashMap<String, Arc<MeshGeometry>>,
    out: &mut BTreeMap<String, String>,
) {
    for piece in &part.pieces {
        let Some(g) = meshes.get(&piece.geometry.to_ascii_lowercase()) else {
            continue;
        };
        if let Some(m) = piece
            .material
            .clone()
            .or_else(|| g.material_path(&piece.geometry))
        {
            out.insert(m.to_ascii_lowercase(), m);
        }
    }
    for a in &part.attachments {
        materials_of(&a.part, meshes, out);
    }
}

/// A placement, 3 × 4 row-major.
type Affine = [[f32; 4]; 3];

const IDENTITY: Affine = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn multiply(a: &Affine, b: &Affine) -> Affine {
    let mut out = [[0.0; 4]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
        row[3] += a[r][3];
    }
    out
}

/// Position plus rotation as a quaternion `(x, y, z, w)`.
fn compose(p: &[f32; 3], q: &[f32; 4]) -> Affine {
    let [x, y, z, w] = *q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            p[0],
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            p[1],
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
            p[2],
        ],
    ]
}

fn cofactors(m: &Affine) -> ([[f32; 3]; 3], f32) {
    let c = [
        [
            m[1][1] * m[2][2] - m[1][2] * m[2][1],
            m[1][2] * m[2][0] - m[1][0] * m[2][2],
            m[1][0] * m[2][1] - m[1][1] * m[2][0],
        ],
        [
            m[0][2] * m[2][1] - m[0][1] * m[2][2],
            m[0][0] * m[2][2] - m[0][2] * m[2][0],
            m[0][1] * m[2][0] - m[0][0] * m[2][1],
        ],
        [
            m[0][1] * m[1][2] - m[0][2] * m[1][1],
            m[0][2] * m[1][0] - m[0][0] * m[1][2],
            m[0][0] * m[1][1] - m[0][1] * m[1][0],
        ],
    ];
    let det = m[0][0] * c[0][0] + m[0][1] * c[0][1] + m[0][2] * c[0][2];
    (c, det)
}

/// `None` for a degenerate matrix.
fn invert(m: &Affine) -> Option<Affine> {
    let (c, det) = cofactors(m);
    if det == 0.0 {
        return None;
    }
    let mut out = IDENTITY;
    for r in 0..3 {
        for k in 0..3 {
            out[r][k] = c[k][r] / det;
        }
        out[r][3] = -(out[r][0] * m[0][3] + out[r][1] * m[1][3] + out[r][2] * m[2][3]);
    }
    Some(out)
}

fn apply(m: &Affine, p: &[f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

/// Normals transform by the inverse transpose, here up to scale.
fn normal_matrix(m: &Affine) -> [[f32; 3]; 3] {
    cofactors(m).0
}

fn apply_normal(m: &[[f32; 3]; 3], n: &[f32; 3]) -> [f32; 3] {
    normalise(std::array::from_fn(|r| {
        m[r][0] * n[0] + m[r][1] * n[1] + m[r][2] * n[2]
    }))
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 { v.map(|c| c / len) } else { v }
}

/// Each node's placement within its file.
fn node_matrices(g: &MeshGeometry) -> Vec<Affine> {
    fn resolve(g: &MeshGeometry, i: usize, done: &mut [Option<Affine>], depth: usize) -> Affine {
        if let Some(m) = done[i] {
            return m;
        }
        let node = &g.nodes[i];
        let m = match node
            .parent
            .filter(|&p| p < g.nodes.len() && depth < g.nodes.len())
        {
            Some(p) => multiply(&resolve(g, p, done, depth + 1), &node.transform),
            None => node.transform,
        };
        done[i] = Some(m);
        m
    }
    let mut done = vec![None; g.nodes.len()];
    (0..g.nodes.len())
        .map(|i| resolve(g, i, &mut done, 0))
        .collect()
}

/// A part's geometry nodes (lower-case name, placement) in the order a search
/// for an attachment's node meets them: piece by piece, parents first.
fn nodes_of(
    part: &ScenePart,
    meshes: &HashMap<String, Arc<MeshGeometry>>,
) -> Vec<(String, Affine)> {
    let mut out = Vec::new();
    for piece in &part.pieces {
        let Some(g) = meshes.get(&piece.geometry.to_ascii_lowercase()) else {
            continue;
        };
        let matrices = node_matrices(g);
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); g.nodes.len()];
        let mut roots = Vec::new();
        for (i, node) in g.nodes.iter().enumerate() {
            match node.parent.filter(|&p| p < g.nodes.len()) {
                Some(p) => children[p].push(i),
                None => roots.push(i),
            }
        }
        // A worklist, not recursion: a hull has thousands of nodes.
        roots.reverse();
        let mut stack = roots;
        while let Some(i) = stack.pop() {
            out.push((g.nodes[i].name.to_ascii_lowercase(), matrices[i]));
            stack.extend(children[i].iter().rev());
        }
    }
    out
}

/// One submesh at one place in the ship frame.
struct Instance {
    mesh: Arc<MeshGeometry>,
    submesh: usize,
    at: Affine,
}

impl Instance {
    fn indices(&self) -> &[u32] {
        let s = &self.mesh.submeshes[self.submesh];
        &self.mesh.indices[s.first_index as usize..(s.first_index + s.index_count) as usize]
    }
}

struct Placer<'a> {
    meshes: &'a HashMap<String, Arc<MeshGeometry>>,
    materials: &'a HashMap<String, MaterialInfo>,
    skeletons: &'a HashMap<String, Vec<BoneInfo>>,
    skins: &'a [String],
    ports: BTreeMap<String, Vec<Instance>>,
    notes: &'a mut Vec<String>,
}

impl Placer<'_> {
    fn bones(&self, part: &ScenePart) -> &[BoneInfo] {
        part.skeleton
            .as_ref()
            .and_then(|s| self.skeletons.get(&s.to_ascii_lowercase()))
            .map_or(&[], Vec::as_slice)
    }

    /// A bone by lower-case name: the part's skeleton, else a worn skin's
    /// (only a part with a skeleton can use those).
    fn bone(&self, part: &ScenePart, name: &str) -> Option<&BoneInfo> {
        let found =
            |bones: &'_ [BoneInfo]| bones.iter().position(|b| b.name.eq_ignore_ascii_case(name));
        let own = self.bones(part);
        if let Some(i) = found(own) {
            return Some(&own[i]);
        }
        part.skeleton.as_ref()?;
        self.skins.iter().find_map(|skin| {
            let bones = self.skeletons.get(skin)?;
            found(bones).map(|i| &bones[i])
        })
    }

    /// An item's own helper in its space, for sitting it on a port.
    fn helper(&self, part: &ScenePart, name: &str) -> Option<Affine> {
        let own = self.bones(part);
        if let Some(b) = own.iter().find(|b| b.name.eq_ignore_ascii_case(name)) {
            return Some(compose(&b.position, &b.rotation));
        }
        let lower = name.to_ascii_lowercase();
        nodes_of(part, self.meshes)
            .into_iter()
            .find(|(n, _)| *n == lower)
            .map(|(_, m)| m)
    }

    fn place(&mut self, part: &ScenePart, world: Affine, path: &str) {
        for piece in &part.pieces {
            let Some(g) = self.meshes.get(&piece.geometry.to_ascii_lowercase()) else {
                continue;
            };
            let material = piece
                .material
                .clone()
                .or_else(|| g.material_path(&piece.geometry))
                .and_then(|m| self.materials.get(&m.to_ascii_lowercase()));
            let Some(material) = material else { continue };
            let nodes = node_matrices(g);
            for (submesh, s) in g.submeshes.iter().enumerate() {
                let Some(sub) = material.submaterials.get(s.material as usize) else {
                    continue;
                };
                if sub.shader == "NoDraw" || sub.name.starts_with('$') {
                    continue;
                }
                let range = s.first_index as usize..(s.first_index + s.index_count) as usize;
                let valid = g
                    .indices
                    .get(range)
                    .is_some_and(|i| i.iter().all(|&v| (v as usize) < g.positions.len()));
                if !valid {
                    self.notes.push(format!(
                        "{}: submesh {submesh} out of range",
                        piece.geometry
                    ));
                    continue;
                }
                let at = match s.node.and_then(|n| nodes.get(n)) {
                    Some(node) => multiply(&world, node),
                    None => world,
                };
                self.ports
                    .entry(path.to_ascii_lowercase())
                    .or_default()
                    .push(Instance {
                        mesh: g.clone(),
                        submesh,
                        at,
                    });
            }
        }
        if part.attachments.is_empty() {
            return;
        }
        let nodes = nodes_of(part, self.meshes);
        for a in &part.attachments {
            let name = a.bone.to_ascii_lowercase();
            let anchor = if name.is_empty() {
                Some(world)
            } else if let Some(b) = self.bone(part, &name) {
                Some(multiply(&world, &compose(&b.position, &b.rotation)))
            } else {
                nodes
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, m)| multiply(&world, m))
            };
            let Some(anchor) = anchor else {
                self.notes
                    .push(format!("{}: no bone or node {}", a.part.record, a.bone));
                continue;
            };
            let mut child = multiply(&anchor, &compose(&a.offset_position, &a.offset_rotation));
            if let Some(helper) = &a.item_helper {
                match self.helper(&a.part, helper).and_then(|h| invert(&h)) {
                    Some(inverse) => child = multiply(&child, &inverse),
                    None => self
                        .notes
                        .push(format!("{}: no helper {helper}", a.part.record)),
                }
            }
            let child_path = match (&a.port, path.is_empty()) {
                (Some(port), true) => port.clone(),
                (Some(port), false) => format!("{path}/{port}"),
                (None, _) => path.to_string(),
            };
            self.place(&a.part, child, &child_path);
        }
    }
}

struct Merged {
    path: String,
    centre: [f32; 3],
    scale: f32,
    min: [f32; 3],
    max: [f32; 3],
    vertex_count: u32,
    index_count: u32,
    positions: Vec<u8>,
    normals: Vec<u8>,
    indices: Vec<u8>,
    wide_indices: bool,
}

/// The frame positions are quantised in: bounds centre and largest half extent.
fn frame(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3], [f32; 3], f32) {
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in positions {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    if positions.is_empty() {
        return ([0.0; 3], [0.0; 3], [0.0; 3], 1.0);
    }
    let centre = std::array::from_fn(|k| (min[k] + max[k]) * 0.5);
    let half = (0..3).map(|k| (max[k] - min[k]) * 0.5).fold(0.0, f32::max);
    (min, max, centre, if half > 0.0 { half } else { 1.0 })
}

fn write_positions(positions: &[[f32; 3]], centre: &[f32; 3], scale: f32) -> Vec<u8> {
    let mut out = vec![0u8; positions.len() * 8];
    for (chunk, p) in out.as_chunks_mut::<8>().0.iter_mut().zip(positions) {
        for k in 0..3 {
            let q = ((p[k] - centre[k]) / scale * 32767.0).round() as i16;
            chunk[k * 2..k * 2 + 2].copy_from_slice(&q.to_le_bytes());
        }
    }
    out
}

/// One submesh in the ship frame, with only the vertices it indexes (every
/// submesh of a file shares the file's whole vertex buffer).
struct Placed {
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    indices: Vec<u32>,
}

fn place_submesh(inst: &Instance) -> Placed {
    let g = &inst.mesh;
    let source = inst.indices();
    let has_normals = g.normals.len() == g.positions.len();
    let nm = normal_matrix(&inst.at);
    let mut positions = Vec::with_capacity(source.len() / 2);
    let mut normals = Vec::with_capacity(if has_normals { source.len() / 2 } else { 0 });
    let mut indices = Vec::with_capacity(source.len());
    let mut add = |v: u32| {
        let i = v as usize;
        positions.push(apply(&inst.at, &g.positions[i]));
        if has_normals {
            normals.push(apply_normal(&nm, &g.normals[i]));
        }
        positions.len() as u32 - 1
    };
    let lo = source.iter().copied().min().unwrap_or(0);
    let hi = source.iter().copied().max().unwrap_or(0);
    let span = (hi - lo) as usize + 1;
    // A submesh's vertices are a run of the file's, so a table over that run
    // is the fast path; a scattered one must not allocate the whole file.
    if span <= source.len().saturating_mul(8).max(1024) {
        let mut slot = vec![u32::MAX; span];
        for &v in source {
            let s = &mut slot[(v - lo) as usize];
            if *s == u32::MAX {
                *s = add(v);
            }
            indices.push(*s);
        }
    } else {
        let mut slot: QuickMap<u32, u32> = QuickMap::default();
        for &v in source {
            indices.push(*slot.entry(v).or_insert_with(|| add(v)));
        }
    }
    Placed {
        positions,
        normals: has_normals.then_some(normals),
        indices,
    }
}

/// Merge `list` into one draw, its submeshes `across_cores` for a big port
/// (the hull is most of a ship).
fn merge(path: &str, list: &[Instance], parallel: bool) -> Merged {
    let placed = if parallel {
        across_cores(list, place_submesh)
    } else {
        list.iter().map(place_submesh).collect()
    };
    let vertices = placed.iter().map(|p| p.positions.len()).sum();
    let index_count = placed.iter().map(|p| p.indices.len()).sum();
    let all_normals = placed.iter().all(|p| p.normals.is_some());
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertices);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(if all_normals { vertices } else { 0 });
    let mut indices: Vec<u32> = Vec::with_capacity(index_count);
    for p in &placed {
        let base = positions.len() as u32;
        positions.extend_from_slice(&p.positions);
        if let (true, Some(n)) = (all_normals, &p.normals) {
            normals.extend_from_slice(n);
        }
        indices.extend(p.indices.iter().map(|i| base + i));
    }
    drop(placed);
    if !all_normals {
        normals = face_normals(&positions, &indices);
    }
    let (min, max, centre, scale) = frame(&positions);
    let mut normal_bytes = vec![0u8; normals.len() * 4];
    for (chunk, n) in normal_bytes.as_chunks_mut::<4>().0.iter_mut().zip(&normals) {
        for k in 0..3 {
            chunk[k] = ((n[k] * 127.0).round() as i8) as u8;
        }
    }
    let wide_indices = positions.len() > usize::from(u16::MAX);
    let index_bytes = if wide_indices {
        let mut out = vec![0u8; indices.len() * 4];
        for (chunk, i) in out.as_chunks_mut::<4>().0.iter_mut().zip(&indices) {
            chunk.copy_from_slice(&i.to_le_bytes());
        }
        out
    } else {
        let mut out = vec![0u8; indices.len() * 2];
        for (chunk, i) in out.as_chunks_mut::<2>().0.iter_mut().zip(&indices) {
            chunk.copy_from_slice(&(*i as u16).to_le_bytes());
        }
        out
    };
    Merged {
        path: path.to_string(),
        centre,
        scale,
        min,
        max,
        vertex_count: positions.len() as u32,
        index_count: indices.len() as u32,
        positions: write_positions(&positions, &centre, scale),
        normals: normal_bytes,
        indices: index_bytes,
        wide_indices,
    }
}

/// Smooth normals from the faces, for geometry that came without.
fn face_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; positions.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| positions[i as usize]);
        let (u, v): ([f32; 3], [f32; 3]) = (
            std::array::from_fn(|k| c[k] - b[k]),
            std::array::from_fn(|k| a[k] - b[k]),
        );
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in t {
            for k in 0..3 {
                out[i as usize][k] += n[k];
            }
        }
    }
    out.into_iter().map(normalise).collect()
}

/// FNV-style hashing: the crease search does millions of small lookups and
/// has no untrusted keys.
#[derive(Default)]
struct Quick(u64);

impl Hasher for Quick {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 29)
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

type QuickMap<K, V> = HashMap<K, V, BuildHasherDefault<Quick>>;

/// An edge seen once so far: its two vertices and its face's normal.
type OpenEdge = (u32, u32, [f64; 3]);

/// One submesh's crease lines as point pairs in the ship frame: an edge
/// where two faces meet beyond [`CREASE_ANGLE`], or with one face only.
/// Vertices weld by position to a tenth of a millimetre.
fn creases(inst: &Instance) -> Vec<[f32; 3]> {
    let positions = &inst.mesh.positions;
    let indices = inst.indices();
    let threshold = CREASE_ANGLE.to_radians().cos();
    let mut weld: QuickMap<[i64; 3], u32> = QuickMap::default();
    let mut open: QuickMap<u64, Option<OpenEdge>> = QuickMap::default();
    let mut lines = Vec::new();
    let emit = |lines: &mut Vec<[f32; 3]>, a: u32, b: u32| {
        lines.push(apply(&inst.at, &positions[a as usize]));
        lines.push(apply(&inst.at, &positions[b as usize]));
    };
    for t in indices.as_chunks::<3>().0 {
        let ids = [t[0], t[1], t[2]].map(|v| {
            let key = positions[v as usize].map(|c| (f64::from(c) * 1e4).round() as i64);
            let next = weld.len() as u32;
            *weld.entry(key).or_insert(next)
        });
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
            continue;
        }
        let normal = triangle_normal(t.iter().map(|&v| positions[v as usize]));
        for j in 0..3 {
            let (next, a, b) = ((j + 1) % 3, t[j], t[(j + 1) % 3]);
            let edge = u64::from(ids[j]) << 32 | u64::from(ids[next]);
            let reverse = u64::from(ids[next]) << 32 | u64::from(ids[j]);
            if let Some(slot) = open.get_mut(&reverse)
                && let Some((_, _, other)) = *slot
            {
                let dot = normal[0] * other[0] + normal[1] * other[1] + normal[2] * other[2];
                if dot <= threshold {
                    emit(&mut lines, a, b);
                }
                *slot = None;
            } else {
                open.entry(edge).or_insert(Some((a, b, normal)));
            }
        }
    }
    for (a, b, _) in open.into_values().flatten() {
        emit(&mut lines, a, b);
    }
    lines
}

fn triangle_normal(mut points: impl Iterator<Item = [f32; 3]>) -> [f64; 3] {
    let mut next = || points.next().map_or([0.0; 3], |p| p.map(f64::from));
    let (a, b, c) = (next(), next(), next());
    let u: [f64; 3] = std::array::from_fn(|k| c[k] - b[k]);
    let v: [f64; 3] = std::array::from_fn(|k| a[k] - b[k]);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 {
        n.map(|c| c / len)
    } else {
        [0.0; 3]
    }
}

struct Lines {
    centre: [f32; 3],
    scale: f32,
    vertex_count: u32,
    bytes: Vec<u8>,
}

fn quantise_lines(points: &[[f32; 3]]) -> Lines {
    let (_, _, centre, scale) = frame(points);
    Lines {
        centre,
        scale,
        vertex_count: points.len() as u32,
        bytes: write_positions(points, &centre, scale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_extract::Submesh;

    fn instance(positions: Vec<[f32; 3]>, indices: Vec<u32>, at: Affine) -> Instance {
        let index_count = indices.len() as u32;
        Instance {
            mesh: Arc::new(MeshGeometry {
                positions,
                indices,
                submeshes: vec![Submesh {
                    material: 0,
                    node: None,
                    first_index: 0,
                    index_count,
                }],
                ..MeshGeometry::default()
            }),
            submesh: 0,
            at,
        }
    }

    fn close(a: &Affine, b: &Affine) -> bool {
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn a_placement_inverts() {
        let m = compose(&[3.0, -2.0, 7.0], &[0.0, 0.0, 0.70710677, 0.70710677]);
        assert!(close(&multiply(&m, &invert(&m).unwrap()), &IDENTITY));
    }

    #[test]
    fn a_quarter_turn_about_z_maps_x_to_y() {
        let m = compose(&[0.0; 3], &[0.0, 0.0, 0.70710677, 0.70710677]);
        let p = apply(&m, &[1.0, 0.0, 0.0]);
        assert!(p[0].abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_coplanar_diagonal_is_no_crease_but_the_rim_is() {
        let quad = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let lines = creases(&instance(quad, vec![0, 1, 2, 0, 2, 3], IDENTITY));
        assert_eq!(lines.len(), 8, "four rim edges, no diagonal");
    }

    #[test]
    fn a_right_angle_fold_is_a_crease() {
        let fold = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        // Share the edge 0-1; the faces meet at 90 degrees.
        let lines = creases(&instance(fold, vec![0, 1, 2, 1, 0, 3], IDENTITY));
        assert_eq!(lines.len(), 10, "the fold plus the four open edges");
    }

    #[test]
    fn merge_keeps_only_the_indexed_vertices_and_decodes_back() {
        let at = compose(&[10.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 1.0]);
        let positions = vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
            [99.0, 99.0, 99.0],
        ];
        let m = merge("gun", &[instance(positions, vec![0, 1, 2], at)], false);
        assert_eq!(m.vertex_count, 3);
        assert!(!m.wide_indices);
        assert_eq!(m.min, [10.0, 0.0, 0.0]);
        assert_eq!(m.max, [12.0, 4.0, 0.0]);
        let decode = |v: usize, k: usize| {
            let at = v * 8 + k * 2;
            let q = i16::from_le_bytes([m.positions[at], m.positions[at + 1]]);
            m.centre[k] + f32::from(q) / 32767.0 * m.scale
        };
        assert!((decode(2, 1) - 4.0).abs() < 1e-3);
        assert!((decode(1, 0) - 12.0).abs() < 1e-3);
    }
}
