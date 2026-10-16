use noa_compiler::{
    InspectOptions, inspect, inspect_with_script_resolver, prepare_ir,
    prepare_ir_with_script_resolver,
    resolver::{ScriptReference, ScriptResolution, ScriptResolver},
};
use noa_format::ir::ScriptIdentity;
use std::{fs, path::Path};
use tempfile::TempDir;

const SCRIPT: &str = "--- !u!1 &1\nGameObject:\n  m_Name: Owner\n  m_IsActive: 1\n  m_Layer: 0\n  m_Component:\n  - component: {fileID: 2}\n  - component: {fileID: 7}\n--- !u!4 &2\nTransform:\n  m_GameObject: {fileID: 1}\n  m_Father: {fileID: 0}\n  m_Children: []\n  m_LocalPosition: {x: 0, y: 0, z: 0}\n  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}\n  m_LocalScale: {x: 1, y: 1, z: 1}\n--- !u!114 &7\nMonoBehaviour:\n  m_GameObject: {fileID: 1}\n  m_Enabled: 1\n  m_Script: {fileID: 11500000, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 3}\n  mode: 9\n";
const META: &str = r#"{"format":"unity-monoscript-metadata","version":1,"scripts":[{"guid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","file_id":"11500000","assembly":"Actual","namespace":"From.Metadata","class_name":"Mover"}]}"#;

fn entry(root: &Path) -> std::path::PathBuf {
    let path = root.join("main.prefab");
    fs::write(&path, SCRIPT).unwrap();
    path
}

#[test]
fn metadata_can_supply_script_identity_without_any_csharp_file() {
    let root = TempDir::new().unwrap();
    let path = entry(root.path());
    fs::write(root.path().join("unity.monoscripts.json"), META).unwrap();
    let report = inspect(&path, &InspectOptions::default()).unwrap();
    assert!(!report.has_errors());
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        report.scripts[0].script_identity.as_ref().unwrap().assembly,
        "Actual"
    );
    assert!(report.scripts[0].script_asset.is_none());
    assert_eq!(report.scripts[0].script_reference.file_id, 11500000);
    assert_eq!(report.scripts[0].enabled, Some(true));
}

#[test]
fn missing_metadata_does_not_guess_from_csharp_or_path() {
    let root = TempDir::new().unwrap();
    let path = entry(root.path());
    let directory = root.path().join("scripts/Assembly-Guessed");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("Mover.cs"),
        "namespace Guessed; class Mover {}",
    )
    .unwrap();
    fs::write(
        directory.join("Mover.cs.meta"),
        "guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    let report = inspect(&path, &InspectOptions::default()).unwrap();
    assert!(report.scripts[0].script_identity.is_none());
    assert!(report.scripts[0].script_reference.guid.is_some());
    assert_eq!(report.diagnostics[0].code, "SCRIPT_METADATA_MISSING");
}

#[test]
fn conflicting_metadata_retains_raw_reference_and_reports_an_error() {
    let root = TempDir::new().unwrap();
    let path = entry(root.path());
    fs::write(root.path().join("first.monoscripts.json"), META).unwrap();
    fs::write(
        root.path().join("second.monoscripts.json"),
        META.replace("Actual", "Conflicting"),
    )
    .unwrap();
    let report = inspect(&path, &InspectOptions::default()).unwrap();
    assert!(report.has_errors());
    assert!(report.scripts[0].script_identity.is_none());
    assert_eq!(report.scripts[0].script_reference.file_id, 11500000);
    assert_eq!(report.scripts[0].fields["mode"], 9);
    assert_eq!(report.diagnostics[0].code, "SCRIPT_METADATA_CONFLICT");
    let draft = prepare_ir(&path, &InspectOptions::default()).unwrap();
    assert!(draft.custom_components[0].script.is_none());
    assert_eq!(draft.custom_components[0].source_script.file_id, 11500000);
    assert_eq!(draft.diagnostics[0].code, "SCRIPT_METADATA_CONFLICT");
}

#[test]
fn additional_search_scope_discovers_sidecars_and_identical_duplicates_are_safe() {
    let root = TempDir::new().unwrap();
    let extra = TempDir::new().unwrap();
    let path = entry(root.path());
    fs::write(extra.path().join("first.monoscripts.json"), META).unwrap();
    fs::write(extra.path().join("same.monoscripts.json"), META).unwrap();
    let report = inspect(
        &path,
        &InspectOptions {
            search_roots: vec![extra.path().into()],
        },
    )
    .unwrap();
    assert!(report.diagnostics.is_empty());
}

#[test]
fn provider_can_resolve_native_metadata_without_a_sidecar() {
    struct Native;
    impl ScriptResolver for Native {
        fn resolve(&self, source: &str, reference: &ScriptReference) -> ScriptResolution {
            assert!(source.ends_with("main.prefab"));
            assert_eq!(reference.file_id, 11500000);
            ScriptResolution::Resolved(ScriptIdentity {
                assembly: "Native".into(),
                namespace: "".into(),
                class_name: "Mover".into(),
            })
        }
    }
    let root = TempDir::new().unwrap();
    let report =
        inspect_with_script_resolver(&entry(root.path()), &InspectOptions::default(), &Native)
            .unwrap();
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        report.scripts[0].script_identity.as_ref().unwrap().assembly,
        "Native"
    );
    let draft = prepare_ir_with_script_resolver(
        &root.path().join("main.prefab"),
        &InspectOptions::default(),
        &Native,
    )
    .unwrap();
    assert!(draft.diagnostics.is_empty());
    assert_eq!(
        draft.custom_components[0].script.as_ref().unwrap().assembly,
        "Native"
    );
    assert_eq!(draft.custom_components[0].node_id, draft.nodes[0].id);
}
