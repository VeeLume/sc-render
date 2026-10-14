//! Decode every texture the stage-1 items use and print size and channel
//! ranges, to check each DDS format decodes.
//!
//! ```bash
//! cargo run -p render-assets --release --example decode_probe
//! ```

use render_assets::{Assets, TextureKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets::open()?;
    for (model, mtl) in [
        (
            "objects/spaceships/power_plants/small_component/aegs/powr_aegs_s01_pl03.cga",
            None,
        ),
        (
            "objects/buildingsets/human/universal/prop/dressing/can/can_drink_3_energy_a.cga",
            Some("objects/buildingsets/human/universal/prop/dressing/can/can_drink_3_a.mtl"),
        ),
    ] {
        let m = assets.model(model)?;
        println!("== {model}: {} bytes", m.buffer.len());
        let mtl = match mtl {
            Some(p) => p.to_string(),
            None => m.info.material.clone().ok_or("no material")?,
        };
        let mtl = assets.material(&mtl)?;
        let mut files: Vec<(String, TextureKind)> = Vec::new();
        for s in &mtl.submaterials {
            for t in &s.textures {
                let kind = if t.file.contains("_ddn") {
                    TextureKind::Normal
                } else {
                    TextureKind::Color
                };
                files.push((t.file.clone(), kind));
                if t.file.contains("_ddna") {
                    files.push((t.file.clone(), TextureKind::Gloss));
                }
            }
            for l in &s.layers {
                let layer = assets.material(&l.path)?;
                for t in layer.submaterials.iter().flat_map(|s| &s.textures) {
                    files.push((t.file.clone(), TextureKind::Color));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        for (file, kind) in files {
            match assets.texture(&file, kind, 512) {
                Ok(t) => {
                    let mut lo = [255u8; 4];
                    let mut hi = [0u8; 4];
                    for px in t.data.chunks_exact(4) {
                        for c in 0..4 {
                            lo[c] = lo[c].min(px[c]);
                            hi[c] = hi[c].max(px[c]);
                        }
                    }
                    println!(
                        "  {:?} {:48} {}x{} lo {lo:?} hi {hi:?}",
                        kind,
                        file.rsplit('/').next().unwrap_or(&file),
                        t.width,
                        t.height
                    );
                }
                Err(e) => println!("  {kind:?} {file}: ERR {e}"),
            }
        }
    }
    Ok(())
}
