use noa_compiler::{InspectOptions, compile, unity::UnityFile};
use noa_format::{document, wire as w};
use std::path::{Path, PathBuf};
fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenario/effect/kirakira_01_rip")
}
fn copy_sample(destination: &Path) {
    for item in walkdir::WalkDir::new(sample()) {
        let item = item.unwrap();
        let dst = destination.join(item.path().strip_prefix(sample()).unwrap());
        if item.file_type().is_dir() {
            std::fs::create_dir_all(dst).unwrap();
        } else {
            std::fs::copy(item.path(), dst).unwrap();
        }
    }
}
fn material_source(id: i64, default_texture: bool, tint: bool) -> String {
    let mut f =
        UnityFile::parse(&std::fs::read_to_string(sample().join("mat_kirakira_01.mat")).unwrap())
            .unwrap();
    let o = f.objects.values_mut().find(|o| o.class_id == 21).unwrap();
    let v = &mut o.data;
    v["m_Name"] = "renamed material with misleading blend properties".into();
    v["m_Shader"] = serde_yaml_ng::from_str(&format!(
        "{{fileID: {id}, guid: 0000000000000000f000000000000000, type: 0}}"
    ))
    .unwrap();
    v["m_CustomRenderQueue"] = 3101.into();
    let props = &mut v["m_SavedProperties"];
    props["m_Floats"]["_SrcBlend"] = 0.into();
    props["m_Floats"]["_DstBlend"] = 0.into();
    props["m_Floats"]["_ZWrite"] = 1.into();
    props["m_Floats"]["_InvFade"] = 2.25.into();
    // _Color is a stale Standard shader property; only _TintColor belongs to this shader.
    props["m_Colors"]["_Color"] = serde_yaml_ng::from_str("{r: 10, g: 20, b: 30, a: 40}").unwrap();
    if tint {
        props["m_Colors"]["_TintColor"] =
            serde_yaml_ng::from_str("{r: 0.1, g: 0.2, b: 0.3, a: 0.4}").unwrap();
    } else {
        props["m_Colors"]
            .as_mapping_mut()
            .unwrap()
            .remove("_TintColor");
    }
    if default_texture {
        props["m_TexEnvs"]
            .as_mapping_mut()
            .unwrap()
            .remove("_MainTex");
    }
    let mut root = serde_yaml_ng::Mapping::new();
    root.insert("Material".into(), v.clone());
    format!(
        "--- !u!21 &{}\n{}",
        o.file_id,
        serde_yaml_ng::to_string(&root).unwrap()
    )
}
#[test]
fn builtin_particle_recipes_use_real_blending_tint_and_explicit_environment() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    for (fid, model, dst, feature) in [
        (
            10720,
            w::MaterialModel::LegacyParticleAdditive,
            w::BlendFactor::One,
            "material.unity-legacy-particle-additive",
        ),
        (
            10721,
            w::MaterialModel::LegacyParticleAlphaBlend,
            w::BlendFactor::OneMinusSrcAlpha,
            "material.unity-legacy-particle-alpha-blend",
        ),
    ] {
        for default_texture in [false, true] {
            std::fs::write(
                dir.path().join("mat_kirakira_01.mat"),
                material_source(fid, default_texture, !default_texture),
            )
            .unwrap();
            let c = compile(
                &dir.path().join("kirakira_01.prefab"),
                &InspectOptions::default(),
            )
            .unwrap();
            assert_eq!(
                document::decode(&c.files["scene.noir"]).unwrap(),
                c.document
            );
            let m = c
                .document
                .materials
                .iter()
                .flatten()
                .find(|m| m.model == model)
                .unwrap();
            let tint = m.tint.as_ref().unwrap();
            assert_eq!(
                (tint.r, tint.g, tint.b, tint.a),
                if default_texture {
                    (0.5, 0.5, 0.5, 0.5)
                } else {
                    (0.1, 0.2, 0.3, 0.4)
                }
            );
            assert_eq!(m.render_state.src_rgb, w::BlendFactor::SrcAlpha);
            assert_eq!(m.render_state.dst_rgb, dst);
            assert!(!m.render_state.depth_write);
            assert_eq!(m.render_state.cull, w::CullMode::None);
            assert_eq!(m.render_state.color_write_mask, 7);
            assert_eq!(m.render_queue, 3101);
            assert_eq!(m.floats.as_ref().unwrap()[0].value, 2.25);
            assert!(
                c.diagnostics
                    .iter()
                    .any(|d| d.code == "HOST_PARTICLE_ENVIRONMENT_REQUIRED")
            );
            let recipe: serde_json::Value = serde_json::from_slice(
                &c.files[&format!("assets/{}.shader.json", m.shader_data_asset_id)],
            )
            .unwrap();
            assert_eq!(recipe["source_builtin"]["file_id"], fid.to_string());
            assert_eq!(recipe["environment"]["fog"]["required"], true);
            assert_eq!(
                recipe["environment"]["soft_particles"]["depth_required_when_enabled"],
                true
            );
            assert_eq!(recipe["rgb_premultiply"], false);
            assert!(
                recipe["color_formula"]
                    .as_str()
                    .unwrap()
                    .contains("saturate")
            );
            for name in [feature, "host.legacy-particle-environment"] {
                assert!(
                    c.document
                        .features
                        .iter()
                        .flatten()
                        .any(|f| f.name == name && f.required && f.revision == 1)
                );
                let mut bad = c.document.clone();
                bad.features.as_mut().unwrap().retain(|f| f.name != name);
                assert!(document::encode(&bad).unwrap_err().contains(name));
            }
            if default_texture {
                let img = m.textures.as_ref().unwrap()[0].image_id;
                assert_eq!(
                    c.files[&format!("assets/{img}.png")],
                    include_bytes!("../resources/white.png")
                );
            }
        }
    }
}
#[test]
fn unknown_builtin_shaders_and_local_variants_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    for (fid, variant) in [(10722, false), (10720, true)] {
        let mut text = material_source(fid, false, false);
        if variant {
            text = text.replace("m_ValidKeywords: []", "m_ValidKeywords: [SOFTPARTICLES_ON]");
        }
        std::fs::write(dir.path().join("mat_kirakira_01.mat"), text).unwrap();
        let e = compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default(),
        )
        .unwrap_err();
        assert!(
            e.message.contains(if variant {
                "variant not supported"
            } else {
                "builtin shader"
            }),
            "{e}"
        );
    }
}
