//! List `Data.p4k` entries whose path contains every given substring
//! (case-insensitive), with their sizes.
//!
//! ```bash
//! cargo run -p sc-render --release --example p4k_find -- <substring>...
//! ```

use sc_extract::AssetSource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let needles: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
    let install = sc_discovery::discover_primary()?;
    let assets = AssetSource::from_install(&install)?;
    let hits = assets.find(move |name| {
        let p = name.to_lowercase();
        needles.iter().all(|n| p.contains(n))
    });
    for e in hits.take(200) {
        println!("{:>10}  {}", e.uncompressed_size, e.name);
    }
    Ok(())
}
