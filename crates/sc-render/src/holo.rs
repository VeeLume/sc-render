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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sc_extract::MeshGeometry;
use serde::Serialize;

use crate::{Assets, BoneInfo, MaterialInfo, ScenePart};

/// Two faces meeting at more than this draw a crease, degrees.
const CREASE_ANGLE: f64 = 30.0;
/// Crease lines shorter than this share of the ship's radius are dropped: a
/// pixel or less at the framed view, up to a full-window canvas.
const SHORTEST_LINE: f32 = 1.0 / 600.0;

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

/// The crease lines: pairs of `i16 × 4` positions (normalised), ordered so
/// that drawing any prefix covers the whole ship evenly.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HoloEdges {
    pub centre: [f32; 3],
    pub scale: f32,
    pub vertex_count: u32,
    pub offset: u32,
    /// Prefixes to draw, shortest prefix first.
    pub levels: Vec<HoloEdgeLevel>,
}

/// A prefix of the crease lines.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HoloEdgeLevel {
    pub vertex_count: u32,
    /// Total length of the prefix's lines, metres.
    pub length: f32,
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
        let mut buffer = Vec::new();
        let target = &mut buffer;
        let info = self.holo_into(root, move |size| {
            target.resize(size, 0);
            target.as_mut_slice()
        });
        Holo { info, buffer }
    }

    /// [`holo`](Self::holo), written into the memory `target` hands out for
    /// the size it is told: every core fills it directly, so a host with
    /// memory the page can see copies nothing. `target` must return exactly
    /// that many bytes.
    pub fn holo_into<'b>(
        &self,
        root: &ScenePart,
        target: impl FnOnce(usize) -> &'b mut [u8],
    ) -> HoloInfo {
        let started = Instant::now();
        let mut notes = Vec::new();

        let mut files = Files::default();
        scan(root, &mut files);
        // Scan order, not sorted: the hull comes first, and it is the file
        // everything else waits for.
        let paths: Vec<(&String, &&str)> = files.geometry.iter().map(|(k, p)| (k, p)).collect();
        let read = across_cores(&paths, |(_, path)| {
            MeshGeometry::read_shape(self.source(), path).map(Arc::new)
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

        let mut info = assemble(&ports, target);
        notes.push(format!(
            "holo: {} draws from {meshes_drawn} meshes, {:.0} MB",
            info.ports.len(),
            f64::from(info.byte_length) / 1e6
        ));
        notes.push(format!(
            "holo: files {} ms, built {} ms",
            loaded.as_millis(),
            (started.elapsed() - loaded).as_millis()
        ));
        info.notes = notes;
        info
    }
}

fn align4(n: usize) -> usize {
    n.next_multiple_of(4)
}

/// `f` over `items` on every core; results in `items`' order.
fn across_cores<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    // Three quarters of the logical cores measured best from Arrow to Polaris:
    // past that, threads only fight the one big hull for memory and turbo.
    let workers = (std::thread::available_parallelism().map_or(4, |n| n.get()) * 3 / 4)
        .max(1)
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
    /// In the order the scene names them, root first.
    geometry: Vec<(String, &'a str)>,
    seen: BTreeSet<String>,
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
        if files.seen.insert(key.clone()) {
            files.geometry.push((key, &piece.geometry));
        }
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

/// The frame positions are quantised in: bounds centre and largest half extent.
fn fit(min: &[f32; 3], max: &[f32; 3]) -> ([f32; 3], f32) {
    let centre = std::array::from_fn(|k| (min[k] + max[k]) * 0.5);
    let half = (0..3).map(|k| (max[k] - min[k]) * 0.5).fold(0.0, f32::max);
    (centre, if half > 0.0 { half } else { 1.0 })
}

fn write_position(chunk: &mut [u8; 8], p: &[f32; 3], centre: &[f32; 3], scale: f32) {
    for k in 0..3 {
        let q = ((p[k] - centre[k]) / scale * 32767.0).round() as i16;
        chunk[k * 2..k * 2 + 2].copy_from_slice(&q.to_le_bytes());
    }
}

fn write_normal(chunk: &mut [u8; 4], n: &[f32; 3]) {
    for k in 0..3 {
        chunk[k] = ((n[k] * 127.0).round() as i8) as u8;
    }
}

/// Where each vertex a submesh indexes lands in its own output (every submesh
/// of a file shares the file's whole vertex buffer).
enum Slots {
    /// A submesh's vertices are a run of the file's: a table over that run.
    Run { lo: u32, table: Vec<u32> },
    /// A scattered one must not allocate the whole file.
    Scattered(QuickMap<u32, u32>),
}

impl Slots {
    fn new(source: &[u32]) -> Self {
        let lo = source.iter().copied().min().unwrap_or(0);
        let hi = source.iter().copied().max().unwrap_or(0);
        let span = (hi - lo) as usize + 1;
        if span <= source.len().saturating_mul(8).max(1024) {
            Slots::Run {
                lo,
                table: vec![u32::MAX; span],
            }
        } else {
            Slots::Scattered(QuickMap::default())
        }
    }

    /// `v`'s output vertex, which is `next` if `v` is new; and whether it is.
    fn slot(&mut self, v: u32, next: u32) -> (u32, bool) {
        match self {
            Slots::Run { lo, table } => {
                let s = &mut table[(v - *lo) as usize];
                if *s == u32::MAX {
                    *s = next;
                    (next, true)
                } else {
                    (*s, false)
                }
            }
            Slots::Scattered(map) => {
                let at = *map.entry(v).or_insert(next);
                (at, at == next)
            }
        }
    }
}

/// What one submesh will take up, before anything is written.
struct Survey {
    vertices: u32,
    min: [f32; 3],
    max: [f32; 3],
}

fn survey(inst: &Instance) -> Survey {
    let source = inst.indices();
    let mut slots = Slots::new(source);
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let mut vertices = 0;
    for &v in source {
        if slots.slot(v, vertices).1 {
            vertices += 1;
            let p = apply(&inst.at, &inst.mesh.positions[v as usize]);
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    Survey { vertices, min, max }
}

/// One submesh's share of the buffer.
struct Job<'a> {
    inst: &'a Instance,
    positions: &'a mut [u8],
    normals: &'a mut [u8],
    indices: &'a mut [u8],
    /// Its port's vertices before this submesh.
    base: u32,
    wide: bool,
    centre: [f32; 3],
    scale: f32,
}

fn fill(job: &mut Job) {
    let inst = job.inst;
    let g = &inst.mesh;
    let source = inst.indices();
    let has_normals = g.normals.len() == g.positions.len();
    let nm = normal_matrix(&inst.at);
    let positions = job.positions.as_chunks_mut::<8>().0;
    let normals = job.normals.as_chunks_mut::<4>().0;
    let mut slots = Slots::new(source);
    let mut next = 0;
    // Only geometry that came without normals needs these.
    let (mut world, mut local) = (Vec::new(), Vec::new());
    for (k, &v) in source.iter().enumerate() {
        let (at, new) = slots.slot(v, next);
        if new {
            let i = v as usize;
            let p = apply(&inst.at, &g.positions[i]);
            write_position(&mut positions[next as usize], &p, &job.centre, job.scale);
            if has_normals {
                let n = apply_normal(&nm, &g.normals[i]);
                write_normal(&mut normals[next as usize], &n);
            } else {
                world.push(p);
            }
            next += 1;
        }
        if !has_normals {
            local.push(at);
        }
        let index = job.base + at;
        if job.wide {
            job.indices[k * 4..k * 4 + 4].copy_from_slice(&index.to_le_bytes());
        } else {
            job.indices[k * 2..k * 2 + 2].copy_from_slice(&(index as u16).to_le_bytes());
        }
    }
    if !has_normals {
        for (chunk, n) in normals.iter_mut().zip(face_normals(&world, &local)) {
            write_normal(chunk, &n);
        }
    }
}

/// Every part's crease lines, not only the hull's: without them a dense
/// mounted item (a Centaurus engine) reads as a flat patch over the ship.
/// A submesh used many times (a turret, a door) is searched once, and lines
/// too short to see at the framed view are left out.
fn crease_lines(flat: &[&Instance], surveys: &[Survey]) -> Vec<[f32; 3]> {
    let mut seen: HashMap<(*const MeshGeometry, usize), usize> = HashMap::new();
    let mut distinct: Vec<&Instance> = Vec::new();
    let mut which = Vec::with_capacity(flat.len());
    for inst in flat {
        let next = distinct.len();
        let at = *seen
            .entry((Arc::as_ptr(&inst.mesh), inst.submesh))
            .or_insert(next);
        if at == next {
            distinct.push(inst);
        }
        which.push(at);
    }
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for s in surveys.iter().filter(|s| s.vertices > 0) {
        for k in 0..3 {
            min[k] = min[k].min(s.min[k]);
            max[k] = max[k].max(s.max[k]);
        }
    }
    let radius = (0..3)
        .map(|k| (max[k] - min[k]) * 0.5)
        .map(|h| h * h)
        .sum::<f32>()
        .sqrt();
    let shortest = if radius.is_finite() {
        radius * SHORTEST_LINE
    } else {
        0.0
    };
    let pairs = across_cores(&distinct, |inst| creases(inst, shortest));
    let jobs: Vec<(&Instance, usize)> = flat.iter().copied().zip(which).collect();
    across_cores(&jobs, |(inst, at)| place_lines(inst, &pairs[*at], shortest)).concat()
}

/// Point pairs reordered so any prefix covers the ship evenly: the ship is cut
/// into cells, and each cell gives up its longest line first, then its second
/// longest, and so on. Ranking by length across the whole ship left some
/// parts full of long lines beside bare neighbours, which read as streaks.
/// Returns the prefix ending each rank class.
fn spread_evenly(points: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<HoloEdgeLevel>) {
    const CLASSES: usize = 32;
    /// Cells across the ship's radius: a few pixels each at card size.
    const CELLS_PER_RADIUS: f32 = 30.0;
    let pairs = points.as_chunks::<2>().0;
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in points {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let radius = (0..3)
        .map(|k| ((max[k] - min[k]) * 0.5).powi(2))
        .sum::<f32>()
        .sqrt();
    let cell = (radius / CELLS_PER_RADIUS).max(1e-3);
    let mut measured = Vec::with_capacity(pairs.len());
    let mut cells: QuickMap<u64, Vec<(f32, u32)>> = QuickMap::default();
    for (i, [p, q]) in pairs.iter().enumerate() {
        let (x, y, z) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
        let len = (x * x + y * y + z * z).sqrt();
        measured.push(len);
        let at = |k: usize| (((p[k] + q[k]) * 0.5 - min[k]) / cell) as u64 & 0x1F_FFFF;
        cells
            .entry(at(0) << 42 | at(1) << 21 | at(2))
            .or_default()
            .push((len, i as u32));
    }
    let cells: Vec<Vec<(f32, u32)>> = cells.into_values().collect();
    let ranked = across_cores(&cells, |lines| {
        let mut sorted = lines.clone();
        sorted.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        sorted
    });
    // Every early rank has its own class, then they widen: the budget needs
    // fine steps where it spends most of its time.
    let class_of = |rank: usize| {
        if rank < 8 {
            rank
        } else {
            (8 + ((rank as f32 / 8.0).log2() * 3.0) as usize).min(CLASSES - 1)
        }
    };
    let mut classes = vec![0u8; pairs.len()];
    for lines in &ranked {
        for (rank, &(_, i)) in lines.iter().enumerate() {
            classes[i as usize] = class_of(rank) as u8;
        }
    }
    let mut counts = [0usize; CLASSES];
    let mut lengths = [0.0f32; CLASSES];
    for (&c, &len) in classes.iter().zip(&measured) {
        counts[usize::from(c)] += 1;
        lengths[usize::from(c)] += len;
    }
    let mut next = [0usize; CLASSES];
    let mut levels = Vec::new();
    let (mut at, mut total) = (0, 0.0);
    for c in 0..CLASSES {
        next[c] = at;
        at += counts[c];
        total += lengths[c];
        if counts[c] > 0 {
            levels.push(HoloEdgeLevel {
                vertex_count: at as u32 * 2,
                length: total,
            });
        }
    }
    let mut out = vec![[0.0; 3]; points.len()];
    for (pair, &c) in pairs.iter().zip(&classes) {
        let c = usize::from(c);
        out[next[c] * 2] = pair[0];
        out[next[c] * 2 + 1] = pair[1];
        next[c] += 1;
    }
    (out, levels)
}

/// Cut the next `len` bytes at `offset` off `rest`, which starts at `cursor`.
fn carve<'a>(
    rest: &mut &'a mut [u8],
    cursor: &mut usize,
    offset: usize,
    len: usize,
) -> &'a mut [u8] {
    let (_, tail) = std::mem::take(rest).split_at_mut(offset - *cursor);
    let (piece, tail) = tail.split_at_mut(len);
    *rest = tail;
    *cursor = offset + len;
    piece
}

/// `n` bytes off the front of `slice`.
fn front<'a>(slice: &mut &'a mut [u8], n: usize) -> &'a mut [u8] {
    let (head, tail) = std::mem::take(slice).split_at_mut(n);
    *slice = tail;
    head
}

/// Size every port and crease line, ask `target` for exactly that much
/// memory, and fill it across every core.
fn assemble<'b>(
    ports: &[(String, Vec<Instance>)],
    target: impl FnOnce(usize) -> &'b mut [u8],
) -> HoloInfo {
    let flat: Vec<&Instance> = ports.iter().flat_map(|(_, list)| list.iter()).collect();
    let surveys = across_cores(&flat, |inst| survey(inst));
    let (lines, levels) = spread_evenly(&crease_lines(&flat, &surveys));

    let mut info = HoloInfo::default();
    let mut size = 0;
    let mut at = 0;
    for (path, list) in ports {
        let these = &surveys[at..at + list.len()];
        at += list.len();
        let vertex_count: u32 = these.iter().map(|s| s.vertices).sum();
        let index_count: usize = list.iter().map(|i| i.indices().len()).sum();
        let (mut min, mut max) = ([0.0; 3], [0.0; 3]);
        if vertex_count > 0 {
            (min, max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for s in these {
                for k in 0..3 {
                    min[k] = min[k].min(s.min[k]);
                    max[k] = max[k].max(s.max[k]);
                }
            }
        }
        let (centre, scale) = fit(&min, &max);
        let wide_indices = vertex_count > u32::from(u16::MAX);
        let positions_offset = align4(size);
        let normals_offset = align4(positions_offset + vertex_count as usize * 8);
        let indices_offset = align4(normals_offset + vertex_count as usize * 4);
        size = indices_offset + index_count * if wide_indices { 4 } else { 2 };
        info.ports.push(HoloPort {
            path: path.clone(),
            centre,
            scale,
            min,
            max,
            vertex_count,
            index_count: index_count as u32,
            positions_offset: positions_offset as u32,
            normals_offset: normals_offset as u32,
            indices_offset: indices_offset as u32,
            wide_indices,
        });
    }

    let edge_frame = (!lines.is_empty()).then(|| {
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &lines {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        let (centre, scale) = fit(&min, &max);
        let offset = align4(size);
        size = offset + lines.len() * 8;
        info.edges = Some(HoloEdges {
            centre,
            scale,
            vertex_count: lines.len() as u32,
            offset: offset as u32,
            levels,
        });
        (offset, centre, scale)
    });
    info.byte_length = size as u32;

    let out = target(size);
    assert_eq!(
        out.len(),
        size,
        "target must hand out exactly the size asked"
    );
    let mut rest = out;
    let mut cursor = 0;
    let mut jobs = Vec::with_capacity(flat.len());
    let mut at = 0;
    for (p, (_, list)) in info.ports.iter().zip(ports) {
        let vertices = p.vertex_count as usize;
        let width = if p.wide_indices { 4 } else { 2 };
        let mut positions = carve(
            &mut rest,
            &mut cursor,
            p.positions_offset as usize,
            vertices * 8,
        );
        let mut normals = carve(
            &mut rest,
            &mut cursor,
            p.normals_offset as usize,
            vertices * 4,
        );
        let mut indices = carve(
            &mut rest,
            &mut cursor,
            p.indices_offset as usize,
            p.index_count as usize * width,
        );
        let mut base = 0;
        for inst in list {
            let n = surveys[at].vertices;
            at += 1;
            jobs.push(Mutex::new(Job {
                inst,
                positions: front(&mut positions, n as usize * 8),
                normals: front(&mut normals, n as usize * 4),
                indices: front(&mut indices, inst.indices().len() * width),
                base,
                wide: p.wide_indices,
                centre: p.centre,
                scale: p.scale,
            }));
            base += n;
        }
    }
    if let Some((offset, centre, scale)) = edge_frame {
        let bytes = carve(&mut rest, &mut cursor, offset, lines.len() * 8);
        for (chunk, p) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(&lines) {
            write_position(chunk, p, &centre, scale);
        }
    }
    across_cores(&jobs, |job| fill(&mut job.lock().unwrap()));
    info
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

/// A position, welded to a tenth of a millimetre, as a 64-bit key. Collisions
/// are about one in 10^7 even on a capital ship's hull, and would only cost
/// one line.
fn weld_key(p: &[f32; 3]) -> u64 {
    p.iter().fold(0u64, |h, &c| {
        let q = (f64::from(c) * 1e4).round() as i64 as u64;
        (h.rotate_left(23) ^ q).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    })
}

/// Direction matters: an edge and its reverse must get different keys.
fn edge_key(a: u64, b: u64) -> u64 {
    a.rotate_left(29).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ b
}

/// One submesh's crease lines as vertex pairs: an edge where two faces meet
/// beyond [`CREASE_ANGLE`], or with one face only. Edges shorter than
/// `shortest` never reach the table: most of a hull's are, and they could
/// not be drawn anyway.
fn creases(inst: &Instance, shortest: f32) -> Vec<[u32; 2]> {
    let positions = &inst.mesh.positions;
    let indices = inst.indices();
    let threshold = CREASE_ANGLE.to_radians().cos();
    let least = shortest * shortest;
    let long = |a: u32, b: u32| {
        let (p, q) = (positions[a as usize], positions[b as usize]);
        let (x, y, z) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
        x * x + y * y + z * z >= least
    };
    // Sized up front: growing the table was most of the cost.
    let mut open: QuickMap<u64, Option<OpenEdge>> =
        QuickMap::with_capacity_and_hasher(indices.len() / 2, Default::default());
    let mut lines = Vec::new();
    for t in indices.as_chunks::<3>().0 {
        let keep = [long(t[0], t[1]), long(t[1], t[2]), long(t[2], t[0])];
        if keep == [false; 3] {
            continue;
        }
        let ids = [t[0], t[1], t[2]].map(|v| weld_key(&positions[v as usize]));
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
            continue;
        }
        let normal = triangle_normal(t.iter().map(|&v| positions[v as usize]));
        for j in 0..3 {
            if !keep[j] {
                continue;
            }
            let (next, a, b) = ((j + 1) % 3, t[j], t[(j + 1) % 3]);
            let edge = edge_key(ids[j], ids[next]);
            let reverse = edge_key(ids[next], ids[j]);
            if let Some(slot) = open.get_mut(&reverse)
                && let Some((_, _, other)) = *slot
            {
                let dot = normal[0] * other[0] + normal[1] * other[1] + normal[2] * other[2];
                if dot <= threshold {
                    lines.push([a, b]);
                }
                *slot = None;
            } else {
                open.entry(edge).or_insert(Some((a, b, normal)));
            }
        }
    }
    lines.extend(open.into_values().flatten().map(|(a, b, _)| [a, b]));
    lines
}

/// `inst`'s crease lines as point pairs in the ship frame, without those
/// shorter than `shortest`.
fn place_lines(inst: &Instance, pairs: &[[u32; 2]], shortest: f32) -> Vec<[f32; 3]> {
    let least = shortest * shortest;
    let mut out = Vec::with_capacity(pairs.len());
    for &[a, b] in pairs {
        let p = apply(&inst.at, &inst.mesh.positions[a as usize]);
        let q = apply(&inst.at, &inst.mesh.positions[b as usize]);
        let (x, y, z) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
        if x * x + y * y + z * z >= least {
            out.push(p);
            out.push(q);
        }
    }
    out
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
        let lines = creases(&instance(quad, vec![0, 1, 2, 0, 2, 3], IDENTITY), 0.0);
        assert_eq!(lines.len(), 4, "four rim edges, no diagonal");
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
        let lines = creases(&instance(fold, vec![0, 1, 2, 1, 0, 3], IDENTITY), 0.0);
        assert_eq!(lines.len(), 5, "the fold plus the four open edges");
    }

    #[test]
    fn every_cell_gives_up_its_longest_line_before_any_second_line() {
        let at = |x: f32| [x, 0.0, 0.0];
        // Two clusters far apart: the near one's second line is longer than
        // the far one's first, and must still wait for it.
        let points = [
            at(0.0),
            at(4.0),
            at(0.0),
            at(3.0),
            at(1000.0),
            at(1002.0),
            at(1000.0),
            at(1001.0),
        ];
        let (out, levels) = spread_evenly(&points);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].vertex_count, 4);
        assert_eq!(levels[1].vertex_count, 8);
        assert!((levels[0].length - 6.0).abs() < 1e-3, "4 m and 2 m first");
        assert!((levels[1].length - 10.0).abs() < 1e-3);
        let first: Vec<f32> = out[..4].iter().map(|p| p[0]).collect();
        assert!(first.contains(&4.0) && first.contains(&1002.0));
    }

    #[test]
    fn lines_shorter_than_the_limit_are_dropped() {
        let positions = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.01, 0.0, 0.0]];
        let inst = instance(positions, vec![0, 1, 2], IDENTITY);
        let lines = place_lines(&inst, &[[0, 1], [1, 2]], 1.0);
        assert_eq!(lines.len(), 2, "one line kept, as a point pair");
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
        let ports = vec![(
            "gun".to_string(),
            vec![instance(positions, vec![0, 1, 2], at)],
        )];
        let mut buffer = Vec::new();
        let target = &mut buffer;
        let info = assemble(&ports, move |size| {
            target.resize(size, 0);
            target.as_mut_slice()
        });
        let m = &info.ports[0];
        assert_eq!(m.vertex_count, 3);
        assert!(!m.wide_indices);
        assert_eq!(m.min, [10.0, 0.0, 0.0]);
        assert_eq!(m.max, [12.0, 4.0, 0.0]);
        let decode = |v: usize, k: usize| {
            let at = m.positions_offset as usize + v * 8 + k * 2;
            let q = i16::from_le_bytes([buffer[at], buffer[at + 1]]);
            m.centre[k] + f32::from(q) / 32767.0 * m.scale
        };
        assert!((decode(2, 1) - 4.0).abs() < 1e-3);
        assert!((decode(1, 0) - 12.0).abs() < 1e-3);
    }

    #[test]
    fn two_submeshes_index_into_one_merged_vertex_list() {
        let a = instance(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
            IDENTITY,
        );
        let b = instance(
            vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
            vec![2, 1, 0],
            IDENTITY,
        );
        let ports = vec![("gun".to_string(), vec![a, b])];
        let mut buffer = Vec::new();
        let target = &mut buffer;
        let info = assemble(&ports, move |size| {
            target.resize(size, 0);
            target.as_mut_slice()
        });
        let m = &info.ports[0];
        assert_eq!((m.vertex_count, m.index_count), (6, 6));
        let index = |i: usize| {
            let at = m.indices_offset as usize + i * 2;
            u16::from_le_bytes([buffer[at], buffer[at + 1]])
        };
        assert_eq!((0..6).map(index).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_scattered_submesh_dedupes_like_a_compact_one() {
        let mut slots = Slots::new(&[0, 1_000_000]);
        assert!(matches!(slots, Slots::Scattered(_)));
        assert_eq!(slots.slot(1_000_000, 0), (0, true));
        assert_eq!(slots.slot(0, 1), (1, true));
        assert_eq!(slots.slot(1_000_000, 2), (0, false));
    }
}
