use noa_compiler::{InspectOptions, compile, unity::UnityFile};
use noa_format::{document, wire as w};
use std::path::{Path, PathBuf};

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/scenario/effect/kirakira_01_rip/kirakira_01.prefab")
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
fn write_shapes(destination: &Path, kind: i64) {
    let mut file = UnityFile::parse(&std::fs::read_to_string(sample()).unwrap()).unwrap();
    let mut text = String::new();
    for object in file.objects.values_mut() {
        if object.class_id == 198 {
            let s = &mut object.data["ShapeModule"];
            s["type"] = kind.into();
            s["radius"]["value"] = 2.25.into();
            s["radius"]["mode"] = 1.into();
            s["radius"]["spread"] = 0.2.into();
            s["radius"]["speed"]["minMaxState"] = 0.into();
            s["radius"]["speed"]["scalar"] = 2.5.into();
            s["arc"]["value"] = 240.into();
            s["arc"]["mode"] = 2.into();
            s["arc"]["spread"] = 0.1.into();
            s["arc"]["speed"]["minMaxState"] = 0.into();
            s["arc"]["speed"]["scalar"] = 0.75.into();
            s["radiusThickness"] = 0.25.into();
            s["donutRadius"] = 0.31.into();
            s["angle"] = 35.into();
            s["length"] = 4.into();
            s["boxThickness"] = serde_yaml_ng::from_str("{x: 0.1, y: 0.2, z: 0.3}").unwrap();
            s["m_Scale"] = serde_yaml_ng::from_str("{x: -2, y: 3, z: 0}").unwrap();
            s["m_Rotation"] = serde_yaml_ng::from_str("{x: 0, y: 0, z: 90}").unwrap();
        }
        let mut root = serde_yaml_ng::Mapping::new();
        root.insert(object.type_name.clone().into(), object.data.clone());
        text.push_str(&format!("--- !u!{} &{}\n", object.class_id, object.file_id));
        text.push_str(&serde_yaml_ng::to_string(&root).unwrap());
    }
    std::fs::write(destination.join("kirakira_01.prefab"), text).unwrap();
}
fn geometries(d: &w::DocumentT) -> impl Iterator<Item = &w::ShapeModuleT> {
    d.components
        .iter()
        .flatten()
        .filter_map(|c| {
            if let w::ComponentDataT::ParticleSystem(p) = &c.data {
                Some(p)
            } else {
                None
            }
        })
        .flat_map(|p| p.modules.iter().flatten())
        .filter_map(|m| {
            if let w::ParticleModuleDataT::ShapeModule(s) = &m.data {
                Some(s.as_ref())
            } else {
                None
            }
        })
}
fn first_shape(d: &mut w::DocumentT) -> &mut w::ShapeModuleT {
    d.components
        .iter_mut()
        .flatten()
        .find_map(|c| {
            if let w::ComponentDataT::ParticleSystem(p) = &mut c.data {
                p.modules.iter_mut().flatten().find_map(|m| {
                    if let w::ParticleModuleDataT::ShapeModule(s) = &mut m.data {
                        Some(s.as_mut())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}");
}

#[test]
fn every_t1_shape_roundtrips_with_its_emission_region_and_required_capability() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    for (kind, capability) in [
        (0, "sphere"),
        (2, "hemisphere"),
        (5, "box"),
        (15, "box"),
        (16, "box"),
        (4, "cone"),
        (8, "cone-volume"),
        (10, "circle"),
        (12, "edge"),
    ] {
        write_shapes(dir.path(), kind);
        let c = compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default(),
        )
        .unwrap();
        let d = document::decode(&c.files["scene.noir"]).unwrap();
        assert_eq!(d, c.document);
        let feature = format!("particle.shape.{capability}");
        assert!(
            d.features
                .iter()
                .flatten()
                .any(|f| f.name == feature && f.required && f.revision == 1)
        );
        assert_eq!(geometries(&d).count(), 15);
        for s in geometries(&d) {
            let scale = s.transform.scale.as_ref().unwrap();
            assert_eq!((scale.x, scale.y, scale.z), (-2., 3., 0.));
            let q = s.transform.rotation.as_ref().unwrap();
            near(q.z, std::f32::consts::FRAC_1_SQRT_2);
            near(q.w, std::f32::consts::FRAC_1_SQRT_2);
            match &s.geometry {
                w::ShapeGeometryT::SphereShape(s) => {
                    assert_eq!(s.hemisphere, kind == 2);
                    assert_eq!(s.radius.as_ref().unwrap().constant_max, 2.25);
                    assert_eq!(s.radius_thickness, 0.25);
                }
                w::ShapeGeometryT::BoxShape(b) => {
                    assert_eq!(
                        b.region,
                        match kind {
                            5 => w::BoxRegion::Volume,
                            15 => w::BoxRegion::Shell,
                            _ => w::BoxRegion::Edge,
                        }
                    );
                    let t = b.thickness.as_ref().unwrap();
                    assert_eq!((t.x, t.y, t.z), (0.1, 0.2, 0.3));
                }
                w::ShapeGeometryT::ConeShape(c) => {
                    assert_eq!(
                        c.region,
                        if kind == 4 {
                            w::ConeRegion::Base
                        } else {
                            w::ConeRegion::Volume
                        }
                    );
                    assert_eq!(c.radius.as_ref().unwrap().constant_max, 2.25);
                    assert_eq!(c.radius_thickness, 0.25);
                    near(c.angle_radians, 35_f32.to_radians());
                    near(c.arc_radians, 240_f32.to_radians());
                    assert_eq!(c.length, 4.);
                    assert_eq!(c.radius_mode, w::ShapeSweep::Loop);
                    assert_eq!(c.arc_mode, w::ShapeSweep::PingPong);
                    assert_eq!(c.radius_speed.as_ref().unwrap().constant_max, 2.5);
                    assert_eq!(c.arc_speed.as_ref().unwrap().constant_max, 0.75);
                }
                w::ShapeGeometryT::CircleShape(c) => {
                    near(c.arc_radians, 240_f32.to_radians());
                    assert_eq!(c.radius.as_ref().unwrap().constant_max, 2.25);
                }
                w::ShapeGeometryT::EdgeShape(e) => {
                    assert_eq!(e.half_length.as_ref().unwrap().constant_max, 2.25);
                    assert_eq!(e.mode, w::ShapeSweep::Loop);
                    assert_eq!(e.speed.as_ref().unwrap().constant_max, 2.5);
                    assert_eq!(e.spread, 0.2);
                }
                _ => panic!("wrong geometry for Unity shape {kind}"),
            }
        }
        if kind != 10 {
            let mut bad = d.clone();
            bad.features.as_mut().unwrap().retain(|f| f.name != feature);
            assert!(document::encode(&bad).unwrap_err().contains(&feature));
            let mut bad = d.clone();
            bad.features
                .as_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f.name == feature)
                .unwrap()
                .required = false;
            assert!(document::encode(&bad).is_err());
        }
    }
}

#[test]
fn bad_shape_geometry_and_unimplemented_shapes_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    for kind in [0, 2, 4, 5, 8, 12, 15, 16] {
        write_shapes(dir.path(), kind);
        let c = compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default(),
        )
        .unwrap();
        let mut d = c.document.clone();
        let s = first_shape(&mut d);
        match &mut s.geometry {
            w::ShapeGeometryT::SphereShape(s) => s.radius_thickness = 1.1,
            w::ShapeGeometryT::ConeShape(c) => c.angle_radians = 2.,
            w::ShapeGeometryT::BoxShape(b) => b.thickness.as_mut().unwrap().x = -0.1,
            w::ShapeGeometryT::EdgeShape(e) => e.half_length.as_mut().unwrap().constant_max = -1.,
            _ => unreachable!(),
        }
        assert!(document::encode(&d).is_err());
        let mut d = c.document;
        first_shape(&mut d).transform.scale = None;
        assert!(document::encode(&d).is_err());
    }
    for kind in [1, 3, 6, 7, 9, 11, 13, 14, 18, 20, 999] {
        write_shapes(dir.path(), kind);
        let err = compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default(),
        )
        .unwrap_err();
        assert!(err.message.contains("ShapeModule.type"), "{err}");
    }
}

#[test]
fn cone_and_edge_speed_curves_require_unity_infinity_and_reject_bad_keys() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    for kind in [4, 8, 12] {
        write_shapes(dir.path(), kind);
        let c = compile(
            &dir.path().join("kirakira_01.prefab"),
            &InspectOptions::default(),
        )
        .unwrap();
        let mut d = c.document;
        let s = first_shape(&mut d);
        let p = match &mut s.geometry {
            w::ShapeGeometryT::ConeShape(c) => c.arc_speed.as_mut().unwrap(),
            w::ShapeGeometryT::EdgeShape(e) => e.speed.as_mut().unwrap(),
            _ => unreachable!(),
        };
        p.mode = w::ScalarMode::Curve;
        let mut curve = w::ScalarCurveT::default();
        curve.pre_wrap = w::WrapMode::UnitySerialized;
        let k0 = w::FloatKeyT::default();
        let mut k1 = k0.clone();
        k1.time = 1.;
        curve.keys = vec![k0, k1];
        p.curve_max = Some(Box::new(curve));
        d.features
            .as_mut()
            .unwrap()
            .retain(|f| f.name != "curve.unity-infinity");
        assert!(document::requires_unity_infinity(&d));
        assert!(
            document::encode(&d)
                .unwrap_err()
                .contains("curve.unity-infinity")
        );
        let mut f = w::FeatureRequirementT::default();
        f.name = "curve.unity-infinity".into();
        f.revision = 1;
        f.required = true;
        d.features.as_mut().unwrap().push(f);
        document::encode(&d).unwrap();
        let s = first_shape(&mut d);
        let p = match &mut s.geometry {
            w::ShapeGeometryT::ConeShape(c) => c.arc_speed.as_mut().unwrap(),
            w::ShapeGeometryT::EdgeShape(e) => e.speed.as_mut().unwrap(),
            _ => unreachable!(),
        };
        p.curve_max.as_mut().unwrap().keys[1].time = 0.;
        assert!(
            document::encode(&d)
                .unwrap_err()
                .contains("curve key times")
        );
    }
}

#[test]
fn donut_preserves_torus_geometry_and_arc_sweep_without_circle_substitution() {
    let dir = tempfile::tempdir().unwrap();
    copy_sample(dir.path());
    write_shapes(dir.path(), 17);
    let c = compile(
        &dir.path().join("kirakira_01.prefab"),
        &InspectOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for s in geometries(&c.document) {
        let w::ShapeGeometryT::DonutShape(d) = &s.geometry else {
            panic!("expected torus")
        };
        near(d.radius, 2.25);
        near(d.tube_radius, 0.31);
        near(d.radius_thickness, 0.25);
        near(d.arc_radians, 240f32.to_radians());
        assert_eq!(d.arc_mode, w::ShapeSweep::PingPong);
        near(d.arc_spread, 0.1);
    }
    assert!(
        c.document
            .features
            .iter()
            .flatten()
            .any(|f| f.name == "particle.shape.donut" && f.required)
    );
    assert_eq!(
        document::decode(&c.files["scene.noir"]).unwrap(),
        c.document
    );
    let mut d = c.document.clone();
    if let w::ShapeGeometryT::DonutShape(s) = &mut first_shape(&mut d).geometry {
        s.tube_radius = -1.;
    }
    assert!(document::encode(&d).is_err());
}
