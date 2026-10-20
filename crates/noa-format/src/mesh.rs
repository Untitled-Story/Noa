//! Independent NOMESH 2.0 geometry, preserving Unity coordinates and winding.
use crate::{generated::mesh_generated::noa::mesh as wire, ir::Vec3};
use serde::Serialize;
use std::{collections::BTreeSet, fmt};
pub const FORMAT_MAJOR: u16 = 2;
pub const FORMAT_MINOR: u16 = 0;
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Submesh {
    pub first_index: u32,
    pub index_count: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TexCoord {
    pub channel: u8,
    pub dimension: u8,
    pub values: Vec<[f32; 4]>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct MeshData {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub texcoords: Vec<TexCoord>,
    pub indices: Vec<u32>,
    pub submeshes: Vec<Submesh>,
}
#[derive(Debug)]
pub struct MeshError(pub String);
impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MeshError {}
impl MeshData {
    pub fn validate(&self) -> Result<(), MeshError> {
        let fail = |v: &str| Err(MeshError(v.into()));
        let n = self.positions.len();
        if n == 0 || n > u32::MAX as usize {
            return fail("mesh needs nonzero uint32 vertex count");
        }
        if self
            .positions
            .iter()
            .chain(&self.normals)
            .any(|v| ![v.x, v.y, v.z].iter().all(|v| v.is_finite()))
        {
            return fail("non-finite mesh");
        }
        if !self.normals.is_empty() && self.normals.len() != n {
            return fail("normal count differs");
        }
        for a in [&self.tangents, &self.colors] {
            if !a.is_empty() && a.len() != n {
                return fail("attribute count differs");
            }
            if a.iter().flatten().any(|v| !v.is_finite()) {
                return fail("non-finite mesh attribute");
            }
        }
        let mut channels = BTreeSet::new();
        for uv in &self.texcoords {
            if uv.channel > 7
                || !(1..=4).contains(&uv.dimension)
                || !channels.insert(uv.channel)
                || uv.values.len() != n
            {
                return fail("invalid texcoord channel/dimension/attribute count");
            }
            if uv.values.iter().any(|v| {
                v.iter().any(|x| !x.is_finite())
                    || v[uv.dimension as usize..].iter().any(|x| *x != 0.)
            }) {
                return fail("invalid texcoord values");
            }
        }
        if self.indices.is_empty()
            || !self.indices.len().is_multiple_of(3)
            || self.indices.len() > u32::MAX as usize
            || self.indices.iter().any(|i| *i as usize >= n)
        {
            return fail("invalid triangle index buffer");
        }
        let mut end = 0u32;
        for sub in &self.submeshes {
            if sub.first_index != end || sub.index_count == 0 || !sub.index_count.is_multiple_of(3)
            {
                return fail("invalid contiguous submesh");
            }
            end = end
                .checked_add(sub.index_count)
                .ok_or_else(|| MeshError("submesh overflow".into()))?;
        }
        if end as usize != self.indices.len() {
            return fail("submeshes must cover indices");
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, MeshError> {
        self.validate()?;
        let v3 = |v: &Vec3| wire::Vec3T {
            x: v.x,
            y: v.y,
            z: v.z,
        };
        let v4 = |v: &[f32; 4]| wire::Vec4T {
            x: v[0],
            y: v[1],
            z: v[2],
            w: v[3],
        };
        let d = wire::MeshT {
            format_major: FORMAT_MAJOR,
            format_minor: FORMAT_MINOR,
            name: Some(self.name.clone()),
            positions: self.positions.iter().map(v3).collect(),
            normals: (!self.normals.is_empty()).then(|| self.normals.iter().map(v3).collect()),
            tangents: (!self.tangents.is_empty()).then(|| self.tangents.iter().map(v4).collect()),
            colors: (!self.colors.is_empty()).then(|| self.colors.iter().map(v4).collect()),
            texcoords: Some(
                self.texcoords
                    .iter()
                    .map(|u| wire::TexCoordT {
                        channel: u.channel,
                        dimension: u.dimension,
                        values: u.values.iter().map(v4).collect(),
                    })
                    .collect(),
            ),
            indices: self.indices.clone(),
            submeshes: self
                .submeshes
                .iter()
                .map(|v| wire::SubmeshT {
                    first_index: v.first_index,
                    index_count: v.index_count,
                })
                .collect(),
        };
        let mut b = flatbuffers::FlatBufferBuilder::new();
        let root = d.pack(&mut b);
        wire::finish_mesh_buffer(&mut b, root);
        Ok(b.finished_data().to_vec())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, MeshError> {
        if bytes.len() < 8 || !wire::mesh_buffer_has_identifier(bytes) {
            return Err(MeshError("missing NOMS identifier".into()));
        }
        let r = wire::root_as_mesh(bytes).map_err(|e| MeshError(e.to_string()))?;
        if r.format_major() != FORMAT_MAJOR || r.format_minor() > FORMAT_MINOR {
            return Err(MeshError("unsupported mesh version".into()));
        }
        let d = r.unpack();
        let v3 = |v: wire::Vec3T| Vec3 {
            x: v.x,
            y: v.y,
            z: v.z,
        };
        let v4 = |v: wire::Vec4T| [v.x, v.y, v.z, v.w];
        let m = Self {
            name: d.name.unwrap_or_default(),
            positions: d.positions.into_iter().map(v3).collect(),
            normals: d.normals.unwrap_or_default().into_iter().map(v3).collect(),
            tangents: d.tangents.unwrap_or_default().into_iter().map(v4).collect(),
            colors: d.colors.unwrap_or_default().into_iter().map(v4).collect(),
            texcoords: d
                .texcoords
                .unwrap_or_default()
                .into_iter()
                .map(|u| TexCoord {
                    channel: u.channel,
                    dimension: u.dimension,
                    values: u.values.into_iter().map(v4).collect(),
                })
                .collect(),
            indices: d.indices,
            submeshes: d
                .submeshes
                .into_iter()
                .map(|v| Submesh {
                    first_index: v.first_index,
                    index_count: v.index_count,
                })
                .collect(),
        };
        m.validate()?;
        Ok(m)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn unchecked_buffer(version: u16, index: u32) -> Vec<u8> {
        let data = wire::MeshT {
            format_major: version,
            positions: vec![wire::Vec3T {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }],
            indices: vec![index; 3],
            submeshes: vec![wire::SubmeshT {
                first_index: 0,
                index_count: 3,
            }],
            ..Default::default()
        };
        let mut builder = flatbuffers::FlatBufferBuilder::new();
        let root = data.pack(&mut builder);
        wire::finish_mesh_buffer(&mut builder, root);
        builder.finished_data().to_vec()
    }

    #[test]
    fn reader_rejects_valid_flatbuffers_with_bad_version_or_index_semantics() {
        for version in [0, 1] {
            assert!(
                MeshData::decode(&unchecked_buffer(version, 0))
                    .unwrap_err()
                    .0
                    .contains("version")
            );
        }
        assert!(
            MeshData::decode(&unchecked_buffer(3, 0))
                .unwrap_err()
                .0
                .contains("version")
        );
        assert!(
            MeshData::decode(&unchecked_buffer(2, 1))
                .unwrap_err()
                .0
                .contains("index")
        );
    }
}
