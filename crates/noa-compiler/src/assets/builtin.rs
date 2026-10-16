use crate::{
    convert::{EntityIds, SourceIdentity},
    diagnostics::Error,
    unity::{Guid, ReferenceKind, UnityFile},
};
use noa_format::{
    ir::{EntityId, SourceReference, Vec2, Vec3},
    mesh::{MeshData, Submesh},
};
use serde::Serialize;
use std::collections::BTreeSet;

const DEFAULT_RESOURCES: &str = "0000000000000000e000000000000000";

/// Explicit engine identity, never inferred from an object's or asset's name.
/// New default-resource converters are added here as their semantics are verified.
pub fn builtin_mesh(guid: &Guid, file_id: i64) -> Result<MeshData, Error> {
    match (guid.to_string().as_str(), file_id) {
        (DEFAULT_RESOURCES, 10210) => Ok(quad()),
        _ => Err(Error::new(
            "UNSUPPORTED_BUILTIN_MESH",
            format!("no Mesh converter for Unity builtin {guid}/{file_id}"),
        )),
    }
}

fn source(guid: &Guid, file_id: i64) -> SourceIdentity {
    SourceIdentity::new(format!("builtin/{guid}"), file_id)
}

#[derive(Debug, Serialize)]
pub struct MeshAsset {
    pub id: EntityId,
    pub source: SourceReference,
    pub geometry: MeshData,
}

/// A resolved resource binding; the Renderer itself may still be pending.
#[derive(Debug, Serialize)]
pub struct MeshBinding {
    pub component_id: EntityId,
    pub slot: u8,
    pub mesh_asset_id: EntityId,
}

#[derive(Debug)]
struct MeshUse {
    component_id: i64,
    slot: u8,
    guid: Guid,
    file_id: i64,
}

#[derive(Debug)]
pub struct BuiltinMeshPlan {
    owner_asset: String,
    keys: BTreeSet<(Guid, i64)>,
    uses: Vec<MeshUse>,
}

impl BuiltinMeshPlan {
    pub fn id_sources(&self) -> impl Iterator<Item = SourceIdentity> + '_ {
        self.keys
            .iter()
            .map(|(guid, file_id)| source(guid, *file_id))
    }

    pub fn lower(&self, ids: &EntityIds) -> Result<(Vec<MeshAsset>, Vec<MeshBinding>), Error> {
        let meshes = self
            .keys
            .iter()
            .map(|(guid, file_id)| {
                Ok(MeshAsset {
                    id: ids.resolve(&source(guid, *file_id))?,
                    source: SourceReference {
                        guid: Some(guid.to_string()),
                        file_id: *file_id,
                    },
                    geometry: builtin_mesh(guid, *file_id)?,
                })
            })
            .collect::<Result<_, Error>>()?;
        let bindings = self
            .uses
            .iter()
            .map(|usage| {
                Ok(MeshBinding {
                    component_id: ids.get(&self.owner_asset, usage.component_id)?,
                    slot: usage.slot,
                    mesh_asset_id: ids.resolve(&source(&usage.guid, usage.file_id))?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok((meshes, bindings))
    }
}

/// Collect builtin geometry actually used by Mesh-mode particle Renderers.
/// Billboard Renderers often retain inactive Mesh fields; these are not assets.
pub fn plan_builtin_meshes(asset: &str, file: &UnityFile) -> Result<BuiltinMeshPlan, Error> {
    let mut plan = BuiltinMeshPlan {
        owner_asset: asset.into(),
        keys: BTreeSet::new(),
        uses: Vec::new(),
    };
    for reference in file.references()? {
        let object = &file.objects[&reference.source_file_id];
        if object.class_id != 199
            || object.type_name != "ParticleSystemRenderer"
            || object.data["m_RenderMode"].as_i64() != Some(4)
        {
            continue;
        }
        let slot = match reference.property.as_str() {
            "ParticleSystemRenderer.m_Mesh" => 0,
            "ParticleSystemRenderer.m_Mesh1" => 1,
            "ParticleSystemRenderer.m_Mesh2" => 2,
            "ParticleSystemRenderer.m_Mesh3" => 3,
            _ => continue,
        };
        if let ReferenceKind::Builtin { guid } = reference.kind {
            builtin_mesh(&guid, reference.target_file_id).map_err(|e| {
                Error::new(
                    e.code,
                    format!(
                        "{asset} fileID={} {}: {e}",
                        object.file_id, reference.property
                    ),
                )
            })?;
            plan.keys.insert((guid.clone(), reference.target_file_id));
            plan.uses.push(MeshUse {
                component_id: object.file_id,
                slot,
                guid,
                file_id: reference.target_file_id,
            });
        }
    }
    Ok(plan)
}

/// Semantic reconstruction of the engine Quad, not original serialized bytes.
/// Identity evidence and geometry conventions: docs/协议说明.md.
fn quad() -> MeshData {
    MeshData {
        name: "Quad".into(),
        positions: vec![
            Vec3 {
                x: -0.5,
                y: -0.5,
                z: 0.0,
            },
            Vec3 {
                x: 0.5,
                y: -0.5,
                z: 0.0,
            },
            Vec3 {
                x: -0.5,
                y: 0.5,
                z: 0.0,
            },
            Vec3 {
                x: 0.5,
                y: 0.5,
                z: 0.0,
            },
        ],
        normals: vec![
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: -1.0
            };
            4
        ],
        uv0: vec![
            Vec2 { x: 0.0, y: 0.0 },
            Vec2 { x: 1.0, y: 0.0 },
            Vec2 { x: 0.0, y: 1.0 },
            Vec2 { x: 1.0, y: 1.0 },
        ],
        indices: vec![0, 2, 1, 2, 3, 1],
        submeshes: vec![Submesh {
            first_index: 0,
            index_count: 6,
        }],
    }
}
