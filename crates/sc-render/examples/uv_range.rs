//! Print a geometry file's UV range (layer tiling multiplies the UVs) and how
//! well its decoded vertex normals agree with the triangles' geometric
//! normals (mean dot product; near 1 for a correct decode).
//!
//! ```bash
//! cargo run -p sc-render --release --example uv_range -- <geometry path>...
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let install = sc_discovery::discover_primary()?;
    let assets = sc_extract::AssetSource::from_install(&install)?;
    for path in std::env::args().skip(1) {
        let g = sc_extract::MeshGeometry::read(&assets, &path)?;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for uv in &g.uvs {
            lo = [lo[0].min(uv[0]), lo[1].min(uv[1])];
            hi = [hi[0].max(uv[0]), hi[1].max(uv[1])];
        }
        let mut acc = vec![[0f32; 3]; g.positions.len()];
        for t in g.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| g.positions[i as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            for &i in t {
                for k in 0..3 {
                    acc[i as usize][k] += n[k];
                }
            }
        }
        let (mut sum, mut neg, mut count) = (0f64, 0usize, 0usize);
        for (n, a) in g.normals.iter().zip(&acc) {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            if l < 1e-12 {
                continue;
            }
            let d = (n[0] * a[0] + n[1] * a[1] + n[2] * a[2]) / l;
            sum += d as f64;
            neg += usize::from(d < 0.0);
            count += 1;
        }
        println!(
            "{path}: {} uvs, u {:.3}..{:.3}, v {:.3}..{:.3}; normals {} mean dot {:.3}, {} opposed",
            g.uvs.len(),
            lo[0],
            hi[0],
            lo[1],
            hi[1],
            g.normals.len(),
            sum / count.max(1) as f64,
            neg
        );
    }
    Ok(())
}
