use noa_compiler::{
    InspectOptions,
    convert::{
        EntityIds, SourceIdentity, convert_color_parameter, convert_gradient,
        convert_particle_colors,
    },
    prepare_ir,
    resolver::{AssetIndex, resolve_color_context},
    unity::UnityFile,
};
use noa_format::ir::{ColorMode, ColorSpace, GradientMode};
use serde_yaml_ng::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

const GRADIENT: &str = "m_Mode: 0\nm_ColorSpace: -1\nm_NumColorKeys: 2\nm_NumAlphaKeys: 3\nkey0: {r: 0.5, g: 2, b: -0.25, a: 0.25}\nkey1: {r: 0.75, g: 0.1, b: 1, a: 0.5}\nkey2: {a: 1}\nctime0: 12345\nctime1: 65535\natime0: 0\natime1: 32768\natime2: 65535\n";

fn gradient() -> Value {
    serde_yaml_ng::from_str(GRADIENT).unwrap()
}

#[test]
fn gradient_preserves_authored_hdr_values_independent_key_times_and_unknown_space() {
    let value = convert_gradient(&gradient(), "fixture.gradient").unwrap();
    assert_eq!(value.mode, GradientMode::Blend);
    assert_eq!(value.key_color_space, ColorSpace::Unspecified);
    assert_eq!(value.rgb.len(), 2);
    assert_eq!(value.alpha.len(), 3);
    assert_eq!(value.rgb[0].r, 0.5);
    assert_eq!(value.rgb[0].g, 2.0);
    assert_eq!(value.rgb[0].b, -0.25);
    assert_eq!(value.rgb[0].time, 12345.0 / 65535.0);
    assert_eq!(value.alpha[1].time, 32768.0 / 65535.0);
    assert_eq!(value.alpha[0].alpha, 0.25);
}

#[test]
fn gradient_modes_and_key_metadata_are_preserved_without_rgb_conversion() {
    for (mode, expected) in [
        (0, GradientMode::Blend),
        (1, GradientMode::Fixed),
        (2, GradientMode::PerceptualBlend),
    ] {
        for (space, expected_space) in [
            (-1, ColorSpace::Unspecified),
            (0, ColorSpace::SRGB),
            (1, ColorSpace::Linear),
        ] {
            let mut value = gradient();
            value["m_Mode"] = Value::from(mode);
            value["m_ColorSpace"] = Value::from(space);
            let converted = convert_gradient(&value, "fixture.gradient").unwrap();
            assert_eq!(converted.mode, expected);
            assert_eq!(converted.key_color_space, expected_space);
            assert_eq!(converted.rgb[0].r, 0.5);
        }
    }
}

#[test]
fn all_color_parameter_modes_keep_only_the_active_values() {
    for (mode, expected) in [
        (0, ColorMode::Constant),
        (1, ColorMode::Gradient),
        (2, ColorMode::TwoColors),
        (3, ColorMode::TwoGradients),
        (4, ColorMode::RandomColor),
    ] {
        let mut value: Value = serde_yaml_ng::from_str("minMaxState: 0\nminColor: {r: 0.5, g: 1, b: 0.1, a: 0.25}\nmaxColor: {r: 2, g: -1, b: 0.5, a: 0.5}\n").unwrap();
        value["minMaxState"] = Value::from(mode);
        value["minGradient"] = gradient();
        value["maxGradient"] = gradient();
        let converted = convert_color_parameter(&value, "color").unwrap();
        assert_eq!(converted.mode, expected);
        assert_eq!(converted.color_min.is_some(), mode == 2);
        assert_eq!(converted.color_max.is_some(), matches!(mode, 0 | 2));
        assert_eq!(converted.gradient_min.is_some(), mode == 3);
        assert_eq!(converted.gradient_max.is_some(), matches!(mode, 1 | 3 | 4));
        if let Some(color) = converted.color_max {
            assert_eq!(color.r, 2.0);
            assert_eq!(color.g, -1.0);
            assert_eq!(color.a, 0.5);
        }
    }
    let minimal: Value = serde_yaml_ng::from_str("minMaxState: 0\nmaxColor: {r: 0.5, g: 0.5, b: 0.5, a: 0.25}\nminGradient: garbage\nmaxGradient: garbage\n").unwrap();
    assert_eq!(
        convert_color_parameter(&minimal, "constant")
            .unwrap()
            .color_max
            .unwrap()
            .r,
        0.5
    );
}

#[test]
fn invalid_gradient_data_and_unknown_modes_fail_with_property_context() {
    for (key, bad) in [
        ("m_Mode", 3),
        ("m_ColorSpace", 42),
        ("m_NumColorKeys", 0),
        ("m_NumAlphaKeys", 9),
        ("ctime1", 65536),
        ("atime1", -1),
        ("ctime1", 12345),
        ("ctime1", 1),
    ] {
        let mut value = gradient();
        value[key] = Value::from(bad);
        let error = convert_gradient(&value, "input.system.ColorModule.gradient").unwrap_err();
        assert!(error.message.contains("input.system.ColorModule.gradient"));
    }
    for bad in [f64::NAN, f64::INFINITY, 1e100] {
        let mut value = gradient();
        value["key0"]["r"] = Value::from(bad);
        assert!(
            convert_gradient(&value, "gradient")
                .unwrap_err()
                .message
                .contains("gradient.key0")
        );
    }
    let mut value = gradient();
    value.as_mapping_mut().unwrap().remove(Value::from("key1"));
    assert!(convert_gradient(&value, "gradient").is_err());
    let invalid: Value = serde_yaml_ng::from_str("minMaxState: 99").unwrap();
    assert_eq!(
        convert_color_parameter(&invalid, "color").unwrap_err().code,
        "UNSUPPORTED_COLOR_MODE"
    );
}

fn sample_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/scenario/effect/kirakira_01_rip/kirakira_01.prefab")
}

fn ids(file: &UnityFile) -> EntityIds {
    EntityIds::allocate(
        file.objects
            .keys()
            .map(|id| SourceIdentity::new("sample", *id)),
    )
    .unwrap()
}

#[test]
fn sample_keeps_all_color_consumers_and_explicit_source_environment_requirement() {
    let path = sample_path();
    let file = UnityFile::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let ids = ids(&file);
    let fragment = convert_particle_colors("sample", &file, &ids).unwrap();
    assert_eq!(fragment.systems.len(), 16);
    assert_eq!(fragment.renderers.len(), 16);
    assert!(
        fragment
            .renderers
            .iter()
            .all(|r| r.apply_active_color_space)
    );
    assert_eq!(
        fragment
            .systems
            .iter()
            .filter(|p| p.start_color.mode == ColorMode::RandomColor)
            .count(),
        11
    );
    assert_eq!(
        fragment
            .systems
            .iter()
            .filter(|p| p.over_lifetime.is_some())
            .count(),
        15
    );
    assert!(fragment.systems.iter().all(|p| p.custom_streams.is_empty()));
    for object in file
        .objects
        .values()
        .filter(|o| o.type_name == "ParticleSystem")
    {
        let converted = fragment
            .systems
            .iter()
            .find(|p| p.system_component_id == ids.get("sample", object.file_id).unwrap())
            .unwrap();
        let source = &object.data["InitialModule"]["startColor"];
        if let Some(gradient) = &converted.start_color.gradient_max {
            assert_eq!(gradient.key_color_space, ColorSpace::Unspecified);
            assert_eq!(
                gradient.rgb[0].r,
                source["maxGradient"]["key0"]["r"].as_f64().unwrap() as f32
            );
        }
    }
    let draft = prepare_ir(&path, &InspectOptions::default()).unwrap();
    assert_eq!(draft.particle_colors.len(), 16);
    assert_eq!(draft.renderer_colors.len(), 16);
    assert_eq!(draft.pending_components.len(), 32);
    let context = draft.color_context.unwrap();
    assert_eq!(context.project_color_space, ColorSpace::Unspecified);
    assert_eq!(
        context.host_project_color_space_slot.as_deref(),
        Some("source-project-color-space")
    );
    assert_eq!(draft.diagnostics.len(), 1);
    assert_eq!(
        draft.diagnostics[0].code,
        "HOST_PROJECT_COLOR_SPACE_REQUIRED"
    );
    assert_eq!(
        draft.diagnostics[0].severity,
        noa_compiler::diagnostics::Severity::Warning
    );
    for (prepared, direct) in draft.particle_colors.iter().zip(fragment.systems) {
        assert_eq!(prepared.start_color, direct.start_color);
        assert_eq!(prepared.over_lifetime, direct.over_lifetime);
    }
}

#[test]
fn renderer_flags_and_custom_color_streams_remain_independent() {
    let mut file = UnityFile::parse(&fs::read_to_string(sample_path()).unwrap()).unwrap();
    let renderer = file
        .objects
        .values_mut()
        .find(|o| o.type_name == "ParticleSystemRenderer")
        .unwrap();
    let renderer_id = renderer.file_id;
    renderer.data["m_ApplyActiveColorSpace"] = Value::from(0);
    let system = file
        .objects
        .values_mut()
        .find(|o| o.type_name == "ParticleSystem")
        .unwrap();
    let system_id = system.file_id;
    system.data["CustomDataModule"]["enabled"] = Value::Bool(true);
    system.data["CustomDataModule"]["mode0"] = Value::from(2);
    system.data["CustomDataModule"]["mode1"] = Value::from(0);
    system.data["CustomDataModule"]["color0"] =
        serde_yaml_ng::from_str("minMaxState: 0\nmaxColor: {r: 0.5, g: 2, b: 0, a: 0.25}").unwrap();
    let ids = ids(&file);
    let fragment = convert_particle_colors("sample", &file, &ids).unwrap();
    assert!(
        !fragment
            .renderers
            .iter()
            .find(|r| r.renderer_component_id == ids.get("sample", renderer_id).unwrap())
            .unwrap()
            .apply_active_color_space
    );
    let stream = &fragment
        .systems
        .iter()
        .find(|s| s.system_component_id == ids.get("sample", system_id).unwrap())
        .unwrap()
        .custom_streams[0];
    assert_eq!(stream.slot, 0);
    assert_eq!(stream.color.color_max.unwrap().r, 0.5);
    file.objects.get_mut(&system_id).unwrap().data["ColorBySpeedModule"]["enabled"] =
        Value::Bool(true);
    assert_eq!(
        convert_particle_colors("sample", &file, &ids)
            .unwrap_err()
            .code,
        "UNSUPPORTED_COLOR_BY_SPEED"
    );
}

#[test]
fn project_settings_are_discovered_but_dependency_projects_cannot_override_them() {
    let root = TempDir::new().unwrap();
    let assets = root.path().join("Assets/Effects");
    let settings = root.path().join("ProjectSettings");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&settings).unwrap();
    let prefab = assets.join("main.prefab");
    fs::write(&prefab, "--- !u!1 &1\nGameObject: {}\n").unwrap();
    let extra = TempDir::new().unwrap();
    fs::create_dir_all(extra.path().join("Assets")).unwrap();
    fs::create_dir_all(extra.path().join("ProjectSettings")).unwrap();
    fs::write(
        extra.path().join("ProjectSettings/ProjectSettings.asset"),
        "--- !u!129 &1\nPlayerSettings:\n  m_ActiveColorSpace: 1\n",
    )
    .unwrap();
    let index = AssetIndex::discover(&prefab, &[extra.path().join("Assets")]).unwrap();
    let unknown = resolve_color_context(&index).unwrap();
    assert_eq!(unknown.project_color_space, ColorSpace::Unspecified);
    assert_eq!(
        unknown.host_project_color_space_slot.as_deref(),
        Some("source-project-color-space")
    );
    for (raw, expected) in [(0, ColorSpace::SRGB), (1, ColorSpace::Linear)] {
        fs::write(
            settings.join("ProjectSettings.asset"),
            format!("--- !u!129 &1\nPlayerSettings:\n  m_ActiveColorSpace: {raw}\n"),
        )
        .unwrap();
        let context = resolve_color_context(&index).unwrap();
        assert_eq!(context.project_color_space, expected);
        assert!(context.host_project_color_space_slot.is_none());
    }
    fs::write(
        settings.join("ProjectSettings.asset"),
        "--- !u!129 &1\nPlayerSettings:\n  m_ActiveColorSpace: 8\n",
    )
    .unwrap();
    assert_eq!(
        resolve_color_context(&index).unwrap_err().code,
        "UNSUPPORTED_COLOR_SPACE"
    );
}
