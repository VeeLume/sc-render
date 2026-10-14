//! Print a `.mtl` as the renderer sees it, plus its LayerBlend layers'
//! `TintMode` / `ShadingModel` and the layer material's own surface.
//!
//! ```bash
//! cargo run -p render-assets --release --example mtl_dump -- <mtl path>
//! ```

use render_assets::Assets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("mtl path")?;
    let assets = Assets::open()?;
    for s in assets.material(&path)?.submaterials {
        println!(
            "[{}] {} {:?} diff {:?} spec {:?} shin {}",
            s.name, s.shader, s.features, s.diffuse, s.specular, s.shininess
        );
        for t in &s.textures {
            println!("    {} {}", t.slot, t.file);
        }
        for l in &s.layers {
            let layer = assets.material(&l.path).ok();
            let sub = layer.as_ref().and_then(|m| {
                m.submaterials
                    .iter()
                    .find(|x| x.name == l.submaterial)
                    .or(m.submaterials.first())
            });
            let param = |k: &str| {
                sub.and_then(|x| x.params.iter().find(|(key, _)| key == k))
                    .map_or("-", |(_, v)| v.as_str())
            };
            println!(
                "    {} {} tint {:?} gloss_mult {} uv {} palette {} | TintMode {} ShadingModel {} | diff {:?} spec {:?} shin {:?}",
                l.name,
                l.path,
                l.tint,
                l.gloss_mult,
                l.uv_tiling,
                l.palette_tint,
                param("TintMode"),
                param("ShadingModel"),
                sub.map(|x| x.diffuse),
                sub.map(|x| x.specular),
                sub.map(|x| x.shininess),
            );
        }
    }
    Ok(())
}
