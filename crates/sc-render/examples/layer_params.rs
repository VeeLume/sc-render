//! Tabulate the `<PublicParams>` of every layer material
//! (`materials/layers/*.mtl`): one line per submaterial with the requested
//! keys, then a value histogram per key.
//!
//! ```bash
//! cargo run -p render-assets --release --example layer_params -- TintMode LayerTiling ShadingModel
//! ```

use std::collections::BTreeMap;

use sc_extract::{AssetSource, Material};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let keys: Vec<String> = std::env::args().skip(1).collect();
    let install = sc_discovery::discover_primary()?;
    let assets = AssetSource::from_install(&install)?;
    let paths: Vec<String> = assets
        .find(|name| {
            let p = name.to_lowercase().replace('\\', "/");
            p.contains("materials/layers/") && p.ends_with(".mtl")
        })
        .map(|e| e.name.to_string())
        .collect();
    let mut histogram: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    for path in &paths {
        let Ok(mtl) = Material::read(&assets, path) else {
            continue;
        };
        for s in &mtl.submaterials {
            let values: Vec<String> = keys
                .iter()
                .map(|k| {
                    s.params
                        .iter()
                        .find(|(key, _)| key == k)
                        .map_or("-".to_string(), |(_, v)| v.clone())
                })
                .collect();
            for (k, v) in keys.iter().zip(&values) {
                *histogram
                    .entry(k)
                    .or_default()
                    .entry(v.clone())
                    .or_default() += 1;
            }
            println!(
                "{path} [{}] diff {:?} spec {:?} shin {} | {}",
                s.name,
                s.diffuse,
                s.specular,
                s.shininess,
                values.join(" ")
            );
        }
    }
    for (k, counts) in histogram {
        println!("== {k}: {counts:?}");
    }
    if std::env::var_os("LAYER_USAGE").is_some() {
        usage(&assets, &paths, &keys)?;
    }
    Ok(())
}

/// For layers whose first requested key is not `1`, the `UVTiling` the
/// item materials give them (`LAYER_USAGE=1`; scans every `.mtl` in
/// `objects/`).
fn usage(
    assets: &AssetSource,
    layers: &[String],
    keys: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let key = keys.first().ok_or("a key")?;
    let mut odd: BTreeMap<String, String> = BTreeMap::new();
    for path in layers {
        let Ok(mtl) = Material::read(assets, path) else {
            continue;
        };
        if let Some(v) = mtl
            .submaterials
            .first()
            .and_then(|s| s.params.iter().find(|(k, _)| k == key))
            && v.1 != "1"
        {
            let norm = path.to_lowercase().replace('\\', "/");
            odd.insert(norm.trim_start_matches("data/").to_string(), v.1.clone());
        }
    }
    let items: Vec<String> = assets
        .find(|name| {
            let p = name.to_lowercase().replace('\\', "/");
            p.contains("/objects/") && p.ends_with(".mtl")
        })
        .map(|e| e.name.to_string())
        .collect();
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for path in &items {
        let Ok(mtl) = Material::read(assets, path) else {
            continue;
        };
        for s in &mtl.submaterials {
            for l in &s.layers {
                let p = l.path.to_lowercase().replace('\\', "/");
                let p = if p.ends_with(".mtl") {
                    p
                } else {
                    format!("{p}.mtl")
                };
                if let Some(v) = odd.get(&p) {
                    *seen
                        .entry((format!("{p} ({key} {v})"), l.uv_tiling.to_string()))
                        .or_default() += 1;
                }
            }
        }
    }
    for ((layer, uv), n) in seen {
        println!("{layer}: UVTiling {uv} x{n}");
    }
    Ok(())
}
