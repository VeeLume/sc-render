//! Resolve a Starlume `inventory.json` dump to record names and print the
//! item trees whose record name matches a filter, with each child's port CRC.
//!
//! ```bash
//! cargo run -p lab-catalog --release --example inventory_tree -- <inventory.json> <filter>...
//! cargo run -p lab-catalog --release --example inventory_tree -- --crc <class crc>...
//! ```

use sc_extract::{AssetConfig, AssetData, AssetSource, CrcIndex, Datacore, RecordPaths};
use serde_json::Value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("inventory.json path")?;
    let filters: Vec<String> = args.map(|a| a.to_lowercase()).collect();
    let install = sc_discovery::discover_primary()?;
    let assets = AssetSource::from_install(&install)?;
    let data = AssetData::extract(&assets, &AssetConfig::standard())?;
    let dc = Datacore::parse(&assets, &data)?;
    let paths = RecordPaths::build(&dc);
    let crcs = CrcIndex::from_paths(&paths);
    let name = |crc: u64| {
        crcs.guid(crc as u32)
            .and_then(|g| paths.get(&g))
            .map(|r| format!("{} [{}] {}", r.name, r.guid, r.path))
            .unwrap_or_else(|| format!("?crc {crc}"))
    };
    // `--crc <n>...` instead of a file: resolve bare class CRCs.
    if file == "--crc" {
        for f in &filters {
            println!("{f}: {}", name(f.parse()?));
        }
        return Ok(());
    }
    let inv: Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    fn walk(v: &Value, depth: usize, name: &dyn Fn(u64) -> String, out: &mut Vec<String>) {
        let crc = v["class_crc"].as_u64().unwrap_or(0);
        out.push(format!(
            "{}{} port={} geid={}",
            "  ".repeat(depth),
            name(crc),
            v["port_crc"],
            v["geid"]
        ));
        for k in ["attached", "contents"] {
            for c in v[k].as_array().into_iter().flatten() {
                walk(c, depth + 1, name, out);
            }
        }
    }
    for stash in inv["stashes"].as_array().into_iter().flatten() {
        for item in stash["items"].as_array().into_iter().flatten() {
            let mut out = Vec::new();
            walk(item, 0, &name, &mut out);
            let text = out.join("\n");
            if filters.iter().any(|f| text.to_lowercase().contains(f)) {
                println!("{}\n{text}\n", stash["location"]);
            }
        }
    }
    Ok(())
}
