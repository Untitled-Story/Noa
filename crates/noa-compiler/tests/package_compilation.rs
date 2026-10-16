use noa_compiler::{
    InspectOptions, compile, compile_to_path,
    convert::{
        EntityIds, SourceIdentity,
        curves::{curve, scalar},
        particles::convert_system,
    },
    unity::UnityFile,
};
use noa_format::{document, package::Manifest, wire as w};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/scenario/effect/kirakira_01_rip/kirakira_01.prefab")
}
fn opts() -> InspectOptions {
    InspectOptions {
        search_roots: vec![],
    }
}
fn source() -> UnityFile {
    UnityFile::parse(&std::fs::read_to_string(sample()).unwrap()).unwrap()
}
fn copy_sample(destination: &Path) {
    let root = sample().parent().unwrap().to_owned();
    for item in walkdir::WalkDir::new(&root) {
        let item = item.unwrap();
        let dst = destination.join(item.path().strip_prefix(&root).unwrap());
        if item.file_type().is_dir() {
            std::fs::create_dir_all(dst).unwrap();
        } else {
            std::fs::copy(item.path(), dst).unwrap();
        }
    }
}

#[test]
fn constant_particle_parameters_do_not_advertise_unity_curve_requirement() {
    fn constants(value: &mut serde_yaml_ng::Value) {
        match value {
            serde_yaml_ng::Value::Mapping(m) => {
                if m.contains_key("scalar") && m.contains_key("minMaxState") {
                    m.insert("minMaxState".into(), 0.into());
                }
                for value in m.values_mut() {
                    constants(value);
                }
            }
            serde_yaml_ng::Value::Sequence(values) => {
                for value in values {
                    constants(value);
                }
            }
            _ => {}
        }
    }
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    let mut file = source();
    let mut text = String::new();
    for object in file.objects.values_mut() {
        constants(&mut object.data);
        let mut root = serde_yaml_ng::Mapping::new();
        root.insert(object.type_name.clone().into(), object.data.clone());
        text.push_str(&format!("--- !u!{} &{}\n", object.class_id, object.file_id));
        text.push_str(&serde_yaml_ng::to_string(&root).unwrap());
    }
    let prefab = dir.path().join("kirakira_01.prefab");
    std::fs::write(&prefab, text).unwrap();
    let c = compile(&prefab, &opts()).unwrap();
    assert!(
        !c.document
            .features
            .iter()
            .flatten()
            .any(|f| f.name == "curve.unity-infinity")
    );
    assert!(!document::requires_unity_infinity(&c.document));
    document::decode(&c.files["scene.noir"]).unwrap();
    let original = compile(&sample(), &opts()).unwrap();
    assert!(
        original
            .document
            .features
            .iter()
            .flatten()
            .any(|f| f.name == "curve.unity-infinity" && f.required)
    );
}
#[test]
fn sample_compiles_complete_and_roundtrips() {
    let c = compile(&sample(), &opts()).unwrap();
    let d = &c.document;
    assert_eq!(d.nodes.len(), 17);
    assert_eq!(d.components.as_ref().unwrap().len(), 51);
    assert_eq!(d.materials.as_ref().unwrap().len(), 2);
    assert_eq!(d.assets.as_ref().unwrap().len(), 4);
    assert_eq!(d.animations.as_ref().unwrap().len(), 2);
    assert_eq!(document::decode(&c.files["scene.noir"]).unwrap(), *d);
    let components = d.components.as_ref().unwrap();
    let mut counts = BTreeMap::new();
    let mut particles = 0;
    let mut renderers = 0;
    for c in components {
        match &c.data {
            w::ComponentDataT::ParticleSystem(p) => {
                particles += 1;
                for m in p.modules.iter().flatten() {
                    *counts
                        .entry(m.data.particle_module_data_type().variant_name().unwrap())
                        .or_insert(0) += 1;
                }
            }
            w::ComponentDataT::ParticleRenderer(r) => {
                renderers += 1;
                if r.mode == w::RenderMode::Mesh {
                    assert_eq!(r.mesh_asset_ids.as_ref().unwrap().len(), 1);
                }
                if c.enabled {
                    assert!(
                        r.vertex_streams
                            .as_ref()
                            .unwrap()
                            .iter()
                            .any(|s| s.semantic == w::VertexSemantic::Custom1 && s.components == 1)
                    );
                }
            }
            _ => {}
        }
    }
    assert_eq!((particles, renderers), (16, 16));
    for (name, count) in [
        ("EmissionModule", 15),
        ("ShapeModule", 15),
        ("ColorModule", 15),
        ("SizeModule", 4),
        ("VelocityModule", 15),
        ("LimitVelocityModule", 2),
        ("NoiseModule", 4),
        ("TextureSheetModule", 15),
        ("CustomDataModule", 15),
    ] {
        assert_eq!(counts.get(name), Some(&count), "{name}");
    }
    assert_eq!(c.diagnostics.len(), 1);
    assert_eq!(c.diagnostics[0].code, "HOST_PROJECT_COLOR_SPACE_REQUIRED");
    let material = d
        .materials
        .as_ref()
        .unwrap()
        .iter()
        .find(|m| m.model == w::MaterialModel::ParticleAddAlpha)
        .unwrap();
    assert_eq!(material.render_state.src_rgb, w::BlendFactor::One);
    assert_eq!(
        material.render_state.dst_rgb,
        w::BlendFactor::OneMinusSrcAlpha
    );
    assert!(!material.render_state.depth_write);
    assert_eq!(material.render_state.color_write_mask, 7);
    let custom = components
        .iter()
        .find_map(|c| {
            if let w::ComponentDataT::CustomComponent(s) = &c.data {
                (s.script.as_ref().unwrap().class_name == "ScenarioEffector").then_some(s)
            } else {
                None
            }
        })
        .unwrap();
    let animator = custom
        .fields
        .iter()
        .find(|f| f.name == "mainAnimator")
        .unwrap();
    let w::SerializedValueDataT::ReferenceValue(r) = &animator.value.data else {
        panic!()
    };
    assert_eq!(r.source.file_id, 95256069497284392);
    let target = r.target.as_ref().unwrap();
    assert!(
        components
            .iter()
            .any(|c| c.id == target.id && matches!(c.data, w::ComponentDataT::Animator(_)))
    );
}
#[test]
fn package_is_deterministic_and_manifest_hashes_all_payloads() {
    let a = compile(&sample(), &opts()).unwrap();
    let b = compile(&sample(), &opts()).unwrap();
    let mut left = Cursor::new(Vec::new());
    let mut right = Cursor::new(Vec::new());
    a.write_archive(&mut left).unwrap();
    b.write_archive(&mut right).unwrap();
    assert_eq!(left.get_ref(), right.get_ref());
    let mut zip = zip::ZipArchive::new(Cursor::new(left.into_inner())).unwrap();
    let manifest: Manifest =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest.files.len() + 1, zip.len());
    for f in manifest.files {
        let mut bytes = Vec::new();
        zip.by_name(&f.path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(f.byte_length, bytes.len() as u64);
        assert_eq!(f.sha256, format!("{:x}", Sha256::digest(&bytes)));
        if f.kind == "mesh" {
            noa_format::mesh::MeshData::decode(&bytes).unwrap();
        }
    }
}
#[test]
fn failed_compile_and_failed_write_preserve_old_output() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("effect.noap");
    std::fs::write(&output, b"old").unwrap();
    assert!(compile_to_path(&dir.path().join("missing.prefab"), &output, &opts()).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"old");
    let mut c = compile(&sample(), &opts()).unwrap();
    c.document.default_entry_id = 0;
    assert!(c.write(&output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn compiler_rejects_enabled_unknown_modules_and_ignores_disabled_data() {
    let mut file = source();
    let id = file
        .objects
        .values()
        .find(|o| o.class_id == 198)
        .unwrap()
        .file_id;
    let ids = EntityIds::allocate(
        file.objects
            .keys()
            .map(|id| SourceIdentity::new("entry", *id)),
    )
    .unwrap();
    file.objects.get_mut(&id).unwrap().data["CollisionModule"]["enabled"] =
        serde_yaml_ng::Value::from(1);
    assert!(
        convert_system("entry", &file.objects[&id], &ids, &file)
            .unwrap_err()
            .message
            .contains("CollisionModule")
    );
    file.objects.get_mut(&id).unwrap().data["CollisionModule"]["enabled"] =
        serde_yaml_ng::Value::from(0);
    file.objects.get_mut(&id).unwrap().data["CollisionModule"]["type"] =
        serde_yaml_ng::Value::from(999);
    convert_system("entry", &file.objects[&id], &ids, &file).unwrap();
}
#[test]
fn curves_preserve_reversed_random_endpoints_weighted_segments_and_steps() {
    let constant: serde_yaml_ng::Value =
        serde_yaml_ng::from_str("minMaxState: 3\nscalar: 0.2\nminScalar: 0.9").unwrap();
    let p = scalar(&constant, w::CurveDomain::Normalized).unwrap();
    assert_eq!(p.mode, w::ScalarMode::TwoConstants);
    assert_eq!((p.constant_min, p.constant_max), (0.9, 0.2));
    let mut v:serde_yaml_ng::Value=serde_yaml_ng::from_str("m_PreInfinity: 2\nm_PostInfinity: 2\nm_Curve:\n- {time: 0, value: 0.5, inSlope: 0, outSlope: 2, weightedMode: 2, inWeight: 0.3, outWeight: 0.2}\n- {time: 1, value: 2, inSlope: 1, outSlope: 0, weightedMode: 0, inWeight: 0.3, outWeight: 0.3}").unwrap();
    let c = curve(&v, w::CurveDomain::Seconds).unwrap();
    assert_eq!(c.keys[0].interpolation, w::Interpolation::WeightedBezier);
    assert_eq!(c.keys[0].out_weight, 0.2);
    assert_eq!(c.source_pre_infinity, 2);
    v["m_Curve"][0]["outSlope"] = serde_yaml_ng::Value::from(f64::INFINITY);
    assert_eq!(
        curve(&v, w::CurveDomain::Seconds).unwrap().keys[0].interpolation,
        w::Interpolation::Step
    );
    v["m_Curve"][1]["time"] = serde_yaml_ng::Value::from(0);
    assert!(curve(&v, w::CurveDomain::Seconds).is_err());
}
#[test]
fn streams_remain_hex_even_when_all_digits() {
    let f=UnityFile::parse("--- !u!199 &1\nParticleSystemRenderer:\n  m_VertexStreams: 00010304\n  m_TrailVertexStreams: 00010304\n").unwrap();
    assert_eq!(
        f.objects[&1].data["m_VertexStreams"].as_str(),
        Some("00010304")
    );
}
#[test]
fn noir_validation_rejects_bad_references_nan_unknown_enum_and_corruption() {
    let c = compile(&sample(), &opts()).unwrap();
    let mut d = c.document.clone();
    d.nodes[0].parent_id = u32::MAX;
    assert!(document::encode(&d).is_err());
    let mut d = c.document.clone();
    d.nodes[0].transform.position.as_mut().unwrap().x = f32::NAN;
    assert!(document::encode(&d).is_err());
    let mut d = c.document.clone();
    d.assets.as_mut().unwrap()[0].kind = w::AssetKind(255);
    assert!(document::encode(&d).is_err());
    let mut bytes = c.files["scene.noir"].clone();
    bytes[4] = 0;
    assert!(document::decode(&bytes).is_err());
    assert!(document::decode(&bytes[..6]).is_err());
}

#[test]
fn manifest_rejects_duplicate_paths_asset_ids_and_traversal() {
    let c = compile(&sample(), &opts()).unwrap();
    let m = c.manifest().unwrap();
    m.validate().unwrap();
    let mut bad = m.clone();
    bad.files.push(bad.files[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = m.clone();
    bad.files[0].path = "../outside.png".into();
    assert!(bad.validate().is_err());
    let mut bad = m.clone();
    bad.files[0].sha256 = "z".repeat(64);
    assert!(bad.validate().is_err());
    let mut bad = m.clone();
    bad.files[1].asset_id = bad.files[0].asset_id;
    assert!(bad.validate().is_err());
}

#[test]
fn shader_recipe_uses_program_identity_and_unknown_program_fails() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    let shader = dir.path().join("shaders/particle add + alpha blend.asset");
    let text = std::fs::read_to_string(&shader)
        .unwrap()
        .replace("Sekai/Particles/Additive+AlphaBlend", "Renamed/Program");
    std::fs::write(&shader, &text).unwrap();
    let c = compile(&dir.path().join("kirakira_01.prefab"), &opts()).unwrap();
    assert!(
        c.document
            .materials
            .as_ref()
            .unwrap()
            .iter()
            .any(|m| m.source_shader.as_deref() == Some("Renamed/Program"))
    );
    let changed = text.replace("compressedBlob: 53040000", "compressedBlob: 54040000");
    assert_ne!(text, changed);
    std::fs::write(&shader, changed).unwrap();
    assert!(
        compile(&dir.path().join("kirakira_01.prefab"), &opts())
            .unwrap_err()
            .message
            .contains("unrecognized program")
    );
}

#[test]
fn data_only_scene_and_full_width_script_numbers_compile_without_game_assets() {
    let dir = tempfile::tempdir().unwrap();
    let prefab = dir.path().join("plain.prefab");
    std::fs::write(&prefab,"--- !u!1 &1\nGameObject:\n  m_Name: plain\n  m_IsActive: 1\n  m_Layer: 0\n  m_Component: [{component: {fileID: 2}}, {component: {fileID: 3}}]\n--- !u!4 &2\nTransform:\n  m_GameObject: {fileID: 1}\n  m_Father: {fileID: 0}\n  m_Children: []\n  m_LocalPosition: {x: 1, y: 2, z: 3}\n  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}\n  m_LocalScale: {x: 1, y: 1, z: 1}\n--- !u!114 &3\nMonoBehaviour:\n  m_GameObject: {fileID: 1}\n  m_Enabled: 1\n  m_Script: {fileID: 11500000, guid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, type: 3}\n  smallest: -9223372036854775808\n  largest: 18446744073709551615\n  owner: {fileID: 1}\n").unwrap();
    std::fs::write(dir.path().join("test.monoscripts.json"),r#"{"format":"unity-monoscript-metadata","version":1,"scripts":[{"guid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","file_id":"11500000","assembly":"Test","namespace":"Example","class_name":"Data"}]}"#).unwrap();
    let c = compile(&prefab, &opts()).unwrap();
    assert!(c.diagnostics.is_empty());
    assert!(c.document.color_context.is_none());
    assert!(c.document.assets.as_ref().unwrap().is_empty());
    assert_eq!(c.files.len(), 1);
    let d = document::decode(&c.files["scene.noir"]).unwrap();
    let w::ComponentDataT::CustomComponent(s) = &d.components.as_ref().unwrap()[0].data else {
        panic!()
    };
    let field = |name| &s.fields.iter().find(|f| f.name == name).unwrap().value.data;
    let w::SerializedValueDataT::SignedValue(v) = field("smallest") else {
        panic!()
    };
    assert_eq!(v.value, i64::MIN);
    let w::SerializedValueDataT::UnsignedValue(v) = field("largest") else {
        panic!()
    };
    assert_eq!(v.value, u64::MAX);
}

#[test]
fn script_nesting_limit_matches_the_binary_reader() {
    fn tree(depth: usize) -> w::SerializedValueT {
        let mut v = w::SerializedValueT::default();
        if depth == 0 {
            v.data = w::SerializedValueDataT::NullValue(Box::default());
        } else {
            let mut f = w::SerializedFieldT::default();
            f.name = "child".into();
            f.value = Box::new(tree(depth - 1));
            let mut o = w::ObjectValueT::default();
            o.fields = vec![f];
            v.data = w::SerializedValueDataT::ObjectValue(Box::new(o));
        }
        v
    }
    let mut c = compile(&sample(), &opts()).unwrap();
    let script = c
        .document
        .components
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|c| {
            if let w::ComponentDataT::CustomComponent(s) = &mut c.data {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    let mut field = w::SerializedFieldT::default();
    field.name = "nested".into();
    field.value = Box::new(tree(128));
    script.fields.push(field);
    let bytes = document::encode(&c.document).unwrap();
    assert_eq!(document::decode(&bytes).unwrap(), c.document);
    let script = c
        .document
        .components
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|c| {
            if let w::ComponentDataT::CustomComponent(s) = &mut c.data {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    *script.fields.last_mut().unwrap().value = tree(129);
    assert!(document::encode(&c.document).is_err());
}
