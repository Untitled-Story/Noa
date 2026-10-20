use noa_compiler::{InspectOptions, compile, unity::UnityFile};
use noa_format::{document, wire as w};
use std::path::Path;

#[test]
fn by_speed_preserves_axes_ranges_curves_and_required_capabilities() {
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
    let prefab = dir.path().join("kirakira_01.prefab");
    let mut file = UnityFile::parse(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    let mut text = String::new();
    for o in file.objects.values_mut() {
        if o.class_id == 198 {
            for name in ["SizeBySpeedModule", "RotationBySpeedModule"] {
                o.data[name]["enabled"] = 1.into();
                o.data[name]["separateAxes"] = 1.into();
                o.data[name]["range"] = serde_yaml_ng::from_str("{x: 2, y: 9}").unwrap();
                let axes = if name == "SizeBySpeedModule" {
                    ["curve", "y", "z"]
                } else {
                    ["x", "y", "curve"]
                };
                for (i, axis) in axes.iter().enumerate() {
                    o.data[name][*axis] = o.data["InitialModule"]["startLifetime"].clone();
                    o.data[name][*axis]["scalar"] = ((i + 1) as i64).into();
                }
            }
        }
        let mut mapping = serde_yaml_ng::Mapping::new();
        mapping.insert(o.type_name.clone().into(), o.data.clone());
        text.push_str(&format!("--- !u!{} &{}\n", o.class_id, o.file_id));
        text.push_str(&serde_yaml_ng::to_string(&mapping).unwrap());
    }
    std::fs::write(&prefab, text).unwrap();
    let c = compile(&prefab, &InspectOptions::default()).unwrap();
    let mut d = document::decode(&document::encode(&c.document).unwrap()).unwrap();
    for component in d.components.as_ref().unwrap() {
        if let w::ComponentDataT::ParticleSystem(p) = &component.data {
            for m in p.modules.iter().flatten() {
                let (v, r) = match &m.data {
                    w::ParticleModuleDataT::SizeBySpeedModule(s) => {
                        (&s.size, s.speed_range.as_ref().unwrap())
                    }
                    w::ParticleModuleDataT::RotationBySpeedModule(s) => {
                        (&s.angular_velocity, s.speed_range.as_ref().unwrap())
                    }
                    _ => continue,
                };
                assert_eq!((r.x, r.y), (2., 9.));
                assert_eq!(v.x.as_ref().unwrap().constant_max, 1.);
                assert_eq!(v.y.as_ref().unwrap().constant_max, 2.);
                assert_eq!(v.z.as_ref().unwrap().constant_max, 3.);
            }
        }
    }
    d.features
        .as_mut()
        .unwrap()
        .retain(|f| f.name != "particle.size-by-speed");
    assert!(document::encode(&d).unwrap_err().contains("size-by-speed"));
    d.features = c.document.features.clone();
    let system = d
        .components
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|c| match &mut c.data {
            w::ComponentDataT::ParticleSystem(p) => Some(p),
            _ => None,
        })
        .unwrap();
    let size = system
        .modules
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|m| match &mut m.data {
            w::ParticleModuleDataT::SizeBySpeedModule(s) => Some(s),
            _ => None,
        })
        .unwrap();
    let x = size.size.x.as_mut().unwrap();
    x.mode = w::ScalarMode::Curve;
    let mut curve = w::ScalarCurveT::default();
    curve.keys = vec![w::FloatKeyT::default(), w::FloatKeyT::default()];
    x.curve_max = Some(Box::new(curve));
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("curve key times")
    );
    let system = d
        .components
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|c| match &mut c.data {
            w::ComponentDataT::ParticleSystem(p) => Some(p),
            _ => None,
        })
        .unwrap();
    let size = system
        .modules
        .as_mut()
        .unwrap()
        .iter_mut()
        .find_map(|m| match &mut m.data {
            w::ParticleModuleDataT::SizeBySpeedModule(s) => Some(s),
            _ => None,
        })
        .unwrap();
    size.size.x.as_mut().unwrap().mode = w::ScalarMode::Constant;
    size.speed_range.as_mut().unwrap().y = 1.;
    assert!(document::encode(&d).unwrap_err().contains("speed range"));
}
