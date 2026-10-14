//! Decode textures and write each as a binary PPM: the RGB image followed to
//! the right by its R, G, B and A channels as greyscale.
//!
//! ```bash
//! cargo run -p render-assets --release --example tex_dump -- <out dir> <texture path>...
//! ```

use render_assets::{Assets, TextureKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().ok_or("out dir")?);
    let assets = Assets::open()?;
    for path in args {
        let t = assets.texture(&path, TextureKind::Color, 256)?;
        let (w, h) = (t.width as usize, t.height as usize);
        let mut px = Vec::with_capacity(w * 5 * h * 3);
        for y in 0..h {
            let row = &t.data[y * w * 4..(y + 1) * w * 4];
            px.extend(
                row.as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2]]),
            );
            for c in 0..4 {
                px.extend(row.as_chunks::<4>().0.iter().flat_map(|p| [p[c]; 3]));
            }
        }
        let name = path.rsplit(['/', '\\']).next().unwrap_or("tex");
        let out = dir.join(format!("{name}.ppm"));
        let mut file = format!("P6 {} {h} 255\n", w * 5).into_bytes();
        file.extend(px);
        std::fs::write(&out, file)?;
        println!("{} {w}x{h} -> {}", path, out.display());
    }
    Ok(())
}
