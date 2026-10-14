//! DDS → RGBA8, shaped by what the texture is used for.

use image_dds::ddsfile::Dds;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// What a texture is sampled as; decides how its channels are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum TextureKind {
    /// As stored (diffuse, masks, decals).
    Color,
    /// Tangent-space normal map: RG as stored (BC5 is signed), B rebuilt
    /// as `sqrt(1 - x² - y²)`; all three encoded `n × 0.5 + 0.5`.
    Normal,
    /// The gloss of a `ddna` map: the separate `.a` texture's red channel,
    /// else the main texture's alpha; delivered in R, G and B.
    Gloss,
}

/// Decoded pixels, row-major, top row first.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub(crate) fn decode(t: &sc_extract::Texture, kind: TextureKind, max_size: u32) -> Result<Rgba> {
    match kind {
        TextureKind::Color => decode_dds(&t.dds, max_size),
        TextureKind::Normal => {
            let mut img = decode_dds(&t.dds, max_size)?;
            for px in img.data.chunks_exact_mut(4) {
                let x = px[0] as f32 / 255.0 * 2.0 - 1.0;
                let y = px[1] as f32 / 255.0 * 2.0 - 1.0;
                let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                px[2] = ((z * 0.5 + 0.5) * 255.0).round() as u8;
                px[3] = 255;
            }
            Ok(img)
        }
        TextureKind::Gloss => {
            let (mut img, from_alpha_file) = match &t.alpha {
                Some(a) => (decode_dds(a, max_size)?, true),
                None => (decode_dds(&t.dds, max_size)?, false),
            };
            for px in img.data.chunks_exact_mut(4) {
                let g = if from_alpha_file { px[0] } else { px[3] };
                px.copy_from_slice(&[g, g, g, 255]);
            }
            Ok(img)
        }
    }
}

/// The largest mip that fits in `max_size`, decoded.
fn decode_dds(bytes: &[u8], max_size: u32) -> Result<Rgba> {
    let dds = Dds::read(bytes).map_err(|e| Error::Decode(e.to_string()))?;
    let (w, h) = (dds.get_width(), dds.get_height());
    let mips = dds.get_num_mipmap_levels().max(1);
    let mut mip = 0;
    while mip + 1 < mips && (w >> mip).max(h >> mip) > max_size {
        mip += 1;
    }
    let img = image_dds::image_from_dds(&dds, mip).map_err(|e| Error::Decode(e.to_string()))?;
    Ok(Rgba {
        width: img.width(),
        height: img.height(),
        data: img.into_raw(),
    })
}
