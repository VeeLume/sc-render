//! A geometry file packed into one little-endian buffer for GPU upload.
//!
//! Layout, each section present only when its `*_offset` is `Some`:
//! positions `f32 × 3`, normals `f32 × 3`, tangents `f32 × 4`, uvs `f32 × 2`,
//! colours `u8 × 4`, skin joints `u16 × 4` and weights `u8 × 4` (normalised),
//! then indices `u32`. Every section starts 4-byte aligned.

use sc_extract::{BoneWeights, MeshGeometry};
use serde::Serialize;

/// The packed buffer and what is in it.
pub struct Model {
    pub info: ModelInfo,
    pub buffer: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ModelInfo {
    pub path: String,
    /// The `.mtl` the geometry names (bare names resolved next to it).
    pub material: Option<String>,
    pub vertex_count: u32,
    pub index_count: u32,
    pub positions_offset: u32,
    pub normals_offset: Option<u32>,
    pub tangents_offset: Option<u32>,
    pub uvs_offset: Option<u32>,
    pub colors_offset: Option<u32>,
    /// Skinning: per vertex the four strongest of up to eight influences,
    /// indices into the `.skin`'s own bone list.
    pub joints_offset: Option<u32>,
    /// The matching weights, renormalised to sum to 255.
    pub weights_offset: Option<u32>,
    pub indices_offset: u32,
    pub byte_length: u32,
    pub nodes: Vec<NodeInfo>,
    pub submeshes: Vec<SubmeshInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NodeInfo {
    pub name: String,
    pub parent: Option<u32>,
    /// 3 × 4 row-major, relative to the parent.
    pub transform: [f32; 12],
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SubmeshInfo {
    pub material: u32,
    pub node: Option<u32>,
    pub first_index: u32,
    pub index_count: u32,
}

impl Model {
    pub(crate) fn pack(path: &str, g: &MeshGeometry) -> Self {
        let n = g.positions.len();
        let mut buf: Vec<u8> = Vec::new();
        let section = |buf: &mut Vec<u8>, bytes: &mut dyn FnMut(&mut Vec<u8>)| -> u32 {
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
            let at = buf.len() as u32;
            bytes(buf);
            at
        };
        let positions_offset = section(&mut buf, &mut |b| {
            g.positions
                .iter()
                .flatten()
                .for_each(|f| b.extend(f.to_le_bytes()))
        });
        let normals_offset = (g.normals.len() == n).then(|| {
            section(&mut buf, &mut |b| {
                g.normals
                    .iter()
                    .flatten()
                    .for_each(|f| b.extend(f.to_le_bytes()))
            })
        });
        let tangents_offset = (g.tangents.len() == n).then(|| {
            section(&mut buf, &mut |b| {
                g.tangents
                    .iter()
                    .flatten()
                    .for_each(|f| b.extend(f.to_le_bytes()))
            })
        });
        let uvs_offset = (g.uvs.len() == n).then(|| {
            section(&mut buf, &mut |b| {
                g.uvs
                    .iter()
                    .flatten()
                    .for_each(|f| b.extend(f.to_le_bytes()))
            })
        });
        let colors_offset = (g.colors.len() == n)
            .then(|| section(&mut buf, &mut |b| b.extend(g.colors.iter().flatten())));
        let skin: Option<Vec<([u16; 4], [u8; 4])>> = (g.bone_weights.len() == n)
            .then(|| g.bone_weights.iter().map(strongest_four).collect());
        let joints_offset = skin.as_ref().map(|skin| {
            section(&mut buf, &mut |b| {
                skin.iter()
                    .flat_map(|(j, _)| j)
                    .for_each(|j| b.extend(j.to_le_bytes()))
            })
        });
        let weights_offset = skin.as_ref().map(|skin| {
            section(&mut buf, &mut |b| {
                b.extend(skin.iter().flat_map(|(_, w)| w))
            })
        });
        let indices_offset = section(&mut buf, &mut |b| {
            g.indices.iter().for_each(|i| b.extend(i.to_le_bytes()))
        });
        let info = ModelInfo {
            path: path.to_string(),
            material: g.material_path(path),
            vertex_count: n as u32,
            index_count: g.indices.len() as u32,
            positions_offset,
            normals_offset,
            tangents_offset,
            uvs_offset,
            colors_offset,
            joints_offset,
            weights_offset,
            indices_offset,
            byte_length: buf.len() as u32,
            nodes: g
                .nodes
                .iter()
                .map(|node| NodeInfo {
                    name: node.name.clone(),
                    parent: node.parent.map(|p| p as u32),
                    transform: std::array::from_fn(|i| node.transform[i / 4][i % 4]),
                })
                .collect(),
            submeshes: g
                .submeshes
                .iter()
                .map(|s| SubmeshInfo {
                    material: s.material as u32,
                    node: s.node.map(|n| n as u32),
                    first_index: s.first_index,
                    index_count: s.index_count,
                })
                .collect(),
        };
        Self { info, buffer: buf }
    }
}

/// A GPU vertex takes four influences: keep the strongest four of up to
/// eight and scale their weights back to 255.
fn strongest_four(w: &BoneWeights) -> ([u16; 4], [u8; 4]) {
    let mut slots: Vec<(u16, u8)> = w.joints.iter().copied().zip(w.weights).collect();
    slots.sort_by_key(|s| std::cmp::Reverse(s.1));
    let kept = &slots[..4];
    let total: u32 = kept.iter().map(|&(_, x)| u32::from(x)).sum();
    let mut joints = [0u16; 4];
    let mut weights = [0u8; 4];
    if total == 0 {
        return (joints, weights);
    }
    let mut given = 0u32;
    for (i, &(j, x)) in kept.iter().enumerate() {
        joints[i] = j;
        let scaled = if i == 3 {
            255 - given
        } else {
            (u32::from(x) * 255 + total / 2) / total
        };
        let scaled = scaled.min(255 - given);
        weights[i] = scaled as u8;
        given += scaled;
    }
    (joints, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_strongest_four_summing_to_255() {
        let w = BoneWeights {
            joints: [1, 2, 3, 4, 5, 6, 7, 8],
            weights: [10, 200, 0, 20, 5, 15, 3, 2],
        };
        let (j, x) = strongest_four(&w);
        assert_eq!(j, [2, 4, 6, 1]);
        assert_eq!(x.iter().map(|&v| u32::from(v)).sum::<u32>(), 255);
        assert!(x[0] > x[1] && x[1] >= x[2]);
    }

    #[test]
    fn unweighted_stays_empty() {
        assert_eq!(strongest_four(&BoneWeights::default()), ([0; 4], [0; 4]));
    }
}
