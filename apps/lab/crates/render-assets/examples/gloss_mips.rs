//! Mean gloss per mip level of `ddna` textures, from the texture's own mip
//! chain (the separate `.a` gloss texture, else the main alpha).
//!
//! ```bash
//! cargo run -p render-assets --release --example gloss_mips -- <ddna path>...
//! ```

use image_dds::ddsfile::Dds;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    for path in std::env::args().skip(1) {
        let t = sc_extract::Texture::read(&assets, &path)?;
        let (bytes, from_alpha_file) = match &t.alpha {
            Some(a) => (a.as_slice(), true),
            None => (t.dds.as_slice(), false),
        };
        let dds = Dds::read(bytes)?;
        let mips = dds.get_num_mipmap_levels().max(1);
        let means: Vec<String> = (0..mips)
            .map(|m| {
                let img = image_dds::image_from_dds(&dds, m).expect("decode");
                let px = img.as_raw().as_chunks::<4>().0;
                let sum: f64 = px
                    .iter()
                    .map(|p| f64::from(if from_alpha_file { p[0] } else { p[3] }))
                    .sum();
                format!("{:.2}", sum / px.len() as f64 / 255.0)
            })
            .collect();
        println!("{path}: {}", means.join(" "));
    }
    Ok(())
}
