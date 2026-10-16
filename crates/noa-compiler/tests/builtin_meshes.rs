use noa_compiler::{
    InspectOptions,
    assets::{builtin_mesh, plan_builtin_meshes},
    convert::{EntityIds, SourceIdentity},
    prepare_ir,
    unity::{Guid, UnityFile},
};
use noa_format::{
    ir::{Vec2, Vec3},
    mesh::MeshData,
};
use std::{fs, path::Path};

fn quad() -> MeshData {
    builtin_mesh(&"0000000000000000e000000000000000".parse().unwrap(), 10210).unwrap()
}

#[test]
fn default_quad_preserves_dimensions_front_face_and_uvs_and_roundtrips() {
    let mesh = quad();
    assert_eq!(mesh.positions.len(), 4);
    assert!(
        mesh.positions
            .iter()
            .all(|v| v.x.abs() == 0.5 && v.y.abs() == 0.5 && v.z == 0.0)
    );
    assert!(mesh.normals.iter().all(|v| *v
        == Vec3 {
            x: 0.0,
            y: 0.0,
            z: -1.0
        }));
    for (p, uv) in mesh.positions.iter().zip(&mesh.uv0) {
        assert_eq!(
            *uv,
            Vec2 {
                x: p.x + 0.5,
                y: p.y + 0.5
            }
        );
    }
    // Each triangle faces -Z and the two triangles cover unit area.
    let mut area = 0.0;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.positions[i as usize]);
        let cross_z = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(cross_z < 0.0);
        area += -cross_z * 0.5;
    }
    assert_eq!(area, 1.0);
    let bytes = mesh.encode().unwrap();
    assert_eq!(&bytes[4..8], b"NOMS");
    assert_eq!(MeshData::decode(&bytes).unwrap(), mesh);
    assert_eq!(bytes, quad().encode().unwrap());
}

#[test]
fn geometry_validation_rejects_bad_indices_attributes_and_submesh_ranges() {
    let mut bad = quad();
    bad.indices[0] = 4;
    assert!(bad.encode().unwrap_err().to_string().contains("index"));
    let mut bad = quad();
    bad.uv0.pop();
    assert!(
        bad.encode()
            .unwrap_err()
            .to_string()
            .contains("attribute count")
    );
    let mut bad = quad();
    bad.positions[0].x = f32::NAN;
    assert!(bad.encode().unwrap_err().to_string().contains("non-finite"));
    let mut bad = quad();
    bad.submeshes[0].first_index = 3;
    assert!(bad.encode().unwrap_err().to_string().contains("contiguous"));
    let mut bad = quad();
    bad.submeshes.clear();
    assert!(bad.encode().unwrap_err().to_string().contains("cover"));
}

#[test]
fn checked_reader_rejects_truncated_corrupted_and_wrong_kind_buffers() {
    let bytes = quad().encode().unwrap();
    // Up to three final alignment bytes are not referenced FlatBuffer data.
    for end in 0..bytes.len() - 3 {
        assert!(MeshData::decode(&bytes[..end]).is_err(), "length {end}");
    }
    let mut wrong = bytes.clone();
    wrong[4..8].copy_from_slice(b"NOIR");
    assert!(MeshData::decode(&wrong).is_err());
    let mut wrong = bytes;
    wrong[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(MeshData::decode(&wrong).is_err());
}

#[test]
fn builtin_lookup_requires_both_guid_and_file_id() {
    for (guid, id) in [
        ("0000000000000000e000000000000000", 12345),
        ("0000000000000000f000000000000000", 10210),
        ("1234567890abcdef1234567890abcdef", 10210),
    ] {
        assert_eq!(
            builtin_mesh(&guid.parse().unwrap(), id).unwrap_err().code,
            "UNSUPPORTED_BUILTIN_MESH"
        );
    }
}

fn renderers(second_mode: i32, second_id: i64) -> UnityFile {
    UnityFile::parse(&format!("--- !u!199 &1\nParticleSystemRenderer:\n  m_RenderMode: 4\n  m_Mesh: {{fileID: 10210, guid: 0000000000000000e000000000000000, type: 0}}\n--- !u!199 &2\nParticleSystemRenderer:\n  m_RenderMode: {second_mode}\n  m_Mesh: {{fileID: {second_id}, guid: 0000000000000000e000000000000000, type: 0}}\n")).unwrap()
}

#[test]
fn active_mesh_consumers_deduplicate_geometry_and_billboards_ignore_idle_meshes() {
    let plan = plan_builtin_meshes("fixture", &renderers(4, 10210)).unwrap();
    let ids = EntityIds::allocate(
        [
            SourceIdentity::new("fixture", 1),
            SourceIdentity::new("fixture", 2),
        ]
        .into_iter()
        .chain(plan.id_sources()),
    )
    .unwrap();
    let (meshes, bindings) = plan.lower(&ids).unwrap();
    assert_eq!(meshes.len(), 1);
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].mesh_asset_id, bindings[1].mesh_asset_id);
    assert_ne!(bindings[0].component_id, bindings[1].component_id);
    assert_eq!(
        plan_builtin_meshes("fixture", &renderers(0, 12345))
            .unwrap()
            .id_sources()
            .count(),
        1
    );
    let error = plan_builtin_meshes("fixture", &renderers(4, 12345)).unwrap_err();
    assert_eq!(error.code, "UNSUPPORTED_BUILTIN_MESH");
    assert!(
        error
            .message
            .contains("fileID=2 ParticleSystemRenderer.m_Mesh")
    );
}

#[test]
fn real_sample_mesh_consumers_share_one_self_contained_asset() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/scenario/effect/kirakira_01_rip/kirakira_01.prefab");
    let draft = prepare_ir(&path, &InspectOptions::default()).unwrap();
    assert_eq!(draft.meshes.len(), 1);
    assert_eq!(draft.mesh_bindings.len(), 2);
    assert_eq!(draft.pending_components.len(), 32);
    let mesh = &draft.meshes[0];
    let guid: Guid = mesh.source.guid.as_ref().unwrap().parse().unwrap();
    assert_eq!(
        mesh.geometry,
        builtin_mesh(&guid, mesh.source.file_id).unwrap()
    );
    assert!(
        draft
            .mesh_bindings
            .iter()
            .all(|b| b.mesh_asset_id == mesh.id)
    );
    let file = UnityFile::parse(&fs::read_to_string(path).unwrap()).unwrap();
    for binding in &draft.mesh_bindings {
        assert_eq!(binding.slot, 0);
    }
    assert_eq!(
        file.objects
            .values()
            .filter(|o| o.type_name == "ParticleSystemRenderer"
                && o.data["m_RenderMode"].as_i64() == Some(4))
            .count(),
        2
    );
}
