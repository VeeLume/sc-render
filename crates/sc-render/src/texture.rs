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
    /// The smaller mip levels, each half the one before, down to 1 x 1: the
    /// file's own mips, box-filtered below the smallest it stores. The
    /// stored mips matter: the engine's texture compiler lowers a `ddna`
    /// gloss where the normals average out, so finely tiled fabric turns
    /// matte at a distance (`mesh_vinyl_01`: mean gloss 0.70 at full size,
    /// 0.13 from its fourth mip on).
    pub mips: Vec<Mip>,
    /// The engine reads it as sRGB ([`sc_extract::Texture::is_srgb`]); only
    /// ever set for [`TextureKind::Color`].
    pub srgb: bool,
}

/// One mip level, RGBA8.
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub(crate) fn decode(t: &sc_extract::Texture, kind: TextureKind, max_size: u32) -> Result<Rgba> {
    let mut levels = decode_chain(&t.dds, max_size)?;
    match kind {
        TextureKind::Color => return Ok(rgba(levels, t.is_srgb())),
        TextureKind::Normal => {
            for px in levels
                .iter_mut()
                .flat_map(|m| m.data.as_chunks_mut::<4>().0)
            {
                let x = px[0] as f32 / 255.0 * 2.0 - 1.0;
                let y = px[1] as f32 / 255.0 * 2.0 - 1.0;
                let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                px[2] = ((z * 0.5 + 0.5) * 255.0).round() as u8;
                px[3] = 255;
            }
        }
        TextureKind::NormalGloss => {
            let alpha = match &t.alpha {
                Some(a) => Some(decode_chain(a, max_size)?).filter(|a| {
                    a.len() == levels.len()
                        && (a[0].width, a[0].height) == (levels[0].width, levels[0].height)
                }),
                None => None,
            };
            for (i, level) in levels.iter_mut().enumerate() {
                let gloss: Vec<u8> = match &alpha {
                    Some(a) => a[i].data.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
                    None => level.data.as_chunks::<4>().0.iter().map(|p| p[3]).collect(),
                };
                for (px, g) in level.data.as_chunks_mut::<4>().0.iter_mut().zip(gloss) {
                    px[2] = g;
                    px[3] = 255;
                }
            }
        }
        TextureKind::Gloss => {
            let from_alpha_file = t.alpha.is_some();
            if let Some(a) = &t.alpha {
                levels = decode_chain(a, max_size)?;
            }
            for px in levels
                .iter_mut()
                .flat_map(|m| m.data.as_chunks_mut::<4>().0)
            {
                let g = if from_alpha_file { px[0] } else { px[3] };
                *px = [g, g, g, 255];
            }
        }
    }
    Ok(rgba(levels, false))
}

fn rgba(levels: Vec<Mip>, srgb: bool) -> Rgba {
    let mut levels = levels.into_iter();
    let top = levels.next().expect("a decoded chain has a level");
    Rgba {
        width: top.width,
        height: top.height,
        data: top.data,
        mips: levels.collect(),
        srgb,
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

/// The mips from the largest that fits in `max_size` down, decoded, then
/// box-filtered on to 1 x 1.
fn decode_chain(bytes: &[u8], max_size: u32) -> Result<Vec<Mip>> {
    let dds = Dds::read(bytes).map_err(|e| Error::Decode(e.to_string()))?;
    let (w, h) = (dds.get_width(), dds.get_height());
    let mips = dds.get_num_mipmap_levels().max(1);
    let mut first = 0;
    while first + 1 < mips && (w >> first).max(h >> first) > max_size {
        first += 1;
    }
    let mut levels: Vec<Mip> = Vec::new();
    for mip in first..mips {
        let img = image_dds::image_from_dds(&dds, mip).map_err(|e| Error::Decode(e.to_string()))?;
        let next = Mip {
            width: img.width(),
            height: img.height(),
            data: img.into_raw(),
        };
        // A level that does not halve the one before ends the stored chain.
        if let Some(prev) = levels.last()
            && (next.width, next.height) != ((prev.width / 2).max(1), (prev.height / 2).max(1))
        {
            break;
        }
        levels.push(next);
    }
    while let Some(last) = levels.last()
        && (last.width, last.height) != (1, 1)
    {
        let half = halve(last);
        levels.push(half);
    }
    Ok(levels)
}

/// 2 x 2 box filter (edge texels reused for odd sizes).
fn halve(m: &Mip) -> Mip {
    let (w, h) = ((m.width / 2).max(1), (m.height / 2).max(1));
    let src = |x: u32, y: u32, c: usize| {
        let (x, y) = (x.min(m.width - 1), y.min(m.height - 1));
        u32::from(m.data[((y * m.width + x) * 4) as usize + c])
    };
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let sum = src(2 * x, 2 * y, c)
                    + src(2 * x + 1, 2 * y, c)
                    + src(2 * x, 2 * y + 1, c)
                    + src(2 * x + 1, 2 * y + 1, c);
                data.push(((sum + 2) / 4) as u8);
            }
        }
    }
    Mip {
        width: w,
        height: h,
        data,
    }
}
