//! Print generated scenes: items by record name (default loadout), and the
//! items of an inventory dump whose label or record matches a filter (their
//! real attachments).
//!
//! ```bash
//! cargo run -p render-assets --release --example scene_probe -- <record name>...
//! cargo run -p render-assets --release --example scene_probe -- --inventory <inventory.json> <filter>
//! cargo run -p render-assets --release --example scene_probe -- --coverage
//! ```

use render_assets::{Assets, Body, Catalog, ChildSpec, InventoryNode};

fn print(s: &render_assets::SceneItem) {
    println!("{}", serde_json::to_string_pretty(s).unwrap_or_default());
}

fn children(n: &InventoryNode) -> Vec<ChildSpec> {
    n.attached
        .iter()
        .filter_map(|c| {
            Some(ChildSpec {
                port: c.port.clone()?,
                guid: c.guid.clone()?,
                children: Some(children(c)),
            })
        })
        .collect()
}

fn find<'a>(nodes: &'a [InventoryNode], filter: &str, out: &mut Vec<&'a InventoryNode>) {
    for n in nodes {
        if n.label.to_lowercase().contains(filter) {
            out.push(n);
        }
        find(&n.attached, filter, out);
        find(&n.contents, filter, out);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = Assets::open()?;
    let t = std::time::Instant::now();
    let catalog = Catalog::load(&assets)?;
    eprintln!("catalog in {:?}", t.elapsed());
    if args.first().map(String::as_str) == Some("--coverage") {
        use std::collections::BTreeMap;
        let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut errors: BTreeMap<String, (usize, String)> = BTreeMap::new();
        let mut notes: BTreeMap<String, usize> = BTreeMap::new();
        for s in catalog.all() {
            let kind = s.kind.split('/').next().unwrap_or_default().to_string();
            let e = by_kind.entry(kind).or_default();
            e.0 += 1;
            match catalog.scene(&assets, &s.guid, None, Body::default()) {
                Ok(scene) => {
                    for n in scene.notes {
                        let key = n.split_once(": ").map_or(n.as_str(), |(_, r)| r);
                        let key: String = key
                            .chars()
                            .filter(|c| !c.is_ascii_digit())
                            .take(50)
                            .collect();
                        *notes.entry(key).or_default() += 1;
                    }
                }
                Err(err) => {
                    e.1 += 1;
                    let msg = err.to_string();
                    let key = msg.split_once(": ").map_or(msg.as_str(), |(_, r)| r);
                    let key: String = key.chars().take(60).collect();
                    let slot = errors.entry(key).or_insert((0, s.record.clone()));
                    slot.0 += 1;
                }
            }
        }
        println!("kind: items / failed");
        for (k, (n, f)) in &by_kind {
            println!("  {k:30} {n:6} {f:6}");
        }
        println!("errors (count, example):");
        for (k, (n, ex)) in &errors {
            println!("  {n:6}  {k}  [{ex}]");
        }
        println!("notes:");
        let mut notes: Vec<_> = notes.into_iter().collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.1));
        for (k, n) in notes.iter().take(15) {
            println!("  {n:6}  {k}");
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--inventory") {
        let nodes = catalog.inventory(&std::fs::read_to_string(&args[1])?)?;
        let mut hits = Vec::new();
        find(&nodes, &args[2].to_lowercase(), &mut hits);
        for n in hits {
            let Some(guid) = &n.guid else { continue };
            print(&catalog.scene(&assets, guid, Some(&children(n)), Body::default())?);
        }
        return Ok(());
    }
    for name in &args {
        let hit = catalog
            .search(name, 50)
            .into_iter()
            .find(|h| h.record.eq_ignore_ascii_case(name))
            .ok_or("no such record")?;
        print(&catalog.scene(&assets, &hit.guid, None, Body::default())?);
    }
    Ok(())
}
