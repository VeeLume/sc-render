//! List the bones a `.chr` or `.skin` carries (model space), optionally only
//! those whose name contains a filter; for a `.skin`, also check its skinning
//! weights (coverage, highest joint against the bone count, weight sums).
//!
//! ```bash
//! cargo run -p sc-render --release --example skeleton_bones -- <path> [filter]
//! ```

use sc_render::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("path")?;
    let filter = args.next().unwrap_or_default().to_lowercase();
    let assets = Assets::open()?;
    let bones = assets.skeleton(&path)?;
    println!("{path}: {} bones", bones.len());
    for b in bones
        .iter()
        .filter(|b| b.name.to_lowercase().contains(&filter))
    {
        println!("  {:40} parent {:?} pos {:?}", b.name, b.parent, b.position);
    }
    if path.to_lowercase().ends_with(".skin") {
        let g = sc_extract::MeshGeometry::read(assets.source(), &path)?;
        let max_joint = g
            .bone_weights
            .iter()
            .flat_map(|w| {
                w.joints
                    .iter()
                    .zip(w.weights)
                    .filter(|(_, w)| *w > 0)
                    .map(|(j, _)| *j)
            })
            .max();
        let sums: Vec<u32> = g
            .bone_weights
            .iter()
            .map(|w| w.weights.iter().map(|&x| u32::from(x)).sum())
            .collect();
        println!(
            "weights {} for {} vertices; highest joint {:?} of {} bones; weight sums {:?}..{:?}",
            g.bone_weights.len(),
            g.positions.len(),
            max_joint,
            bones.len(),
            sums.iter().min(),
            sums.iter().max()
        );
    }
    Ok(())
}
