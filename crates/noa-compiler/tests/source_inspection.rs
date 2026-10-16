use noa_compiler::{InspectOptions, inspect, resolver::AssetIndex, unity::UnityFile};
use std::{fs, path::Path};
use tempfile::TempDir;

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn asset(root: &Path, name: &str, guid: &str, text: &str) {
    write(root, name, text);
    write(
        root,
        &format!("{name}.meta"),
        &format!("fileFormatVersion: 2\nguid: {guid}\n"),
    );
}

#[test]
fn unity_document_identity_and_negative_ids_are_preserved() {
    let file = UnityFile::parse("\u{feff}%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &-42\nGameObject:\n  m_Name: example\n--- !u!4 &99 stripped\nTransform:\n  m_GameObject: {fileID: -42}\n").unwrap();
    assert_eq!(file.objects[&-42].class_id, 1);
    assert!(file.objects[&99].stripped);
    assert_eq!(file.references().unwrap()[0].target_file_id, -42);
}

#[test]
fn duplicate_object_ids_and_invalid_documents_fail() {
    assert_eq!(
        UnityFile::parse("--- !u!1 &1\nGameObject: {}\n--- !u!4 &1\nTransform: {}\n")
            .unwrap_err()
            .code,
        "DUPLICATE_FILE_ID"
    );
    assert_eq!(
        UnityFile::parse("--- !u!1 &0\nGameObject: {}\n")
            .unwrap_err()
            .code,
        "INVALID_FILE_ID"
    );
    assert!(UnityFile::parse("GameObject: {}\n").is_err());
    assert!(UnityFile::parse("--- !u!1 &1\nGameObject: {}\nTransform: {}\n").is_err());
}

#[test]
fn pptr_detection_excludes_other_file_id_fields_and_null_refs() {
    let file = UnityFile::parse("--- !u!1 &1\nGameObject:\n  local: {fileID: 1}\n  null: {fileID: 0}\n  unrelated: {fileID: 123, count: 2}\n").unwrap();
    let references = file.references().unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].property, "GameObject.local");
}

#[test]
fn dependencies_are_recursive_but_unreferenced_assets_are_not_loaded() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    asset(
        root,
        "main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  dep: {fileID: 21, guid: 22222222222222222222222222222222, type: 2}\n",
    );
    asset(
        root,
        "sub/m.mat",
        "22222222222222222222222222222222",
        "--- !u!21 &21\nMaterial:\n  image: {fileID: 2800000, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 3}\n",
    );
    asset(
        root,
        "sub/image.png",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "opaque image content",
    );
    asset(
        root,
        "unused.anim",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "intentionally invalid YAML, must not parse",
    );
    let report = inspect(&root.join("main.prefab"), &InspectOptions::default()).unwrap();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.indexed_asset_count, 4);
    assert_eq!(report.assets.len(), 3);
}

#[test]
fn missing_reference_has_precise_source_and_can_be_fixed_by_extra_search_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    asset(
        root,
        "effect/main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  parts:\n  - material: {fileID: 21, guid: abcdefabcdefabcdefabcdefabcdefab, type: 2}\n",
    );
    asset(
        root,
        "shared/m.mat",
        "abcdefabcdefabcdefabcdefabcdefab",
        "--- !u!21 &21\nMaterial: {}\n",
    );
    let prefab = root.join("effect/main.prefab");
    let missing = inspect(&prefab, &InspectOptions::default()).unwrap();
    assert!(missing.has_errors());
    assert_eq!(missing.diagnostics[0].code, "MISSING_ASSET");
    assert_eq!(missing.diagnostics[0].file_id, Some(1));
    assert_eq!(
        missing.diagnostics[0].property.as_deref(),
        Some("GameObject.parts[0].material")
    );
    let resolved = inspect(
        &prefab,
        &InspectOptions {
            search_roots: vec![root.join("shared")],
        },
    )
    .unwrap();
    assert!(!resolved.has_errors());
}

#[test]
fn external_yaml_object_must_exist_not_just_the_guid() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    asset(
        root,
        "main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  dep: {fileID: 999, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 2}\n",
    );
    asset(
        root,
        "m.mat",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--- !u!21 &21\nMaterial: {}\n",
    );
    let report = inspect(&root.join("main.prefab"), &InspectOptions::default()).unwrap();
    assert!(report.has_errors());
    assert_eq!(report.diagnostics[0].code, "MISSING_EXTERNAL_OBJECT");
}

#[test]
fn duplicate_guid_is_rejected_and_overlapping_search_roots_are_deduplicated() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let guid = "abcdefabcdefabcdefabcdefabcdefab";
    asset(root, "main.prefab", guid, "--- !u!1 &1\nGameObject: {}\n");
    assert_eq!(
        AssetIndex::discover(&root.join("main.prefab"), &[root.to_owned()])
            .unwrap()
            .assets
            .len(),
        1
    );
    asset(root, "other.mat", guid, "--- !u!21 &21\nMaterial: {}\n");
    assert_eq!(
        AssetIndex::discover(&root.join("main.prefab"), &[])
            .unwrap_err()
            .code,
        "DUPLICATE_GUID"
    );
}

#[test]
fn unity_project_root_finds_sibling_assets() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("ProjectSettings")).unwrap();
    asset(
        root,
        "Assets/effects/main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  dep: {fileID: 21, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 2}\n",
    );
    asset(
        root,
        "Assets/materials/m.mat",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--- !u!21 &21\nMaterial: {}\n",
    );
    let report = inspect(
        &root.join("Assets/effects/main.prefab"),
        &InspectOptions::default(),
    )
    .unwrap();
    assert!(!report.has_errors());
    assert_eq!(report.entry, "root0/effects/main.prefab");
    assert_eq!(report.assets.len(), 2);
}

#[test]
fn source_cycles_terminate_and_builtin_resources_are_explicit() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    asset(
        root,
        "main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  self: {fileID: 1}\n  dep: {fileID: 21, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 2}\n  mesh: {fileID: 10210, guid: 0000000000000000e000000000000000, type: 0}\n",
    );
    asset(
        root,
        "m.mat",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--- !u!21 &21\nMaterial:\n  back: {fileID: 1, guid: 11111111111111111111111111111111, type: 2}\n",
    );
    let report = inspect(&root.join("main.prefab"), &InspectOptions::default()).unwrap();
    assert!(!report.has_errors());
    assert_eq!(report.assets.len(), 2);
    assert_eq!(report.diagnostics[0].code, "BUILTIN_RESOURCE");
}

#[test]
fn local_missing_object_is_an_error() {
    let temp = TempDir::new().unwrap();
    asset(
        temp.path(),
        "main.prefab",
        "11111111111111111111111111111111",
        "--- !u!1 &1\nGameObject:\n  missing: {fileID: 42}\n",
    );
    let report = inspect(&temp.path().join("main.prefab"), &InspectOptions::default()).unwrap();
    assert_eq!(report.diagnostics[0].code, "MISSING_OBJECT");
}

#[test]
fn index_does_not_follow_directory_symlinks() {
    #[cfg(unix)]
    {
        let temp = TempDir::new().unwrap();
        asset(
            temp.path(),
            "main.prefab",
            "11111111111111111111111111111111",
            "--- !u!1 &1\nGameObject: {}\n",
        );
        std::os::unix::fs::symlink(temp.path(), temp.path().join("loop")).unwrap();
        let index = AssetIndex::discover(&temp.path().join("main.prefab"), &[]).unwrap();
        assert_eq!(index.assets.len(), 1);
    }
}

#[test]
fn guid_lexemes_are_not_converted_to_numbers_or_exponents() {
    use noa_compiler::unity::{ReferenceKind, parse_meta_guid};
    let meta = parse_meta_guid("guid: 00000000000000000000000000000001\n").unwrap();
    assert_eq!(meta.to_string(), "00000000000000000000000000000001");
    let file = UnityFile::parse("--- !u!1 &1\nGameObject:\n  title: 'guid: 11111111111111111111111111111111'\n  refs:\n  - {fileID: 1, guid: 11111111111111111111111111111111, type: 2}\n  - {fileID: 10210, guid: 0000000000000000e000000000000000, type: 0}\n").unwrap();
    assert_eq!(
        file.objects[&1]
            .data
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "guid: 11111111111111111111111111111111"
    );
    let refs = file.references().unwrap();
    assert!(
        matches!(&refs[0].kind, ReferenceKind::External { guid } if guid.to_string()=="11111111111111111111111111111111")
    );
    assert!(
        matches!(&refs[1].kind, ReferenceKind::Builtin { guid } if guid.to_string()=="0000000000000000e000000000000000")
    );
}

fn scene_fixture(parent_active: i32, cyclic: bool) -> String {
    let root_parent = if cyclic { 4 } else { 0 };
    let child_children = if cyclic { "[{fileID: 2}]" } else { "[]" };
    format!(
        "--- !u!1 &1\nGameObject:\n  m_Name: root\n  m_IsActive: {parent_active}\n  m_Component: [{{component: {{fileID: 2}}}}]\n--- !u!4 &2\nTransform:\n  m_GameObject: {{fileID: 1}}\n  m_Father: {{fileID: {root_parent}}}\n  m_Children: [{{fileID: 4}}]\n--- !u!1 &3\nGameObject:\n  m_Name: child\n  m_IsActive: 1\n  m_Component: [{{component: {{fileID: 4}}}}]\n--- !u!4 &4\nTransform:\n  m_GameObject: {{fileID: 3}}\n  m_Father: {{fileID: 2}}\n  m_Children: {child_children}\n"
    )
}

#[test]
fn scene_hierarchy_inherits_inactive_parent_without_changing_active_self() {
    use noa_compiler::unity::scene_nodes;
    let file = UnityFile::parse(&scene_fixture(0, false)).unwrap();
    let nodes = scene_nodes(&file).unwrap();
    let child = nodes.iter().find(|n| n.name == "child").unwrap();
    assert_eq!(child.path, "child");
    assert_eq!(child.parent_file_id, Some(1));
    assert_eq!(child.root_file_id, 1);
    assert!(child.active_self);
    assert!(!child.active_in_hierarchy);
}

#[test]
fn scene_parent_cycles_and_inconsistent_child_edges_fail() {
    use noa_compiler::unity::scene_nodes;
    let cycle = UnityFile::parse(&scene_fixture(1, true)).unwrap();
    assert_eq!(scene_nodes(&cycle).unwrap_err().code, "HIERARCHY_CYCLE");
    let inconsistent =
        scene_fixture(1, false).replace("m_Children: [{fileID: 4}]", "m_Children: []");
    let file = UnityFile::parse(&inconsistent).unwrap();
    assert_eq!(scene_nodes(&file).unwrap_err().code, "INVALID_HIERARCHY");
}

#[test]
fn duplicate_yaml_fields_are_rejected() {
    assert!(
        UnityFile::parse("--- !u!1 &1\nGameObject:\n  m_Name: first\n  m_Name: second\n").is_err()
    );
}

#[test]
fn kirakira_source_graph_resolves_from_just_the_prefab() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenario/effect/kirakira_01_rip");
    let prefab = root.join("kirakira_01.prefab");
    let report = inspect(&prefab, &InspectOptions::default()).unwrap();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.assets.len(), 9);
    assert_eq!(report.particles.len(), 16);
    assert_eq!(report.scenes[0].nodes.len(), 17);
    assert!(
        report
            .assets
            .iter()
            .any(|a| a.path.ends_with("shaders/particle add + alpha blend.asset"))
    );
    assert!(report.assets.iter().any(|a| a.path.ends_with("Stop.anim")));
    assert_eq!(
        report
            .particles
            .iter()
            .filter(|p| p.looped == Some(true))
            .count(),
        12
    );
    // The inactive-vs-animation distinction must not be guessed by inspection.
    assert!(report.scenes[0].nodes.iter().all(|n| n.active_in_hierarchy));
    let repeated = inspect(&prefab, &InspectOptions::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        serde_json::to_string(&repeated).unwrap()
    );
}

#[test]
fn script_user_fields_with_m_prefix_are_not_treated_as_engine_fields() {
    let temp = TempDir::new().unwrap();
    asset(
        temp.path(),
        "main.prefab",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--- !u!114 &114\nMonoBehaviour:\n  m_Name: ''\n  m_Enabled: 1\n  m_Script: {fileID: 0}\n  m_speed: 2.5\n  targets: []\n",
    );
    let report = inspect(&temp.path().join("main.prefab"), &InspectOptions::default()).unwrap();
    assert_eq!(report.scripts[0].fields["m_speed"], 2.5);
    assert_eq!(report.scripts[0].fields["targets"], serde_json::json!([]));
    assert!(!report.scripts[0].fields.contains_key("m_Enabled"));
}
