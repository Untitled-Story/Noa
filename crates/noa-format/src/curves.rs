//! Typed traversal of active curve branches, shared by validation and capability
//! discovery. Serialized script fields never participate in this traversal.
use crate::wire as w;

pub(crate) fn visit(document: &w::DocumentT, mut visitor: impl FnMut(&w::ScalarCurveT)) {
    for component in document.components.iter().flatten() {
        if let w::ComponentDataT::ParticleSystem(p) = &component.data {
            particles(p, &mut visitor);
        }
    }
    for animation in document.animations.iter().flatten() {
        for track in animation.tracks.iter().flatten() {
            match &track.data {
                w::TrackDataT::FloatTrack(t) => visitor(&t.curve),
                w::TrackDataT::Vector3Track(t) => {
                    for c in [&t.x, &t.y, &t.z].into_iter().flatten() {
                        visitor(c);
                    }
                }
                w::TrackDataT::RotationTrack(t) => {
                    for c in [&t.x, &t.y, &t.z, &t.w].into_iter().flatten() {
                        visitor(c);
                    }
                }
                w::TrackDataT::ColorTrack(t) => {
                    for c in [&t.r, &t.g, &t.b, &t.a].into_iter().flatten() {
                        visitor(c);
                    }
                }
                w::TrackDataT::BoolTrack(_)
                | w::TrackDataT::ReferenceTrack(_)
                | w::TrackDataT::NONE => {}
            }
        }
    }
}

fn scalar(p: &w::ScalarParameterT, visitor: &mut impl FnMut(&w::ScalarCurveT)) {
    if matches!(p.mode, w::ScalarMode::Curve | w::ScalarMode::TwoCurves) {
        if let Some(c) = &p.curve_max {
            visitor(c);
        }
        if p.mode == w::ScalarMode::TwoCurves
            && let Some(c) = &p.curve_min
        {
            visitor(c);
        }
    }
}

fn vector(v: &w::VectorParameterT, visitor: &mut impl FnMut(&w::ScalarCurveT)) {
    for p in [&v.x, &v.y, &v.z].into_iter().flatten() {
        scalar(p, visitor);
    }
}

fn particles(p: &w::ParticleSystemT, visitor: &mut impl FnMut(&w::ScalarCurveT)) {
    let m = &p.main;
    for p in [&m.lifetime, &m.speed] {
        scalar(p, visitor);
    }
    for p in [&m.start_delay, &m.gravity_multiplier]
        .into_iter()
        .flatten()
    {
        scalar(p, visitor);
    }
    vector(&m.size, visitor);
    vector(&m.rotation, visitor);
    for module in p.modules.iter().flatten() {
        match &module.data {
            w::ParticleModuleDataT::EmissionModule(e) => {
                for p in [&e.rate_over_time, &e.rate_over_distance]
                    .into_iter()
                    .flatten()
                {
                    scalar(p, visitor);
                }
                for b in e.bursts.iter().flatten() {
                    scalar(&b.count, visitor);
                }
            }
            w::ParticleModuleDataT::ShapeModule(s) => match &s.geometry {
                w::ShapeGeometryT::CircleShape(c) => {
                    for p in [&c.radius, &c.radius_speed, &c.arc_speed]
                        .into_iter()
                        .flatten()
                    {
                        scalar(p, visitor);
                    }
                }
                w::ShapeGeometryT::SphereShape(s) => {
                    if let Some(p) = &s.radius {
                        scalar(p, visitor);
                    }
                }
                w::ShapeGeometryT::ConeShape(c) => {
                    for p in [&c.radius, &c.radius_speed, &c.arc_speed]
                        .into_iter()
                        .flatten()
                    {
                        scalar(p, visitor);
                    }
                }
                w::ShapeGeometryT::EdgeShape(e) => {
                    for p in [&e.half_length, &e.speed].into_iter().flatten() {
                        scalar(p, visitor);
                    }
                }
                w::ShapeGeometryT::BoxShape(_) | w::ShapeGeometryT::NONE => {}
            },
            w::ParticleModuleDataT::SizeModule(s) => vector(&s.size, visitor),
            w::ParticleModuleDataT::SizeBySpeedModule(s) => vector(&s.size, visitor),
            w::ParticleModuleDataT::RotationBySpeedModule(r) => {
                vector(&r.angular_velocity, visitor)
            }
            w::ParticleModuleDataT::RotationModule(r) => vector(&r.angular_velocity, visitor),
            w::ParticleModuleDataT::VelocityModule(v) => {
                for v in [&v.linear, &v.orbital, &v.orbital_offset]
                    .into_iter()
                    .flatten()
                {
                    vector(v, visitor);
                }
                for p in [&v.radial, &v.speed_multiplier].into_iter().flatten() {
                    scalar(p, visitor);
                }
            }
            w::ParticleModuleDataT::LimitVelocityModule(l) => {
                vector(&l.limit, visitor);
                if let Some(p) = &l.drag {
                    scalar(p, visitor);
                }
            }
            w::ParticleModuleDataT::NoiseModule(n) => {
                vector(&n.strength, visitor);
                for p in [
                    &n.scroll_speed,
                    &n.position_amount,
                    &n.rotation_amount,
                    &n.size_amount,
                ]
                .into_iter()
                .flatten()
                {
                    scalar(p, visitor);
                }
                if n.remap_enabled
                    && let Some(v) = &n.remap
                {
                    vector(v, visitor);
                }
            }
            w::ParticleModuleDataT::TextureSheetModule(t) => {
                for p in [&t.frame_over_time, &t.start_frame].into_iter().flatten() {
                    scalar(p, visitor);
                }
            }
            w::ParticleModuleDataT::CustomDataModule(c) => {
                for s in &c.streams {
                    if s.mode == w::CustomDataMode::Vector {
                        for p in s.vector.iter().flatten() {
                            scalar(p, visitor);
                        }
                    }
                }
            }
            w::ParticleModuleDataT::ColorModule(_) | w::ParticleModuleDataT::NONE => {}
        }
    }
}
