//! T3 import contracts: real exported resources, native references and beta wire formats.
use noa_compiler::{InspectOptions, compile, unity::UnityFile};
use noa_format::{document, mesh::MeshData, wire as w};
use serde_yaml_ng::Value as Y;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let source = repo().join("data/scenario/effect/kirakira_01_rip");
    for e in walkdir::WalkDir::new(&source) {
        let e = e.unwrap();
        let dst = dir.path().join(e.path().strip_prefix(&source).unwrap());
        if e.file_type().is_dir() {
            fs::create_dir_all(dst).unwrap();
        } else {
            fs::copy(e.path(), dst).unwrap();
        }
    }
    dir
}
fn read(dir: &Path) -> UnityFile {
    UnityFile::parse(&fs::read_to_string(dir.join("kirakira_01.prefab")).unwrap()).unwrap()
}
fn write(dir: &Path, f: &UnityFile) {
    let mut text = String::new();
    for o in f.objects.values() {
        text.push_str(&format!("--- !u!{} &{}\n", o.class_id, o.file_id));
        let mut m = serde_yaml_ng::Mapping::new();
        m.insert(o.type_name.clone().into(), o.data.clone());
        text.push_str(&serde_yaml_ng::to_string(&m).unwrap());
    }
    fs::write(dir.join("kirakira_01.prefab"), text).unwrap();
}
fn add(f: &mut UnityFile, class: u32, name: &str, id: i64, fields: &str) {
    let node = f
        .objects
        .values()
        .find(|o| o.class_id == 1)
        .unwrap()
        .file_id;
    let text = format!(
        "--- !u!{class} &{id}\n{name}:\n  m_GameObject: {{fileID: {node}}}\n  m_Enabled: 1\n{fields}"
    );
    let extra = UnityFile::parse(&text).unwrap();
    f.objects.extend(extra.objects);
    let r: Y = serde_yaml_ng::from_str(&format!("{{component: {{fileID: {id}}}}}")).unwrap();
    f.objects.get_mut(&node).unwrap().data["m_Component"]
        .as_sequence_mut()
        .unwrap()
        .push(r);
}
fn copy_asset(from: &Path, dir: &Path) -> String {
    let meta = PathBuf::from(format!("{}.meta", from.display()));
    let text = fs::read_to_string(&meta).unwrap();
    fs::copy(from, dir.join(from.file_name().unwrap())).unwrap();
    fs::copy(
        meta,
        dir.join(format!(
            "{}.meta",
            from.file_name().unwrap().to_str().unwrap()
        )),
    )
    .unwrap();
    let v: Y = serde_yaml_ng::from_str(&text).unwrap();
    v["guid"].as_str().unwrap().to_owned()
}
fn compiled(dir: &Path) -> noa_compiler::Compilation {
    compile(&dir.join("kirakira_01.prefab"), &InspectOptions::default())
        .unwrap_or_else(|e| panic!("{e}"))
}
#[test]
fn real_mesh_renderer_preserves_geometry_and_validates_filter_and_capability() {
    let dir = fixture();
    let mut f = read(dir.path());
    let mesh=repo().join("data/scenario/effect/sekai_tree_flash_01_rip/ExportedProject/Assets/Mesh/sekai_PT_05.asset");
    let guid = copy_asset(&mesh, dir.path());
    add(
        &mut f,
        33,
        "MeshFilter",
        10001,
        &format!("  m_Mesh: {{fileID: 4300000, guid: {guid}, type: 2}}\n"),
    );
    add(
        &mut f,
        23,
        "MeshRenderer",
        10002,
        "  m_Materials: [{fileID: 10308, guid: 0000000000000000f000000000000000, type: 0}]\n  m_SortingLayerID: 0\n  m_SortingLayer: 0\n  m_SortingOrder: 7\n  m_CastShadows: 0\n  m_ReceiveShadows: 0\n  m_LightProbeUsage: 0\n  m_ReflectionProbeUsage: 0\n",
    );
    write(dir.path(), &f);
    let c = compiled(dir.path());
    let filter = c
        .document
        .components
        .iter()
        .flatten()
        .find_map(|v| {
            if let w::ComponentDataT::MeshFilter(f) = &v.data {
                Some(f)
            } else {
                None
            }
        })
        .unwrap();
    let geometry =
        MeshData::decode(&c.files[&format!("assets/{}.nomesh", filter.mesh_asset_id)]).unwrap();
    assert_eq!((geometry.positions.len(), geometry.indices.len()), (30, 54));
    assert_eq!(
        (
            geometry.normals.len(),
            geometry.tangents.len(),
            geometry.texcoords[0].dimension
        ),
        (30, 30, 2)
    );
    assert_eq!(
        document::decode(&c.files["scene.noir"]).unwrap(),
        c.document
    );
    let mut d = c.document.clone();
    d.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "mesh.nomesh");
    assert!(document::encode(&d).is_err());
    let mut d = c.document.clone();
    for c in d.components.iter_mut().flatten() {
        if let w::ComponentDataT::MeshRenderer(r) = &mut c.data {
            r.mesh_filter_component_id = 0;
        }
    }
    assert!(document::encode(&d).is_err());
}
#[test]
fn sprite_asset_shape_and_mask_use_actual_geometry_and_inline_resource_dependencies() {
    let dir = fixture();
    let mut f = read(dir.path());
    let root = repo().join("data/scenario/effect/still_tukasa_wnd12_01_rip");
    let sprite = walkdir::WalkDir::new(&root)
        .into_iter()
        .map(Result::unwrap)
        .find(|e| e.file_name() == "tex_still_tukasa_wnd12_01.asset")
        .unwrap()
        .path()
        .to_owned();
    let source = UnityFile::parse(&fs::read_to_string(&sprite).unwrap()).unwrap();
    let data = &source.objects.values().next().unwrap().data;
    let tg = data["m_RD"]["texture"]["guid"].as_str().unwrap();
    // Preserve the exported Sprite geometry/affine mapping, bind a fixture PNG
    // of the same 512x512 size (the original exported Sprite texture is missing).
    let guid = copy_asset(&sprite, dir.path());
    let image_meta: Y = serde_yaml_ng::from_str(
        &fs::read_to_string(dir.path().join("tex_kirakira_01.png.meta")).unwrap(),
    )
    .unwrap();
    let replacement = image_meta["guid"].as_str().unwrap();
    fs::write(
        dir.path().join(sprite.file_name().unwrap()),
        fs::read_to_string(&sprite)
            .unwrap()
            .replace(tg, replacement),
    )
    .unwrap();
    let reference = format!("{{fileID: 21300000, guid: {guid}, type: 2}}");
    for o in f.objects.values_mut().filter(|o| o.class_id == 198) {
        let s = &mut o.data["ShapeModule"];
        s["enabled"] = 1.into();
        s["type"] = 19.into();
        s["placementMode"] = 2.into();
        s["m_MeshNormalOffset"] = 0.25.into();
        s["m_Sprite"] = serde_yaml_ng::from_str(&reference).unwrap();
    }
    add(
        &mut f,
        212,
        "SpriteRenderer",
        10003,
        &format!(
            "  m_Sprite: {reference}\n  m_Materials: [{{fileID: 10754, guid: 0000000000000000f000000000000000, type: 0}}]\n  m_Color: {{r: 1, g: 0.5, b: 1, a: 0.4}}\n  m_FlipX: 1\n  m_FlipY: 0\n  m_DrawMode: 0\n  m_Size: {{x: 1, y: 1}}\n  m_SpriteTileMode: 0\n  m_AdaptiveModeThreshold: 0.5\n  m_MaskInteraction: 1\n  m_SortingLayerID: 0\n  m_SortingLayer: 0\n  m_SortingOrder: 0\n  m_SpriteSortPoint: 1\n"
        ),
    );
    write(dir.path(), &f);
    let c = compiled(dir.path());
    let a = c
        .document
        .assets
        .iter()
        .flatten()
        .find(|a| a.kind == w::AssetKind::Sprite)
        .unwrap();
    let s = a.sprite.as_ref().unwrap();
    let mesh = MeshData::decode(&c.files[&format!("assets/{}.nomesh", s.mesh_asset_id)]).unwrap();
    assert_eq!((mesh.positions.len(), mesh.indices.len()), (65, 189));
    let uv = &mesh
        .texcoords
        .iter()
        .find(|v| v.channel == 0)
        .unwrap()
        .values;
    assert!(uv.iter().any(|v| v[0] != 0.));
    for (p, uv) in mesh.positions.iter().zip(uv) {
        assert!((uv[0] - (p.x * 100. + 256.) / 512.).abs() < 1e-6);
        assert!((uv[1] - (p.y * 100. + 256.) / 512.).abs() < 1e-6);
    }
    assert!(
        !c.manifest()
            .unwrap()
            .files
            .iter()
            .any(|v| v.asset_id == Some(a.id))
    );
    assert_eq!(
        document::decode(&c.files["scene.noir"]).unwrap(),
        c.document
    );
    for component in c.document.components.iter().flatten() {
        if let w::ComponentDataT::ParticleSystem(p) = &component.data {
            for m in p.modules.iter().flatten() {
                if let w::ParticleModuleDataT::ShapeModule(s) = &m.data
                    && let w::ShapeGeometryT::SpriteShape(s) = &s.geometry
                {
                    assert_eq!(s.placement, w::GeometryPlacement::Triangle);
                    assert_eq!(s.normal_offset, 0.25);
                    assert_eq!(s.sprite_asset_id, a.id);
                }
            }
        }
    }
    let mut outside = c.document.clone();
    let pivot = outside
        .assets
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v.id == a.id)
        .unwrap()
        .sprite
        .as_mut()
        .unwrap()
        .pivot
        .as_mut()
        .unwrap();
    pivot.y = 1.2585158; // Present in the real still_miku_shl30_01 Sprite export.
    assert_eq!(
        document::decode(&document::encode(&outside).unwrap()).unwrap(),
        outside
    );
    let mut d = c.document.clone();
    d.assets
        .as_mut()
        .unwrap()
        .retain(|a| a.id != s.mesh_asset_id);
    assert!(document::encode(&d).is_err());
    let mut d = c.document.clone();
    d.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "render.sprite-mask");
    assert!(document::encode(&d).is_err());
}
#[test]
fn standard_ui_is_native_with_canvas_depth_and_masks_not_a_bridge_placeholder() {
    let dir = fixture();
    let mut f = read(dir.path());
    let node = f
        .objects
        .values()
        .find(|o| o.class_id == 1)
        .unwrap()
        .file_id;
    let transform = f
        .objects
        .values_mut()
        .find(|o| o.class_id == 4 && o.data["m_GameObject"]["fileID"].as_i64() == Some(node))
        .unwrap();
    transform.class_id = 224;
    transform.type_name = "RectTransform".into();
    for (key, text) in [
        ("m_AnchorMin", "{x: 0, y: 0}"),
        ("m_AnchorMax", "{x: 1, y: 1}"),
        ("m_Pivot", "{x: 0.5, y: 0.5}"),
        ("m_SizeDelta", "{x: 128, y: 64}"),
        ("m_AnchoredPosition", "{x: 0, y: 0}"),
    ] {
        transform.data[key] = serde_yaml_ng::from_str(text).unwrap();
    }
    let mut scripts = Vec::new();
    for (guid, class, id) in [
        ("11111111111111111111111111111111", "Image", 10007),
        ("22222222222222222222222222222222", "Mask", 10008),
        ("33333333333333333333333333333333", "RectMask2D", 10009),
    ] {
        fs::write(
            dir.path().join(format!("{class}.cs")),
            "// exported placeholder\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(format!("{class}.cs.meta")),
            format!("fileFormatVersion: 2\nguid: {guid}\n"),
        )
        .unwrap();
        scripts.push(serde_json::json!({"guid":guid,"file_id":"11500000","assembly":"UnityEngine.UI","namespace":"UnityEngine.UI","class_name":class}));
        let fields = match class {
            "Image" => {
                "  m_Material: {fileID: 0}\n  m_Sprite: {fileID: 0}\n  m_Color: {r: 0.25, g: 0.5, b: 1, a: 0.75}\n  m_Type: 3\n  m_PreserveAspect: 1\n  m_FillCenter: 1\n  m_FillMethod: 4\n  m_FillAmount: 0.5\n  m_FillClockwise: 1\n  m_FillOrigin: 2\n  m_UseSpriteMesh: 0\n  m_PixelsPerUnitMultiplier: 1\n  m_Maskable: 1\n"
            }
            "Mask" => "  m_ShowMaskGraphic: 0\n",
            _ => "  m_Padding: {x: 1, y: 2, z: 3, w: 4}\n  m_Softness: {x: 2, y: 3}\n",
        };
        add(
            &mut f,
            114,
            "MonoBehaviour",
            id,
            &format!("  m_Script: {{fileID: 11500000, guid: {guid}, type: 3}}\n{fields}"),
        );
    }
    fs::write(
        dir.path().join("ui.monoscripts.json"),
        serde_json::json!({"format":"unity-monoscript-metadata","version":1,"scripts":scripts})
            .to_string(),
    )
    .unwrap();
    add(
        &mut f,
        223,
        "Canvas",
        10004,
        "  m_RenderMode: 2\n  m_Camera: {fileID: 0}\n  m_PlaneDistance: 100\n  m_PixelPerfect: 0\n  m_OverridePixelPerfect: 0\n  m_OverrideSorting: 1\n  m_SortingLayerID: 0\n  m_SortingOrder: 7\n  m_AdditionalShaderChannelsFlag: 0\n  m_TargetDisplay: 0\n  m_VertexColorAlwaysGammaSpace: 1\n",
    );
    add(
        &mut f,
        222,
        "CanvasRenderer",
        10005,
        "  m_CullTransparentMesh: 1\n",
    );
    add(
        &mut f,
        225,
        "CanvasGroup",
        10006,
        "  m_Alpha: 0.5\n  m_IgnoreParentGroups: 1\n",
    );
    write(dir.path(), &f);
    let c = compiled(dir.path());
    let d = &c.document;
    assert_eq!(
        d.components
            .iter()
            .flatten()
            .filter(|v| matches!(
                v.data,
                w::ComponentDataT::UIImage(_)
                    | w::ComponentDataT::UIMask(_)
                    | w::ComponentDataT::RectMask2D(_)
            ))
            .count(),
        3
    );
    assert!(!d.components.iter().flatten().any(|v| {
        if let w::ComponentDataT::CustomComponent(s) = &v.data {
            s.script
                .as_ref()
                .is_some_and(|s| s.namespace == "UnityEngine.UI")
        } else {
            false
        }
    }));
    let material = d
        .materials
        .iter()
        .flatten()
        .find(|m| m.model == w::MaterialModel::UIDefault)
        .unwrap();
    assert_eq!(
        material.render_state.depth_test_source,
        w::DepthTestSource::Canvas
    );
    assert_eq!(material.render_state.src_rgb, w::BlendFactor::One);
    assert_eq!(document::decode(&c.files["scene.noir"]).unwrap(), *d);
    let mut d = d.clone();
    d.components
        .as_mut()
        .unwrap()
        .retain(|v| !matches!(v.data, w::ComponentDataT::CanvasRenderer(_)));
    assert!(document::encode(&d).is_err());
}

#[test]
fn verified_additive_program_uses_shader_state_instead_of_stale_saved_blend_fields() {
    let root = repo().join("data/scenario/effect/aura_02_rip");
    let prefab = walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .find(|e| e.file_name() == "aura_02.prefab")
        .unwrap()
        .path()
        .to_owned();
    let c = compile(&prefab, &InspectOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let m = c
        .document
        .materials
        .iter()
        .flatten()
        .find(|m| m.model == w::MaterialModel::TextureVertexColor)
        .unwrap();
    // The source material's _SrcBlend=One is an unused Standard shader leftover;
    // this program's actual ShaderLab pass uses SrcAlpha, One, ColorMask RGB.
    assert_eq!(m.render_state.src_rgb, w::BlendFactor::SrcAlpha);
    assert_eq!(m.render_state.dst_rgb, w::BlendFactor::One);
    assert_eq!(m.render_state.color_write_mask, 7);
    assert_eq!(m.render_state.depth_test, w::CompareOp::LessEqual);
    assert_eq!(
        m.render_state.stencil.as_ref().unwrap().front.compare,
        w::CompareOp::Always
    );
    let recipe: serde_json::Value =
        serde_json::from_slice(&c.files[&format!("assets/{}.shader.json", m.shader_data_asset_id)])
            .unwrap();
    assert_eq!(
        recipe["program_sha256"],
        "f07136c5ab507bfcb309c1920a6afce6f1d5653f80c1c2f97e8ec0066ba58ac2"
    );
    assert_eq!(recipe["rgb_premultiply"], false);
    let mut d = c.document;
    d.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "material.texture-vertex-color");
    assert!(document::encode(&d).is_err());
}

#[test]
fn renderer_none_keeps_simulation_and_requires_explicit_capability() {
    let dir = fixture();
    let mut f = read(dir.path());
    let r = f.objects.values_mut().find(|o| o.class_id == 199).unwrap();
    r.data["m_RenderMode"] = 5.into();
    r.data["m_Materials"] = Y::Sequence(vec![]);
    r.data["m_ReceiveShadows"] = 1.into();
    write(dir.path(), &f);
    let c = compiled(dir.path());
    assert!(c.document.components.iter().flatten().any(|c|matches!(&c.data,w::ComponentDataT::ParticleRenderer(r) if r.mode==w::RenderMode::None&&r.materials.is_empty())));
    assert!(
        c.document
            .features
            .iter()
            .flatten()
            .any(|f| f.name == "render.environment" && f.required)
    );
    let mut d = c.document;
    d.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "particle.renderer-none");
    assert!(document::encode(&d).is_err());
}
