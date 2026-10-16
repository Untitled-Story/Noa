//! Independent, versioned `.nomesh` assets. Coordinates/UVs are not converted.

use crate::{
    generated::mesh_generated::noa::mesh as wire,
    ir::{Vec2, Vec3},
};
use serde::Serialize;
use std::fmt;

pub const FORMAT_MAJOR: u16 = 1;
pub const FORMAT_MINOR: u16 = 0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Submesh {
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeshData {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uv0: Vec<Vec2>,
    pub indices: Vec<u32>,
    pub submeshes: Vec<Submesh>,
}

#[derive(Debug)]
pub struct MeshError(pub String);

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for MeshError {}

impl MeshData {
    pub fn validate(&self) -> Result<(), MeshError> {
        let fail = |message: &str| Err(MeshError(message.into()));
        if self.positions.is_empty() || self.positions.len() > u32::MAX as usize {
            return fail("mesh must have a nonzero uint32 vertex count");
        }
        if self
            .positions
            .iter()
            .chain(&self.normals)
            .any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite())
            || self
                .uv0
                .iter()
                .any(|v| !v.x.is_finite() || !v.y.is_finite())
        {
            return fail("mesh contains non-finite vertex data");
        }
        if (!self.normals.is_empty() && self.normals.len() != self.positions.len())
            || (!self.uv0.is_empty() && self.uv0.len() != self.positions.len())
        {
            return fail("mesh attribute count differs from vertex count");
        }
        if self.indices.is_empty()
            || !self.indices.len().is_multiple_of(3)
            || self.indices.len() > u32::MAX as usize
        {
            return fail("mesh indices must be nonempty uint32 triangle triples");
        }
        if self
            .indices
            .iter()
            .any(|i| *i as usize >= self.positions.len())
        {
            return fail("mesh index exceeds vertex count");
        }
        let mut end = 0u32;
        for submesh in &self.submeshes {
            if submesh.first_index != end
                || submesh.index_count == 0
                || !submesh.index_count.is_multiple_of(3)
            {
                return fail("submeshes must be contiguous nonempty triangle ranges");
            }
            end = end
                .checked_add(submesh.index_count)
                .ok_or_else(|| MeshError("submesh range overflow".into()))?;
        }
        if end as usize != self.indices.len() {
            return fail("submeshes must cover the complete index buffer");
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, MeshError> {
        self.validate()?;
        let vec3 = |v: &Vec3| wire::Vec3T {
            x: v.x,
            y: v.y,
            z: v.z,
        };
        let data = wire::MeshT {
            format_major: FORMAT_MAJOR,
            format_minor: FORMAT_MINOR,
            name: Some(self.name.clone()),
            positions: self.positions.iter().map(vec3).collect(),
            normals: (!self.normals.is_empty()).then(|| self.normals.iter().map(vec3).collect()),
            uv0: (!self.uv0.is_empty()).then(|| {
                self.uv0
                    .iter()
                    .map(|v| wire::Vec2T { x: v.x, y: v.y })
                    .collect()
            }),
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
        let mut builder = flatbuffers::FlatBufferBuilder::new();
        let root = data.pack(&mut builder);
        wire::finish_mesh_buffer(&mut builder, root);
        Ok(builder.finished_data().to_vec())
    }

    /// Verify the identifier, FlatBuffer structure, version and mesh semantics.
    pub fn decode(bytes: &[u8]) -> Result<Self, MeshError> {
        // The generated identifier helper expects at least eight bytes.
        if bytes.len() < 8 || !wire::mesh_buffer_has_identifier(bytes) {
            return Err(MeshError("missing NOMS mesh identifier".into()));
        }
        let root = wire::root_as_mesh(bytes)
            .map_err(|e| MeshError(format!("invalid mesh FlatBuffer: {e}")))?;
        if root.format_major() != FORMAT_MAJOR || root.format_minor() > FORMAT_MINOR {
            return Err(MeshError(format!(
                "unsupported mesh version {}.{}",
                root.format_major(),
                root.format_minor()
            )));
        }
        let data = root.unpack();
        let vec3 = |v: wire::Vec3T| Vec3 {
            x: v.x,
            y: v.y,
            z: v.z,
        };
        let mesh = Self {
            name: data.name.unwrap_or_default(),
            positions: data.positions.into_iter().map(vec3).collect(),
            normals: data
                .normals
                .unwrap_or_default()
                .into_iter()
                .map(vec3)
                .collect(),
            uv0: data
                .uv0
                .unwrap_or_default()
                .into_iter()
                .map(|v| Vec2 { x: v.x, y: v.y })
                .collect(),
            indices: data.indices,
            submeshes: data
                .submeshes
                .into_iter()
                .map(|v| Submesh {
                    first_index: v.first_index,
                    index_count: v.index_count,
                })
                .collect(),
        };
        mesh.validate()?;
        Ok(mesh)
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
        assert!(
            MeshData::decode(&unchecked_buffer(2, 0))
                .unwrap_err()
                .0
                .contains("version")
        );
        assert!(
            MeshData::decode(&unchecked_buffer(1, 1))
                .unwrap_err()
                .0
                .contains("index")
        );
    }
}
