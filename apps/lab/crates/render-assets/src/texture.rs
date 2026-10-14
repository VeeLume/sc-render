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
    /// A `ddna` map packed into one texture: normal X, Y in R, G (encoded
    /// `n × 0.5 + 0.5`; Z is rebuilt by the shader), gloss in B.
    NormalGloss,
}

/// Decoded pixels, row-major, top row first.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// The engine reads it as sRGB ([`sc_extract::Texture::is_srgb`]); only
    /// ever set for [`TextureKind::Color`].
    pub srgb: bool,
}

pub(crate) fn decode(t: &sc_extract::Texture, kind: TextureKind, max_size: u32) -> Result<Rgba> {
    match kind {
        TextureKind::Color => Ok(Rgba {
            srgb: t.is_srgb(),
            ..decode_dds(&t.dds, max_size)?
        }),
        TextureKind::Normal => {
            let mut img = decode_dds(&t.dds, max_size)?;
            for px in img.data.as_chunks_mut::<4>().0 {
                let x = px[0] as f32 / 255.0 * 2.0 - 1.0;
                let y = px[1] as f32 / 255.0 * 2.0 - 1.0;
                let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                px[2] = ((z * 0.5 + 0.5) * 255.0).round() as u8;
                px[3] = 255;
            }
            Ok(img)
        }
        TextureKind::NormalGloss => {
            let mut img = decode_dds(&t.dds, max_size)?;
            let alpha = match &t.alpha {
                Some(a) => Some(decode_dds(a, max_size)?)
                    .filter(|a| a.width == img.width && a.height == img.height),
                None => None,
            };
            let gloss: Vec<u8> = match &alpha {
                Some(a) => a.data.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
                None => img.data.as_chunks::<4>().0.iter().map(|p| p[3]).collect(),
            };
            for (px, g) in img.data.as_chunks_mut::<4>().0.iter_mut().zip(gloss) {
                px[2] = g;
                px[3] = 255;
            }
            Ok(img)
        }
        TextureKind::Gloss => {
            let (mut img, from_alpha_file) = match &t.alpha {
                Some(a) => (decode_dds(a, max_size)?, true),
                None => (decode_dds(&t.dds, max_size)?, false),
            };
            for px in img.data.as_chunks_mut::<4>().0 {
                let g = if from_alpha_file { px[0] } else { px[3] };
                *px = [g, g, g, 255];
            }
            Ok(img)
        }
    }
}

/// A decoded volume texture: `depth` slices of `width × height` RGBA8,
/// row-major, slice after slice.
pub struct Volume {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub data: Vec<u8>,
}

/// An uncompressed RGB(A) volume DDS (masked channels, top mip only); what
/// the engine's small lookup tables such as the layer-blend hue LUT are.
pub(crate) fn decode_volume(bytes: &[u8]) -> Result<Volume> {
    let dds = Dds::read(bytes).map_err(|e| Error::Decode(e.to_string()))?;
    let pf = &dds.header.spf;
    let (Some(bits), Some(r), Some(g), Some(b)) = (
        pf.rgb_bit_count,
        pf.r_bit_mask,
        pf.g_bit_mask,
        pf.b_bit_mask,
    ) else {
        return Err(Error::Decode("volume is not uncompressed RGB".into()));
    };
    if bits % 8 != 0 || !(8..=32).contains(&bits) {
        return Err(Error::Decode(format!("{bits}-bit volume texels")));
    }
    let a = pf.a_bit_mask.unwrap_or(0);
    let (width, height, depth) = (dds.get_width(), dds.get_height(), dds.get_depth().max(1));
    let stride = (bits / 8) as usize;
    let texels = (width * height * depth) as usize;
    let src = dds
        .data
        .get(..texels * stride)
        .ok_or_else(|| Error::Decode("volume data truncated".into()))?;
    let channel = |texel: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        ((texel & mask) >> mask.trailing_zeros()) as u8
    };
    let mut data = Vec::with_capacity(texels * 4);
    for px in src.chunks_exact(stride) {
        let texel = px
            .iter()
            .rev()
            .fold(0u32, |acc, &byte| (acc << 8) | u32::from(byte));
        data.extend([
            channel(texel, r),
            channel(texel, g),
            channel(texel, b),
            channel(texel, a),
        ]);
    }
    Ok(Volume {
        width,
        height,
        depth,
        data,
    })
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
        srgb: false,
    })
}
