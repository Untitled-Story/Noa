use noa_format::{document, wire as w};

fn curve(unity: bool) -> w::ScalarCurveT {
    let mut c = w::ScalarCurveT::default();
    c.pre_wrap = if unity {
        w::WrapMode::UnitySerialized
    } else {
        w::WrapMode::Clamp
    };
    c.source_pre_infinity = 2;
    for time in [0., 1.] {
        c.keys.push(w::FloatKeyT {
            time,
            value: time,
            ..Default::default()
        });
    }
    c
}

fn parameter() -> w::ScalarParameterT {
    let mut p = w::ScalarParameterT::default();
    p.mode = w::ScalarMode::Curve;
    p.curve_max = Some(Box::new(curve(true)));
    p
}

fn vector(p: w::ScalarParameterT) -> w::VectorParameterT {
    let mut v = w::VectorParameterT::default();
    v.x = Some(Box::new(p));
    v.y = Some(Box::default());
    v.z = Some(Box::default());
    v
}

fn base() -> w::DocumentT {
    let mut d = w::DocumentT::default();
    d.format_major = 2;
    let mut node = w::NodeT::default();
    node.id = 1;
    node.transform.position = Some(w::Vec3T::default());
    node.transform.rotation = Some(w::QuaternionT::default());
    node.transform.scale = Some(w::Vec3T::default());
    d.nodes.push(node);
    let mut entry = w::EntryPointT::default();
    entry.id = 2;
    entry.name = "test".into();
    entry.roots = vec![1];
    entry.host_camera_slot = Some("camera".into());
    d.entries.push(entry);
    d.default_entry_id = 2;
    let mut color = w::ColorContextT::default();
    color.project_color_space = w::ColorSpace::Linear;
    d.color_context = Some(Box::new(color));
    d
}

fn particle(main: w::ScalarParameterT) -> w::ParticleSystemT {
    let mut p = w::ParticleSystemT::default();
    p.main.lifetime = Box::new(main);
    p.main.size = Box::new(vector(w::ScalarParameterT::default()));
    p.main.rotation = Box::new(vector(w::ScalarParameterT::default()));
    p.main.color.color_max = Some(w::ColorT::default());
    p
}

fn with_particle(p: w::ParticleSystemT) -> w::DocumentT {
    let mut d = base();
    let mut c = w::ComponentT::default();
    c.id = 3;
    c.node_id = 1;
    c.data = w::ComponentDataT::ParticleSystem(Box::new(p));
    d.components = Some(vec![c]);
    d
}

fn require(d: &mut w::DocumentT) {
    let mut f = w::FeatureRequirementT::default();
    f.name = "curve.unity-infinity".into();
    d.features = Some(vec![f]);
}

#[test]
fn constants_and_unused_minimum_curves_do_not_require_unity() {
    let mut p = parameter();
    p.curve_min = Some(Box::new(curve(true)));
    p.curve_max = Some(Box::new(curve(false)));
    let d = with_particle(particle(p.clone()));
    assert!(!document::requires_unity_infinity(&d));
    document::encode(&d).unwrap();
    let mut post = p.clone();
    post.curve_max.as_mut().unwrap().post_wrap = w::WrapMode::UnitySerialized;
    assert!(document::requires_unity_infinity(&with_particle(particle(
        post
    ))));
    p.mode = w::ScalarMode::TwoCurves;
    assert!(document::requires_unity_infinity(&with_particle(particle(
        p.clone()
    ))));
    p.mode = w::ScalarMode::Constant;
    p.curve_max = Some(Box::new(curve(true)));
    let d = with_particle(particle(p));
    assert!(!document::requires_unity_infinity(&d));
    document::encode(&d).unwrap();
}

#[test]
fn encoder_and_reader_reject_missing_optional_or_wrong_revision_requirement() {
    let mut d = with_particle(particle(parameter()));
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("curve.unity-infinity")
    );
    // A structurally valid buffer built without the checked encoder must still fail.
    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let root = d.pack(&mut builder);
    w::finish_document_buffer(&mut builder, root);
    assert!(
        document::decode(builder.finished_data())
            .unwrap_err()
            .contains("curve.unity-infinity")
    );
    require(&mut d);
    assert_eq!(document::decode(&document::encode(&d).unwrap()).unwrap(), d);
    d.features.as_mut().unwrap()[0].required = false;
    assert!(document::encode(&d).is_err());
    d.features.as_mut().unwrap()[0].required = true;
    d.features.as_mut().unwrap()[0].revision = 2;
    assert!(document::encode(&d).is_err());
}

#[test]
fn shape_speed_and_animation_curves_cannot_bypass_curve_validation() {
    let mut shape = w::ShapeModuleT::default();
    let mut circle = w::CircleShapeT::default();
    let mut speed = parameter();
    speed.curve_max.as_mut().unwrap().keys[1].time = 0.;
    circle.arc_speed = Some(Box::new(speed));
    shape.geometry = w::ShapeGeometryT::CircleShape(Box::new(circle));
    let mut module = w::ParticleModuleT::default();
    module.data = w::ParticleModuleDataT::ShapeModule(Box::new(shape));
    let mut p = particle(w::ScalarParameterT::default());
    p.modules = Some(vec![module]);
    let mut d = with_particle(p);
    require(&mut d);
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("curve key times")
    );

    let w::ComponentDataT::ParticleSystem(p) = &mut d.components.as_mut().unwrap()[0].data else {
        unreachable!()
    };
    let w::ParticleModuleDataT::ShapeModule(s) = &mut p.modules.as_mut().unwrap()[0].data else {
        unreachable!()
    };
    let w::ShapeGeometryT::CircleShape(c) = &mut s.geometry else {
        unreachable!()
    };
    c.arc_speed.as_mut().unwrap().curve_max = None;
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("missing maximum curve")
    );

    let mut d = base();
    let mut t = w::Vector3TrackT::default();
    t.x = Some(Box::new(curve(true)));
    let mut track = w::TrackT::default();
    track.binding.target_id = 1;
    track.binding.property = w::AnimationProperty::NodePosition;
    let mut feature = w::FeatureRequirementT::default();
    feature.name = "animation.transform".into();
    d.features = Some(vec![feature]);
    track.data = w::TrackDataT::Vector3Track(Box::new(t));
    let mut animation = w::AnimationT::default();
    animation.id = 4;
    animation.tracks = Some(vec![track]);
    d.animations = Some(vec![animation]);
    assert!(document::requires_unity_infinity(&d));
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("curve.unity-infinity")
    );
    require(&mut d);
    let mut feature = w::FeatureRequirementT::default();
    feature.name = "animation.transform".into();
    d.features.as_mut().unwrap().push(feature);
    document::encode(&d).unwrap();
    let w::TrackDataT::Vector3Track(t) =
        &mut d.animations.as_mut().unwrap()[0].tracks.as_mut().unwrap()[0].data
    else {
        unreachable!()
    };
    t.x.as_mut().unwrap().keys[1].time = 0.;
    assert!(
        document::encode(&d)
            .unwrap_err()
            .contains("curve key times")
    );
}

#[test]
fn custom_streams_and_noise_remap_follow_their_enable_modes() {
    let mut s = w::CustomDataStreamT::default();
    s.vector_components = 1;
    s.vector = Some(vec![parameter()]);
    let mut custom = w::CustomDataModuleT::default();
    custom.streams = vec![s.clone()];
    let mut module = w::ParticleModuleT::default();
    module.data = w::ParticleModuleDataT::CustomDataModule(Box::new(custom.clone()));
    let mut p = particle(w::ScalarParameterT::default());
    p.modules = Some(vec![module.clone()]);
    assert!(!document::requires_unity_infinity(&with_particle(
        p.clone()
    )));
    custom.streams[0].mode = w::CustomDataMode::Vector;
    module.data = w::ParticleModuleDataT::CustomDataModule(Box::new(custom));
    p.modules = Some(vec![module.clone()]);
    assert!(document::requires_unity_infinity(&with_particle(p.clone())));

    let mut noise = w::NoiseModuleT::default();
    noise.remap = Some(Box::new(vector(parameter())));
    module.data = w::ParticleModuleDataT::NoiseModule(Box::new(noise.clone()));
    p.modules = Some(vec![module.clone()]);
    assert!(!document::requires_unity_infinity(&with_particle(
        p.clone()
    )));
    noise.remap_enabled = true;
    module.data = w::ParticleModuleDataT::NoiseModule(Box::new(noise));
    p.modules = Some(vec![module]);
    assert!(document::requires_unity_infinity(&with_particle(p)));
}
