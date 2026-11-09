//! Where a ship's first open goes on the Rust side: assembling the scene,
//! reading and packing its geometry files, reading its materials, one at a
//! time and across every core.
//!
//! ```bash
//! cargo run -p lab-catalog --release --example ship_load_cost -- Gladius Idris
//! ```
//!
//! `--indexed` cooks an asset index and checks that every ship's scene and
//! holo come out the same through it, with the archive's file list unloaded.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use lab_catalog::Catalog;
use sc_extract::MeshGeometry;
use sc_render::{Assets, HoloRequest, ScenePart, ShipScene};

fn collect(p: &ScenePart, pieces: &mut usize, files: &mut BTreeMap<String, String>) {
    for piece in &p.pieces {
        *pieces += 1;
        files
            .entry(piece.geometry.to_ascii_lowercase())
            .or_insert_with(|| piece.geometry.clone());
    }
    for a in &p.attachments {
        collect(&a.part, pieces, files);
    }
}

/// `f` over `items` on every core, in no particular order.
fn across_cores<T: Send>(items: &[String], f: impl Fn(&str) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..std::thread::available_parallelism().map_or(4, |n| n.get()) {
            s.spawn(|| {
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let v = f(item);
                    out.lock().unwrap().push(v);
                }
            });
        }
    });
    out.into_inner().unwrap()
}

fn through_index(
    catalog: &Catalog,
    full: &Assets,
    ships: &[lab_catalog::ShipSummary],
) -> Result<(), Box<dyn std::error::Error>> {
    let t = Instant::now();
    let mut builder = sc_extract::AssetIndex::builder(full.source())?;
    catalog.cook_inputs().0.add_files(&mut builder);
    let index = builder.finish();
    let dir = std::env::temp_dir().join("sc-render-index-check");
    std::fs::create_dir_all(&dir)?;
    let index_file = dir.join("assets.index");
    std::fs::write(&index_file, index.to_bytes())?;
    println!("cooked {} paths in {:?}", index.len(), t.elapsed());

    let p4k = sc_discovery::discover_primary()?.data_p4k();
    let t = Instant::now();
    let indexed = Assets::open_at_indexed(&p4k, &index_file)?;
    println!("indexed open {:?}", t.elapsed());
    assert!(
        !indexed.source().holds_file_list(),
        "the index was not used"
    );

    let request = HoloRequest {
        level: 2,
        ..HoloRequest::default()
    };
    let (mut same, mut different) = (0, Vec::new());
    let mut through_index = std::time::Duration::ZERO;
    for s in ships {
        let drawn = |assets: &Assets| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let scene = catalog.ship_scene(assets, &s.guid)?;
            let scene = ShipScene::new(&scene, s.label.clone(), |g| g.to_string());
            Ok(assets.holo(&scene.root, &request).buffer)
        };
        let holo = drawn(&indexed)?;
        // The first build warmed the OS file cache, as an earlier open has.
        let t = Instant::now();
        drawn(&indexed)?;
        through_index += t.elapsed();
        if holo == drawn(full)? {
            same += 1;
        } else {
            different.push(s.label.clone());
        }
    }
    println!(
        "{same} of {} ships build the same holo; different: {different:?}",
        ships.len()
    );
    println!("every ship's scene and holo through the index, warm: {through_index:?}");
    println!(
        "file list loaded along the way: {}",
        indexed.source().holds_file_list()
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets::open()?;
    let t = Instant::now();
    let catalog = Catalog::load(&assets)?;
    println!("catalog {:?}", t.elapsed());

    let (visuals, palettes) = catalog.cook_inputs();
    let t = Instant::now();
    let json = serde_json::to_vec(&(visuals, palettes))?;
    println!(
        "Visuals + Palettes: {:.1} MB as JSON, written in {:?}",
        json.len() as f64 / 1e6,
        t.elapsed()
    );
    let t = Instant::now();
    let _: serde_json::Value = serde_json::from_slice(&json)?;
    println!("  parsed as a JSON value in {:?}", t.elapsed());

    let ships = catalog.ships();
    if std::env::args().nth(1).as_deref() == Some("--indexed") {
        return through_index(&catalog, &assets, &ships);
    }
    if std::env::args().nth(1).as_deref() == Some("--repeat") {
        for name in std::env::args().skip(2) {
            let needle = name.to_lowercase();
            let Some(hit) = ships
                .iter()
                .find(|s| s.label.to_lowercase().contains(&needle))
            else {
                continue;
            };
            let scene = catalog.ship_scene(&assets, &hit.guid)?;
            let scene = ShipScene::new(&scene, hit.label.clone(), |g| g.to_string());
            let request = HoloRequest {
                level: 2,
                ..HoloRequest::default()
            };
            let _ = assets.holo(&scene.root, &request);
            let mut runs = Vec::new();
            for _ in 0..7 {
                let t = Instant::now();
                let holo = assets.holo(&scene.root, &request);
                runs.push((
                    t.elapsed().as_millis(),
                    holo.info.notes.last().cloned().unwrap_or_default(),
                ));
            }
            runs.sort();
            let (ms, note) = &runs[3];
            println!("{name:10} median of 7: {ms} ms ({note})");
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--all") {
        let mut rows = Vec::new();
        for s in &ships {
            let scene = catalog.ship_scene(&assets, &s.guid)?;
            let scene = ShipScene::new(&scene, s.label.clone(), |g| g.to_string());
            // The first pass warms the OS file cache, as an earlier open has.
            let request = HoloRequest {
                level: 2,
                ..HoloRequest::default()
            };
            let t = Instant::now();
            let cold = assets.holo(&scene.root, &request);
            let first = t.elapsed();
            drop(cold);
            let t = Instant::now();
            let holo = assets.holo(&scene.root, &request);
            let triangles: u32 = holo
                .info
                .parts
                .iter()
                .map(|p| holo.info.geometries[p.geometry as usize].index_count / 3)
                .sum();
            let phases = holo.info.notes.last().cloned().unwrap_or_default();
            let phases = phases.trim_start_matches("holo: ").to_string();
            let label = format!(
                "{}  (first pass {} ms; {phases})",
                s.label,
                first.as_millis()
            );
            rows.push((t.elapsed(), triangles, holo.buffer.len(), label));
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        let over = |ms: u128| rows.iter().filter(|r| r.0.as_millis() > ms).count();
        println!(
            "{} ships; Rust build over 500 ms: {}, over 750 ms: {}, over 1000 ms: {}",
            rows.len(),
            over(500),
            over(750),
            over(1000)
        );
        println!("median {:?}", rows[rows.len() / 2].0);
        for (time, triangles, bytes, label) in rows.iter().take(25) {
            println!(
                "{:>6} ms {:>6.2} M tris {:>5.0} MB  {label}",
                time.as_millis(),
                *triangles as f64 / 1e6,
                *bytes as f64 / 1e6
            );
        }
        return Ok(());
    }
    for name in std::env::args().skip(1) {
        let needle = name.to_lowercase();
        let Some(hit) = ships
            .iter()
            .find(|s| s.label.to_lowercase().contains(&needle))
        else {
            println!("no ship {name}");
            continue;
        };
        println!("\n{}", hit.label);
        let t = Instant::now();
        catalog.ship_scene(&assets, &hit.guid)?;
        println!("  assemble Scene            {:?}", t.elapsed());
        let t = Instant::now();
        let ship = catalog.ship(&assets, &hit.guid)?;
        println!("  whole lab ship view       {:?}", t.elapsed());

        let mut pieces = 0;
        let mut files = BTreeMap::new();
        collect(&ship.scene.root, &mut pieces, &mut files);
        let paths: Vec<String> = files.into_values().collect();
        println!("  {pieces} pieces, {} unique geometry files", paths.len());

        let t = Instant::now();
        let meshes: Vec<_> = paths
            .iter()
            .filter_map(|p| MeshGeometry::read(assets.source(), p).ok())
            .collect();
        println!("  read geometry, 1 core     {:?}", t.elapsed());
        let (mut vertices, mut indices) = (0usize, 0usize);
        for m in &meshes {
            vertices += m.positions.len();
            indices += m.indices.len();
        }
        println!(
            "  in those files: {:.2} M vertices, {:.2} M triangles",
            vertices as f64 / 1e6,
            indices as f64 / 3e6
        );
        drop(meshes);

        let mut timed = across_cores(&paths, |p| {
            let t = Instant::now();
            let data = format!("Data\\{p}");
            let bytes = assets.source().read(&data).map_or(0, |b| b.len())
                + assets
                    .source()
                    .read(&format!("{data}m"))
                    .map_or(0, |b| b.len());
            let archive = t.elapsed();
            let t = Instant::now();
            let vertices = MeshGeometry::read(assets.source(), p).map_or(0, |g| g.positions.len());
            (t.elapsed(), archive, bytes, vertices, p.to_string())
        });
        timed.sort_by_key(|t| std::cmp::Reverse(t.0));
        for (whole, archive, bytes, vertices, path) in timed.iter().take(3) {
            println!(
                "    {whole:>10.1?} read ({archive:.1?} from the archive, {:.1} MB, {:.2} M vertices) {}",
                *bytes as f64 / 1e6,
                *vertices as f64 / 1e6,
                path.rsplit('/').next().unwrap_or(path)
            );
        }

        let t = Instant::now();
        let packed = across_cores(&paths, |p| assets.model(p).ok());
        println!("  read + pack, all cores    {:?}", t.elapsed());
        let bytes: usize = packed.iter().flatten().map(|m| m.buffer.len()).sum();
        let failed = packed.iter().filter(|m| m.is_none()).count();
        println!(
            "  packed {:.0} MB; {failed} files unreadable",
            bytes as f64 / 1e6
        );
        let mtls: Vec<String> = packed
            .iter()
            .flatten()
            .filter_map(|m| m.info.material.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        drop(packed);

        let t = Instant::now();
        let read = across_cores(&mtls, |p| assets.material(p).is_ok());
        println!(
            "  {} materials, all cores    {:?} ({} failed)",
            mtls.len(),
            t.elapsed(),
            read.iter().filter(|ok| !**ok).count()
        );
        let t = Instant::now();
        for p in &mtls {
            let _ = assets.material(p);
        }
        println!("  materials, 1 core         {:?}", t.elapsed());
    }
    Ok(())
}
