//! Print a texture's DDS format, size, mip count and whether a separate gloss
//! (`.a`) texture exists.
//!
//! ```bash
//! cargo run -p render-assets --release --example tex_info -- <texture path>...
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    for path in std::env::args().skip(1) {
        match sc_extract::Texture::read(&assets, &path) {
            Ok(t) => println!(
                "{path}: {:?} {}x{} mips {} alpha {} srgb {}",
                t.format(),
                t.width(),
                t.height(),
                t.mip_count(),
                t.alpha.is_some(),
                t.is_srgb()
            ),
            Err(e) => println!("{path}: {e}"),
        }
    }
    Ok(())
}
