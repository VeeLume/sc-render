//! Print a CryXmlB / XML file from `Data.p4k` (a `.cdf`, `.mtl`, …) as text.
//!
//! ```bash
//! cargo run -p render-assets --release --example p4k_xml -- <path>...
//! ```

use sc_extract::{AssetSource, XmlNode, object_container};

fn print(n: &XmlNode, depth: usize) {
    let attrs: String = n
        .attrs
        .iter()
        .map(|(k, v)| format!(" {k}=\"{v}\""))
        .collect();
    println!("{}<{}{attrs}>", "  ".repeat(depth), n.tag);
    for c in &n.children {
        print(c, depth + 1);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let install = sc_discovery::discover_primary()?;
    let assets = AssetSource::from_install(&install)?;
    for path in std::env::args().skip(1) {
        println!("== {path}");
        if let Some(root) = object_container::decode(&assets.read(&path)?)? {
            print(&root, 0);
        }
    }
    Ok(())
}
