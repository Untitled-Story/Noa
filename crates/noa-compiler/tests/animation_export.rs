use noa_compiler::{InspectOptions, compile, unity::UnityFile};
use noa_format::{document, wire as w};
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenario/effect/kirakira_01_rip");
    let dir = tempfile::tempdir().unwrap();
    for e in walkdir::WalkDir::new(&root) {
        let e = e.unwrap();
        let to = dir.path().join(e.path().strip_prefix(&root).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(to).unwrap();
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
    dir
}
fn edit(dir: &Path, f: impl FnOnce(&mut serde_yaml_ng::Value)) {
    let path = dir.join("kirakira_01.anim");
    let mut file = UnityFile::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let o = file.objects.values_mut().next().unwrap();
    f(&mut o.data);
    let mut root = serde_yaml_ng::Mapping::new();
    root.insert(o.type_name.clone().into(), o.data.clone());
    std::fs::write(
        path,
        format!(
            "--- !u!74 &{}\n{}",
            o.file_id,
            serde_yaml_ng::to_string(&root).unwrap()
        ),
    )
    .unwrap();
}
fn anim(d: &w::DocumentT) -> &w::AnimationT {
    d.animations
        .as_ref()
        .unwrap()
        .iter()
        .find(|a| a.name.as_deref() == Some("kirakira_01"))
        .unwrap()
}
const CURVE: &str = "m_PreInfinity: 2\nm_PostInfinity: 2\nm_RotationOrder: 4\nm_Curve:\n- {time: 0, value: 10, inSlope: 0, outSlope: 2, weightedMode: 2, inWeight: 0, outWeight: 0.25}\n- {time: 2, value: 30, inSlope: 3, outSlope: 0, weightedMode: 1, inWeight: 0.4, outWeight: 0}\n";
#[test]
fn exports_partial_transform_curves_bool_keys_and_all_event_parameters() {
    let dir = fixture();
    let guid = std::fs::read_to_string(dir.path().join("kirakira_01.prefab.meta")).unwrap();
    let guid = noa_compiler::unity::parse_meta_guid(&guid)
        .unwrap()
        .to_string();
    let file =
        UnityFile::parse(&std::fs::read_to_string(dir.path().join("kirakira_01.prefab")).unwrap())
            .unwrap();
    let root = file
        .objects
        .values()
        .find(|o| o.class_id == 95)
        .unwrap()
        .data["m_GameObject"]["fileID"]
        .as_i64()
        .unwrap();
    edit(dir.path(), |d| {
        d["m_AnimationClipSettings"]["m_StopTime"] = 2.into();
        d["m_FloatCurves"][0]["curve"]["m_Curve"] = serde_yaml_ng::from_str(
            "[{time: 0, value: 1}, {time: 1, value: 0}, {time: 2, value: 1}]",
        )
        .unwrap();
        let mut track = d["m_FloatCurves"][0].clone();
        track["classID"] = 4.into();
        track["attribute"] = "m_LocalPosition.z".into();
        track["curve"] = serde_yaml_ng::from_str(CURVE).unwrap();
        d["m_FloatCurves"].as_sequence_mut().unwrap().push(track);
        d["m_Events"]=serde_yaml_ng::from_str(&format!("[{{time: 1.25, functionName: Dispatch, data: hello, floatParameter: 1.5, intParameter: -42, messageOptions: 1, objectReferenceParameter: {{fileID: {root}, guid: {guid}, type: 3}}}}]")).unwrap();
    });
    let c = compile(
        &dir.path().join("kirakira_01.prefab"),
        &InspectOptions::default(),
    )
    .unwrap();
    let d = document::decode(&document::encode(&c.document).unwrap()).unwrap();
    let a = anim(&d);
    assert_eq!(a.duration_seconds, 2.);
    let t = a
        .tracks
        .as_ref()
        .unwrap()
        .iter()
        .find(|t| t.binding.property == w::AnimationProperty::NodePosition)
        .unwrap();
    let w::TrackDataT::Vector3Track(t) = &t.data else {
        panic!()
    };
    assert!(t.x.is_none() && t.y.is_none());
    let z = t.z.as_ref().unwrap();
    assert_eq!(z.domain, w::CurveDomain::Seconds);
    assert_eq!(z.keys[0].interpolation, w::Interpolation::WeightedBezier);
    assert_eq!(z.keys[1].time, 2.);
    assert_eq!(z.keys[1].in_slope, 3.);
    let e = &a.events.as_ref().unwrap()[0];
    assert_eq!(e.name, "Dispatch");
    assert_eq!(e.string_parameter.as_deref(), Some("hello"));
    assert_eq!(e.float_parameter, 1.5);
    assert_eq!(e.int_parameter, -42);
    assert_eq!(
        e.message_options,
        w::AnimationMessageOptions::DontRequireReceiver
    );
    assert_eq!(
        e.object_parameter
            .as_ref()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .kind,
        w::ReferenceKind::Node
    );
    assert!(d.components.iter().flatten().any(
        |c| c.id == e.animator_component_id && matches!(c.data, w::ComponentDataT::Animator(_))
    ));
    let mut bad = d.clone();
    bad.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "animation.events-typed");
    assert!(document::encode(&bad).unwrap_err().contains("events-typed"));
    let mut bad = d.clone();
    bad.animations
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a.name.as_deref() == Some("kirakira_01"))
        .unwrap()
        .events
        .as_mut()
        .unwrap()[0]
        .object_parameter
        .as_mut()
        .unwrap()
        .target = None;
    assert!(
        document::encode(&bad)
            .unwrap_err()
            .contains("unresolved event")
    );
}
#[test]
fn vector_euler_curves_convert_values_and_tangents_to_radians() {
    let dir = fixture();
    edit(dir.path(), |d| {
        d["m_AnimationClipSettings"]["m_StopTime"] = 2.into();
        let mut curve: serde_yaml_ng::Value = serde_yaml_ng::from_str(CURVE).unwrap();
        for k in curve["m_Curve"].as_sequence_mut().unwrap() {
            for name in ["value", "inSlope", "outSlope", "inWeight", "outWeight"] {
                let v = k[name].clone();
                k[name] = serde_yaml_ng::from_str("{x: 0, y: 0, z: 0}").unwrap();
                for axis in ["x", "y", "z"] {
                    k[name][axis] = v.clone();
                }
            }
        }
        let mut entry = serde_yaml_ng::Mapping::new();
        entry.insert("path".into(), d["m_FloatCurves"][0]["path"].clone());
        entry.insert("curve".into(), curve);
        d["m_EulerCurves"] = serde_yaml_ng::Value::Sequence(vec![entry.into()]);
    });
    let c = compile(
        &dir.path().join("kirakira_01.prefab"),
        &InspectOptions::default(),
    )
    .unwrap();
    let a = anim(&c.document);
    let t = a
        .tracks
        .as_ref()
        .unwrap()
        .iter()
        .find_map(|t| match &t.data {
            w::TrackDataT::RotationTrack(r) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.encoding, w::RotationEncoding::EulerZXY);
    assert!((t.z.as_ref().unwrap().keys[1].value - 30f32.to_radians()).abs() < 1e-6);
    assert!((t.z.as_ref().unwrap().keys[1].in_slope - 3f32.to_radians()).abs() < 1e-6);
    assert!(t.w.is_none());
}
#[test]
fn unknown_bindings_conflicts_and_missing_event_objects_fail_explicitly() {
    let dir = fixture();
    edit(dir.path(), |d| {
        let t = d["m_FloatCurves"][0].clone();
        d["m_FloatCurves"].as_sequence_mut().unwrap().push(t);
    });
    assert!(
        compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default()
        )
        .unwrap_err()
        .message
        .contains("conflicting")
    );
    let dir = fixture();
    edit(dir.path(), |d| {
        d["m_FloatCurves"][0]["attribute"] = "script_0x12345678_unknown".into()
    });
    assert!(
        compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default()
        )
        .is_err()
    );
    let dir = fixture();
    edit(dir.path(), |d| {
        d["m_Events"]=serde_yaml_ng::from_str("[{time: 0, functionName: Dispatch, data: '', floatParameter: 0, intParameter: 0, messageOptions: 0, objectReferenceParameter: {fileID: 999}}]").unwrap()
    });
    assert!(
        compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default()
        )
        .is_err()
    );
}

#[test]
fn exports_layout_particle_color_scalar_and_looping_bindings() {
    let dir = fixture();
    let mut file =
        UnityFile::parse(&std::fs::read_to_string(dir.path().join("kirakira_01.prefab")).unwrap())
            .unwrap();
    let particle = file
        .objects
        .values_mut()
        .find(|o| o.class_id == 198)
        .unwrap();
    particle.data["CustomDataModule"]["enabled"] = 1.into();
    particle.data["CustomDataModule"]["mode0"] = 2.into();
    particle.data["CustomDataModule"]["mode1"] = 2.into();
    let mut text = String::new();
    for o in file.objects.values() {
        let mut root = serde_yaml_ng::Mapping::new();
        root.insert(o.type_name.clone().into(), o.data.clone());
        text.push_str(&format!(
            "--- !u!{} &{}\n{}",
            o.class_id,
            o.file_id,
            serde_yaml_ng::to_string(&root).unwrap()
        ));
    }
    std::fs::write(dir.path().join("kirakira_01.prefab"), text).unwrap();
    let nodes = noa_compiler::unity::scene_nodes(&file).unwrap();
    let root = file
        .objects
        .values()
        .find(|o| o.class_id == 95)
        .unwrap()
        .data["m_GameObject"]["fileID"]
        .as_i64()
        .unwrap();
    let particle = file.objects.values().find(|o| o.class_id == 198).unwrap();
    let node_id = particle.data["m_GameObject"]["fileID"].as_i64().unwrap();
    let mut node = nodes.iter().find(|n| n.file_id == node_id).unwrap();
    let mut parts = Vec::new();
    while node.file_id != root {
        parts.push(node.name.as_str());
        node = nodes
            .iter()
            .find(|n| Some(n.file_id) == node.parent_file_id)
            .unwrap();
    }
    parts.reverse();
    let path = parts.join("/");
    edit(dir.path(), |d| {
        d["m_AnimationClipSettings"]["m_StopTime"] = 2.into();
        let original = d["m_FloatCurves"][0].clone();
        let tracks = d["m_FloatCurves"].as_sequence_mut().unwrap();
        for (class, attr, path) in [
            (224, "m_AnchoredPosition.x", ""),
            (198, "InitialModule.startColor.maxColor.r", path.as_str()),
            (198, "InitialModule.startSpeed.scalar", path.as_str()),
            (198, "looping", path.as_str()),
            (198, "InitialModule.gravityModifier.scalar", path.as_str()),
            (198, "CustomDataModule.color0.maxColor.r", path.as_str()),
            (198, "CustomDataModule.color1.maxColor.r", path.as_str()),
            (199, "material._MainTex_ST.w", path.as_str()),
        ] {
            let mut t = original.clone();
            t["classID"] = class.into();
            t["attribute"] = attr.into();
            t["path"] = path.into();
            t["curve"] = serde_yaml_ng::from_str(CURVE).unwrap();
            if attr == "looping" {
                t["curve"]["m_Curve"] =
                    serde_yaml_ng::from_str("[{time: 0, value: 1}, {time: 2, value: 0}]").unwrap();
            }
            tracks.push(t);
        }
    });
    let c = compile(
        &dir.path().join("kirakira_01.prefab"),
        &InspectOptions::default(),
    )
    .unwrap();
    let d = document::decode(&document::encode(&c.document).unwrap()).unwrap();
    let tracks = anim(&d).tracks.as_ref().unwrap();
    for prop in [
        w::AnimationProperty::RectAnchoredPosition,
        w::AnimationProperty::ParticleColor,
        w::AnimationProperty::ParticleParameter,
        w::AnimationProperty::ParticleLoop,
    ] {
        assert!(
            tracks.iter().any(|t| t.binding.property == prop),
            "{prop:?}"
        );
    }
    assert_eq!(
        tracks
            .iter()
            .filter(|t| t.binding.property == w::AnimationProperty::ParticleCustomColor)
            .count(),
        2
    );
    assert!(tracks.iter().any(
        |t| matches!(&t.data, w::TrackDataT::Vector4Track(v) if v.w.is_some() && v.x.is_none())
    ));
    assert!(d.components.iter().flatten().any(|c| matches!(&c.data,w::ComponentDataT::ParticleSystem(p) if p.main.host_gravity_slot.as_deref() == Some("world-gravity"))));
    let scalar = tracks
        .iter()
        .find(|t| {
            t.binding.property == w::AnimationProperty::ParticleParameter
                && t.binding.parameter == w::ParticleParameter::StartSpeed
        })
        .unwrap();
    assert_eq!(scalar.binding.property_name.as_deref(), Some("scalar"));
    let w::TrackDataT::FloatTrack(f) = &scalar.data else {
        panic!()
    };
    assert_eq!(f.curve.keys[1].time, 2.);
}

#[test]
fn event_string_parameters_keep_lexical_values_in_unity_yaml() {
    for (input, expected) in [
        ("001", "001"),
        ("true", "true"),
        ("", ""),
        ("'quoted'", "quoted"),
    ] {
        let text = format!(
            "--- !u!74 &7400000\nAnimationClip:\n  m_Events:\n  - time: 0\n    functionName: Notify\n    data: {input}\n"
        );
        let file = UnityFile::parse(&text).unwrap();
        assert_eq!(
            file.objects[&7400000].data["m_Events"][0]["data"].as_str(),
            Some(expected)
        );
    }
}

#[test]
fn event_only_png_is_packaged_and_resolved_to_an_asset() {
    let dir = fixture();
    let root = dir.path();
    std::fs::copy(
        root.join("tex_kirakira_01.png"),
        root.join("event_only.png"),
    )
    .unwrap();
    let meta = std::fs::read_to_string(root.join("tex_kirakira_01.png.meta")).unwrap();
    let old = noa_compiler::unity::parse_meta_guid(&meta)
        .unwrap()
        .to_string();
    let guid = "1234567890abcdef1234567890abcdef";
    std::fs::write(root.join("event_only.png.meta"), meta.replace(&old, guid)).unwrap();
    edit(root, |d| {
        d["m_Events"]=serde_yaml_ng::from_str(&format!("[{{time: 0, functionName: Notify, data: '', floatParameter: 0, intParameter: 0, messageOptions: 0, objectReferenceParameter: {{fileID: 2800000, guid: {guid}, type: 3}}}}]")).unwrap()
    });
    let c = compile(&root.join("kirakira_01.prefab"), &InspectOptions::default()).unwrap();
    let d = document::decode(&document::encode(&c.document).unwrap()).unwrap();
    let target = anim(&d).events.as_ref().unwrap()[0]
        .object_parameter
        .as_ref()
        .unwrap()
        .target
        .as_ref()
        .unwrap();
    assert_eq!(target.kind, w::ReferenceKind::Asset);
    assert!(
        d.assets
            .iter()
            .flatten()
            .any(|a| a.id == target.id && a.kind == w::AssetKind::Image)
    );
    assert_eq!(d.assets.as_ref().unwrap().len(), 5);
}
