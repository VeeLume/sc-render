//! Surface area per submaterial of geometry files, largest first, with the
//! submaterial names from the given `.mtl`.
//!
//! ```bash
//! cargo run -p sc-render --release --example material_area -- <mtl> <geometry>...
//! ```

use std::collections::BTreeMap;

use sc_render::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mtl = args.next().ok_or("mtl path")?;
    let names = Assets::open()?.material(&mtl)?.submaterials;
    let install = sc_discovery::discover_primary()?;
    let source = sc_extract::AssetSource::from_install(&install)?;
    let mut area: BTreeMap<u32, f64> = BTreeMap::new();
    for path in args {
        let g = sc_extract::MeshGeometry::read(&source, &path)?;
        for s in &g.submeshes {
            let tris = &g.indices[s.first_index as usize..(s.first_index + s.index_count) as usize];
            for t in tris.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| g.positions[i as usize]);
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let x = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                *area.entry(s.material as u32).or_default() +=
                    0.5 * ((x[0] * x[0] + x[1] * x[1] + x[2] * x[2]) as f64).sqrt();
            }
        }
    }
    let total: f64 = area.values().sum();
    let mut rows: Vec<_> = area.into_iter().collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (m, a) in rows {
        let name = names
            .get(m as usize)
            .map(|s| s.name.as_str())
            .unwrap_or("?");
        println!("{:5.1}%  [{m:2}] {name}", a / total * 100.0);
    }
    Ok(())
}
