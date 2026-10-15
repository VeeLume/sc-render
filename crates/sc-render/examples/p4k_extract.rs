//! Copy one file out of `Data.p4k` to disk.
//!
//! ```bash
//! cargo run -p render-assets --release --example p4k_extract -- <p4k path> <out file>
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("p4k path")?;
    let out = args.next().ok_or("out file")?;
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    let bytes = assets.read(&path)?;
    std::fs::write(&out, &bytes)?;
    println!("{path}: {} bytes -> {out}", bytes.len());
    Ok(())
}
