//! A geometry file packed into one little-endian buffer for GPU upload.
//!
//! Layout, each section present only when its `*_offset` is `Some`:
//! positions `f32 × 3`, normals `f32 × 3`, tangents `f32 × 4`, uvs `f32 × 2`,
//! colours `u8 × 4`, then indices `u32`. Every section starts 4-byte aligned.

use sc_extract::MeshGeometry;
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
    /// The `.mtl` the geometry names, without extension.
    pub material: Option<String>,
    pub vertex_count: u32,
    pub index_count: u32,
    pub positions_offset: u32,
    pub normals_offset: Option<u32>,
    pub tangents_offset: Option<u32>,
    pub uvs_offset: Option<u32>,
    pub colors_offset: Option<u32>,
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
            while buf.len() % 4 != 0 {
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
        let indices_offset = section(&mut buf, &mut |b| {
            g.indices.iter().for_each(|i| b.extend(i.to_le_bytes()))
        });
        let info = ModelInfo {
            path: path.to_string(),
            material: g.material.clone(),
            vertex_count: n as u32,
            index_count: g.indices.len() as u32,
            positions_offset,
            normals_offset,
            tangents_offset,
            uvs_offset,
            colors_offset,
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
