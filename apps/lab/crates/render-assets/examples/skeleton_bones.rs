//! List the bones a `.chr` or `.skin` carries (model space), optionally only
//! those whose name contains a filter.
//!
//! ```bash
//! cargo run -p render-assets --release --example skeleton_bones -- <path> [filter]
//! ```

use render_assets::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("path")?;
    let filter = args.next().unwrap_or_default().to_lowercase();
    let bones = Assets::open()?.skeleton(&path)?;
    println!("{path}: {} bones", bones.len());
    for b in bones
        .iter()
        .filter(|b| b.name.to_lowercase().contains(&filter))
    {
        println!("  {:40} parent {:?} pos {:?}", b.name, b.parent, b.position);
    }
    Ok(())
}
