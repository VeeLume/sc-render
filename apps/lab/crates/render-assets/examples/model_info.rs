//! Print a geometry file's material reference and whether it resolves.
//!
//! ```bash
//! cargo run -p render-assets --release --example model_info -- <geometry path>...
//! ```

use render_assets::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets::open()?;
    for path in std::env::args().skip(1) {
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
    }
    Ok(())
}
