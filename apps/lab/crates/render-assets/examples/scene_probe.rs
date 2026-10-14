//! Print generated scenes: items by record name (default loadout), and the
//! items of an inventory dump whose label or record matches a filter (their
//! real attachments).
//!
//! ```bash
//! cargo run -p render-assets --release --example scene_probe -- <record name>...
//! cargo run -p render-assets --release --example scene_probe -- --inventory <inventory.json> <filter>
//! ```

use render_assets::{Assets, Catalog, ChildSpec, InventoryNode};

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
    if args.first().map(String::as_str) == Some("--inventory") {
        let nodes = catalog.inventory(&std::fs::read_to_string(&args[1])?)?;
        let mut hits = Vec::new();
        find(&nodes, &args[2].to_lowercase(), &mut hits);
        for n in hits {
            let Some(guid) = &n.guid else { continue };
            print(&catalog.scene(&assets, guid, Some(&children(n)))?);
        }
        return Ok(());
    }
    for name in &args {
        let hit = catalog
            .search(name, 50)
            .into_iter()
            .find(|h| h.record.eq_ignore_ascii_case(name))
            .ok_or("no such record")?;
        print(&catalog.scene(&assets, &hit.guid, None)?);
    }
    Ok(())
}
