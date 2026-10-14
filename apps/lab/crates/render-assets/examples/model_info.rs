//! Print a geometry file's material reference, whether it resolves, and its
//! node tree (name, parent, translation).
//!
//! ```bash
//! cargo run -p render-assets --release --example model_info -- <geometry path>...
//! ```

use render_assets::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets::open()?;
    for path in std::env::args().skip(1) {
        if path.to_ascii_lowercase().ends_with(".chr") {
            println!("{path}");
            for (i, b) in assets.skeleton(&path)?.iter().enumerate() {
                let p = b.position;
                println!(
                    "  bone {i:3} {:40} parent {:?} p=({:.3}, {:.3}, {:.3})",
                    b.name, b.parent, p[0], p[1], p[2]
                );
            }
            continue;
        }
        let m = assets.model(&path)?;
        println!("{path}");
        println!(
            "  material chunk {:?}, {} submeshes",
            m.info.material,
            m.info.submeshes.len()
        );
        if let Some(mtl) = &m.info.material {
            match assets.material(mtl) {
                Ok(i) => println!("  -> {} submaterials", i.submaterials.len()),
                Err(e) => println!("  -> ERR {e}"),
            }
        }
        for (i, n) in m.info.nodes.iter().enumerate() {
            let t = &n.transform;
            println!(
                "  node {i:3} {:40} parent {:?} t=({:.3}, {:.3}, {:.3}) r0=({:.2}, {:.2}, {:.2})",
                n.name, n.parent, t[3], t[7], t[11], t[0], t[1], t[2]
            );
        }
    }
    Ok(())
}
