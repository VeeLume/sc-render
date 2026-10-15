//! Decode a mesh's vertex colours as LayerBlend decal coordinates (24 bits →
//! two 12-bit values, `x / 4095 × 4 − 0.5`) and report how many vertices fall
//! in the diffuse-decal (`2 < u ≤ 3`) and stencil (`2 < v ≤ 3`) ranges, per
//! submesh material.
//!
//! ```bash
//! cargo run -p sc-render --release --example decal_probe -- <geometry path>
//! ```

use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "objects/fps_weapons/weapons_v7/behr/rifle/p4ar/brfl_fps_behr_p4ar_parts.skin".into()
    });
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    let g = sc_extract::MeshGeometry::read(&assets, &path)?;
    let decode = |c: [u8; 4]| {
        let packed = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        let u = ((packed >> 12) & 4095) as f32 / 4095.0 * 4.0 - 0.5;
        let v = (packed & 4095) as f32 / 4095.0 * 4.0 - 0.5;
        (u, v)
    };
    let mut distinct: BTreeMap<[u8; 4], usize> = BTreeMap::new();
    for c in &g.colors {
        *distinct.entry(*c).or_default() += 1;
    }
    println!(
        "{} vertices, {} distinct colours; most common:",
        g.colors.len(),
        distinct.len()
    );
    let mut common: Vec<_> = distinct.iter().collect();
    common.sort_by(|a, b| b.1.cmp(a.1));
    for (c, n) in common.iter().take(6) {
        println!("  {c:?} x{n} -> {:?}", decode(**c));
    }
    // Sheet coordinates (u - 2, v) of vertices away from the parked corner.
    let mut lo = [f32::MAX; 2];
    let mut hi = [f32::MIN; 2];
    let mut placed = 0;
    for &c in &g.colors {
        let (u, v) = decode(c);
        let (su, sv) = (u - 2.0, v);
        if u > 2.0 && u <= 3.0 && (su < 0.99 || sv > 0.01) {
            placed += 1;
            lo = [lo[0].min(su), lo[1].min(sv)];
            hi = [hi[0].max(su), hi[1].max(sv)];
        }
    }
    println!(
        "{placed} vertices placed on the sheet: u {:.3}..{:.3}, v {:.3}..{:.3}",
        lo[0], hi[0], lo[1], hi[1]
    );
    for s in &g.submeshes {
        let tris = &g.indices[s.first_index as usize..(s.first_index + s.index_count) as usize];
        let (mut diffuse, mut stencil) = (0, 0);
        for &i in tris {
            let (u, v) = decode(g.colors[i as usize]);
            if u > 2.0 && u <= 3.0 && !(v > 2.0 && v <= 3.0) {
                diffuse += 1;
            }
            if v > 2.0 && v <= 3.0 {
                stencil += 1;
            }
        }
        println!(
            "  material {:2}: {:6} indices, diffuse-decal {:6}, stencil {:6}",
            s.material,
            tris.len(),
            diffuse,
            stencil
        );
    }
    Ok(())
}
