//! Decode a cubemap DDS (default: the inventory reflection probe) and write
//! its six faces as one binary PPM strip (+X −X +Y −Y +Z −Z), tone-mapped by
//! `x / (1 + x)` when the format is HDR.
//!
//! ```bash
//! cargo run -p render-assets --release --example cube_probe -- [path] [out.ppm]
//! ```

use image_dds::{Surface, ddsfile::Dds, image};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        "Data/Textures/cubemaps/inventory_setup/cm_inventory_probe_cm.dds".into()
    });
    let out = args.next().unwrap_or_else(|| "cube_faces.ppm".into());
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    let t = sc_extract::Texture::read(&assets, &path)?;
    let mut dds = Dds::read(&t.dds[..])?;
    if std::env::var("CUBE_SIGNED").is_ok()
        && let Some(h) = dds.header10.as_mut()
    {
        h.dxgi_format = image_dds::ddsfile::DxgiFormat::BC6H_SF16;
    }
    println!(
        "header10 {:?}",
        dds.header10.as_ref().map(|h| h.dxgi_format)
    );
    let s = Surface::from_dds(&dds)?;
    println!(
        "{}x{} layers {} mips {} format {:?}",
        s.width, s.height, s.layers, s.mipmaps, s.image_format
    );
    let f = s.decode_rgbaf32()?;
    let n = s.width;
    let mut strip = image::RgbaImage::new(n * s.layers, n);
    let (mut lo, mut hi, mut sum) = (f32::MAX, f32::MIN, 0f64);
    for layer in 0..s.layers {
        let face = f.get(layer, 0, 0).ok_or("face")?;
        let brightest = face
            .as_chunks::<4>()
            .0
            .iter()
            .max_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])))
            .ok_or("empty")?;
        println!("face {layer} brightest {brightest:?}");
        for (i, px) in face.as_chunks::<4>().0.iter().enumerate() {
            let l = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
            lo = lo.min(l);
            hi = hi.max(l);
            sum += l as f64;
            let c = |v: f32| ((v / (1.0 + v)).powf(1.0 / 2.2) * 255.0) as u8;
            let (x, y) = (i as u32 % n, i as u32 / n);
            strip.put_pixel(
                layer * n + x,
                y,
                image::Rgba([c(px[0]), c(px[1]), c(px[2]), 255]),
            );
        }
    }
    println!(
        "luminance min {lo:.3} max {hi:.3} mean {:.3}",
        sum / (n * n * s.layers) as f64
    );
    let mut ppm = format!(
        "P6 {} {} 255
",
        strip.width(),
        strip.height()
    )
    .into_bytes();
    ppm.extend(strip.pixels().flat_map(|p| [p[0], p[1], p[2]]));
    std::fs::write(&out, ppm)?;
    println!("wrote {out}");
    Ok(())
}
