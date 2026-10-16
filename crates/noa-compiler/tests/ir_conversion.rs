use noa_compiler::{
    convert::{EntityIds, SourceIdentity, convert_custom_components, convert_scene},
    resolver::{MetadataCatalog, ScriptReference, ScriptResolution, ScriptResolver},
    unity::UnityFile,
};
use noa_format::ir::{ReferenceKind, SerializedValue, TypedReference};
use std::{fs, path::Path};

const SCENE: &str = "--- !u!1 &1
GameObject:
  m_Name: root
  m_IsActive: 0
  m_Layer: 31
  m_Component: [{component: {fileID: 2}}, {component: {fileID: 3}}]
--- !u!4 &2
Transform:
  m_GameObject: {fileID: 1}
  m_Father: {fileID: 0}
  m_Children: []
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalScale: {x: -1, y: 2, z: 0}
--- !u!114 &3
MonoBehaviour:
  m_GameObject: {fileID: 1}
  m_Enabled: 0
  m_Script: {fileID: 11500000, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 3}
  m_speed: 2.5
  mode: 1
  large: 18446744073709551615
  nested:
    label: hi
    children: [{fileID: 1}, {fileID: 99}, {fileID: 0}]
";

fn ids(file: &UnityFile) -> EntityIds {
    EntityIds::allocate(
        file.objects
            .keys()
            .map(|id| SourceIdentity::new("source", *id)),
    )
    .unwrap()
}

fn metadata() -> MetadataCatalog {
    let mut catalog = MetadataCatalog::default();
    catalog.add_json(r#"{"format":"unity-monoscript-metadata","version":1,"scripts":[{"guid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","file_id":"11500000","assembly":"Test","namespace":"Demo","class_name":"Mover"}]}"#).unwrap();
    catalog
}

#[test]
fn ids_are_deterministic_and_source_scoped() {
    let sources = [
        SourceIdentity::new("b", -8),
        SourceIdentity::new("a", -8),
        SourceIdentity::new("a", i64::MAX),
    ];
    let forward = EntityIds::allocate(sources.clone()).unwrap();
    let reverse = EntityIds::allocate(sources.into_iter().rev()).unwrap();
    for (asset, file_id) in [("a", -8), ("b", -8), ("a", i64::MAX)] {
        assert_eq!(
            forward.get(asset, file_id).unwrap(),
            reverse.get(asset, file_id).unwrap()
        );
    }
    assert_ne!(forward.get("a", -8).unwrap(), forward.get("b", -8).unwrap());
    assert!(EntityIds::allocate([SourceIdentity::new("a", 0)]).is_err());
    assert!(
        EntityIds::allocate([SourceIdentity::new("a", 1), SourceIdentity::new("a", 1)]).is_err()
    );
}

#[test]
fn scene_preserves_3d_transform_layer_and_pending_components() {
    let file = UnityFile::parse(SCENE).unwrap();
    let ir = convert_scene("source", &file, &ids(&file)).unwrap();
    let node = &ir.nodes[0];
    assert_eq!(node.transform.position.z, 3.0);
    assert_eq!(node.transform.scale.x, -1.0);
    assert_eq!(node.transform.scale.z, 0.0);
    assert_eq!(node.layer_mask, 0x8000_0000);
    assert!(!node.active);
    assert_eq!(node.parent_id, None);
    assert_eq!(ir.pending_components.len(), 1);
    assert_eq!(ir.pending_components[0].type_name, "MonoBehaviour");
}

#[test]
fn scene_rejects_bad_numeric_data_and_does_not_fill_missing_transform() {
    for (from, to) in [
        ("x: 1, y: 2, z: 3", "x: .nan, y: 2, z: 3"),
        ("x: 1, y: 2, z: 3", "x: 1e99, y: 2, z: 3"),
        ("z: 0, w: 1", "z: 0, w: 0"),
        ("m_Layer: 31", "m_Layer: 32"),
        ("m_LocalScale:", "missing:"),
    ] {
        let file = UnityFile::parse(&SCENE.replace(from, to)).unwrap();
        assert!(
            convert_scene("source", &file, &ids(&file)).is_err(),
            "{from} -> {to}"
        );
    }
}

#[test]
fn custom_fields_preserve_numbers_unknown_types_and_reference_status() {
    let file = UnityFile::parse(SCENE).unwrap();
    let ids = ids(&file);
    let root_id = ids.get("source", 1).unwrap();
    let fragment = convert_custom_components("source", &file, &ids, &metadata(), &|reference| {
        (reference.file_id == 1).then_some(TypedReference {
            kind: ReferenceKind::Node,
            id: root_id,
        })
    })
    .unwrap();
    let component = &fragment.components[0];
    assert!(!component.enabled);
    assert_eq!(component.script.as_ref().unwrap().namespace, "Demo");
    assert_eq!(
        component.fields["m_speed"].value,
        SerializedValue::Float(2.5)
    );
    assert_eq!(component.fields["mode"].value, SerializedValue::Int(1));
    assert_eq!(
        component.fields["large"].value,
        SerializedValue::UInt(u64::MAX)
    );
    assert!(component.fields["mode"].declared_type.is_none());
    assert!(!component.fields.contains_key("m_Enabled"));
    let SerializedValue::Object(nested) = &component.fields["nested"].value else {
        panic!("nested object");
    };
    let SerializedValue::Array(children) = &nested["children"].value else {
        panic!("array");
    };
    let SerializedValue::Reference(found) = &children[0] else {
        panic!("resolved ref");
    };
    assert_eq!(found.target.unwrap().id, root_id);
    let SerializedValue::Reference(missing) = &children[1] else {
        panic!("unresolved ref");
    };
    assert!(missing.target.is_none());
    assert_eq!(missing.source.file_id, 99);
    assert_eq!(children[2], SerializedValue::Null);
    assert_eq!(fragment.diagnostics.len(), 1);
    assert_eq!(
        fragment.diagnostics[0].property.as_deref(),
        Some("MonoBehaviour.nested.children[1]")
    );
}

#[test]
fn missing_script_metadata_preserves_original_reference_and_fields() {
    let file = UnityFile::parse(SCENE).unwrap();
    let fragment = convert_custom_components(
        "source",
        &file,
        &ids(&file),
        &MetadataCatalog::default(),
        &|_| None,
    )
    .unwrap();
    assert!(fragment.components[0].script.is_none());
    assert_eq!(fragment.components[0].source_script.file_id, 11500000);
    assert_eq!(
        fragment.components[0].source_script.guid.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert!(fragment.components[0].fields.contains_key("nested"));
    assert_eq!(fragment.diagnostics[0].code, "SCRIPT_METADATA_MISSING");
}

#[test]
fn duplicate_field_id_spelling_is_not_always_a_reference() {
    let text = SCENE.replace("m_speed: 2.5", "m_speed: {fileID: 71, description: custom}");
    let file = UnityFile::parse(&text).unwrap();
    let fragment =
        convert_custom_components("source", &file, &ids(&file), &metadata(), &|_| None).unwrap();
    assert!(matches!(
        fragment.components[0].fields["m_speed"].value,
        SerializedValue::Object(_)
    ));
}

#[test]
fn metadata_keys_include_full_int64_file_id_and_conflicts_never_choose_a_winner() {
    let mut catalog = metadata();
    let original = ScriptReference {
        guid: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap()),
        file_id: 11500000,
    };
    assert!(matches!(
        catalog.resolve("asset", &original),
        ScriptResolution::Resolved(_)
    ));
    let text = r#"{"format":"unity-monoscript-metadata","version":1,"scripts":[{"guid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","file_id":"9223372036854775807","assembly":"Other","namespace":"","class_name":"Mover"}]}"#;
    catalog.add_json(text).unwrap();
    let huge = ScriptReference {
        file_id: i64::MAX,
        ..original.clone()
    };
    assert!(matches!(
        catalog.resolve("asset", &huge),
        ScriptResolution::Resolved(_)
    ));
    catalog
        .add_json(&text.replace("9223372036854775807", "11500000"))
        .unwrap();
    assert!(
        matches!(catalog.resolve("asset", &original), ScriptResolution::Conflict(v) if v.len() == 2)
    );
}

#[test]
fn metadata_does_not_accept_lossy_json_numbers_or_unknown_versions() {
    for (from, to) in [
        ("\"11500000\"", "11500000"),
        ("\"11500000\"", "\"9223372036854775808\""),
        ("\"version\":1", "\"version\":2"),
    ] {
        let text = r#"{"format":"unity-monoscript-metadata","version":1,"scripts":[{"guid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","file_id":"11500000","assembly":"Test","namespace":"Demo","class_name":"Mover"}]}"#;
        assert!(
            MetadataCatalog::default()
                .add_json(&text.replace(from, to))
                .is_err()
        );
    }
}

#[test]
fn kirakira_scene_and_scripts_use_authoritative_sidecar_data() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenario/effect/kirakira_01_rip");
    let file =
        UnityFile::parse(&fs::read_to_string(root.join("kirakira_01.prefab")).unwrap()).unwrap();
    let ids = ids(&file);
    let scene = convert_scene("source", &file, &ids).unwrap();
    assert_eq!(scene.nodes.len(), 17);
    assert_eq!(scene.layouts.len(), 2);
    assert_eq!(scene.host_layout_nodes.len(), 1);
    assert_eq!(scene.pending_components.len(), 49);
    let catalog = MetadataCatalog::discover(&[root]).unwrap();
    let scripts = convert_custom_components("source", &file, &ids, &catalog, &|_| None).unwrap();
    assert_eq!(scripts.components.len(), 16);
    assert!(
        scripts
            .components
            .iter()
            .all(|c| c.script.as_ref().unwrap().assembly == "Assembly-CSharp")
    );
    let effector = scripts
        .components
        .iter()
        .find(|c| c.script.as_ref().unwrap().class_name == "ScenarioEffector")
        .unwrap();
    assert_eq!(effector.script.as_ref().unwrap().namespace, "Sekai");
    assert_eq!(scripts.diagnostics.len(), 1);
    assert_eq!(
        scripts.diagnostics[0].property.as_deref(),
        Some("MonoBehaviour.mainAnimator")
    );
}
