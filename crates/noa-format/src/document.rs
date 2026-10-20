//! Checked NOIR encoding. Native objects are not the FlatBuffers JSON dialect.
use crate::wire;
use std::collections::{BTreeMap, BTreeSet};

pub fn encode(document: &wire::DocumentT) -> Result<Vec<u8>, String> {
    validate(document)?;
    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let root = document.pack(&mut builder);
    wire::finish_document_buffer(&mut builder, root);
    Ok(builder.finished_data().to_vec())
}

pub fn decode(bytes: &[u8]) -> Result<wire::DocumentT, String> {
    if bytes.len() < 8 || !wire::document_buffer_has_identifier(bytes) {
        return Err("expected NOIR identifier".into());
    }
    // A serialized value can span several FlatBuffers tables per nesting level.
    let options = flatbuffers::VerifierOptions {
        max_depth: 1024,
        ..Default::default()
    };
    let doc = wire::root_as_document_with_opts(&options, bytes)
        .map_err(|e| e.to_string())?
        .unpack();
    validate(&doc)?;
    Ok(doc)
}

/// Whether an active curve branch needs the preserved Unity infinity semantics.
/// Constant parameters and idle minimum curves do not require that capability.
pub fn requires_unity_infinity(document: &wire::DocumentT) -> bool {
    let mut required = false;
    crate::curves::visit(document, |c| {
        required |= c.pre_wrap == wire::WrapMode::UnitySerialized
            || c.post_wrap == wire::WrapMode::UnitySerialized;
    });
    required
}

pub fn validate(d: &wire::DocumentT) -> Result<(), String> {
    if d.format_major != 2 || d.format_minor != 0 || d.semantics_revision != 1 {
        return Err("unsupported NOIR version".into());
    }
    let mut ids = BTreeMap::new();
    let mut add = |id, kind| -> Result<(), String> {
        if id == 0 || ids.insert(id, kind).is_some() {
            return Err(format!("duplicate/zero entity ID {id}"));
        }
        Ok(())
    };
    for n in &d.nodes {
        add(n.id, "node")?;
    }
    for c in d.components.iter().flatten() {
        add(c.id, "component")?;
    }
    for a in d.assets.iter().flatten() {
        add(a.id, "asset")?;
    }
    for m in d.materials.iter().flatten() {
        add(m.id, "material")?;
    }
    for a in d.animations.iter().flatten() {
        add(a.id, "animation")?;
    }
    for e in &d.entries {
        add(e.id, "entry")?;
    }
    for c in d.components.iter().flatten() {
        if let wire::ComponentDataT::Animator(a) = &c.data {
            for l in &a.layers {
                for s in &l.states {
                    add(s.id, "state")?;
                }
            }
        }
    }
    let check = |id, kind| -> Result<(), String> {
        if ids.get(&id) != Some(&kind) {
            Err(format!("invalid {kind} reference {id}"))
        } else {
            Ok(())
        }
    };
    if d.nodes.is_empty() || d.entries.is_empty() {
        return Err("document needs nodes and entries".into());
    }
    check(d.default_entry_id, "entry")?;
    for n in &d.nodes {
        if n.parent_id != 0 {
            check(n.parent_id, "node")?;
        }
        let mut visited = BTreeSet::new();
        let mut next = n.id;
        while next != 0 {
            if !visited.insert(next) {
                return Err("node hierarchy cycle".into());
            }
            next = d
                .nodes
                .iter()
                .find(|n| n.id == next)
                .ok_or("missing parent")?
                .parent_id;
        }
        if n.transform.scale.is_none()
            || n.transform.rotation.is_none()
            || n.transform.position.is_none()
        {
            return Err("incomplete node transform".into());
        }
    }
    for c in d.components.iter().flatten() {
        check(c.node_id, "node")?;
        match &c.data {
            wire::ComponentDataT::NONE => return Err("missing component payload".into()),
            wire::ComponentDataT::Camera(camera) => {
                if camera.near_plane <= 0. || camera.far_plane <= camera.near_plane {
                    return Err("invalid camera clipping planes".into());
                }
                match camera.projection {
                    wire::Projection::Perspective
                        if !(0. ..std::f32::consts::PI).contains(&camera.fov_radians)
                            || camera.fov_radians == 0. =>
                    {
                        return Err("invalid camera FOV".into());
                    }
                    wire::Projection::Orthographic if camera.ortho_half_height <= 0. => {
                        return Err("invalid orthographic size".into());
                    }
                    wire::Projection::Custom
                        if camera
                            .custom_projection
                            .as_ref()
                            .is_none_or(|m| m.values.len() != 16) =>
                    {
                        return Err("custom projection needs 16 elements".into());
                    }
                    _ => {}
                }
                if camera.aspect_mode == wire::AspectMode::Fixed && camera.aspect <= 0. {
                    return Err("invalid camera aspect".into());
                }
            }
            wire::ComponentDataT::ParticleSystem(p) => {
                particles(p)?;
                for module in p.modules.iter().flatten() {
                    let feature = match &module.data {
                        wire::ParticleModuleDataT::SizeBySpeedModule(_) => {
                            Some("particle.size-by-speed")
                        }
                        wire::ParticleModuleDataT::RotationBySpeedModule(_) => {
                            Some("particle.rotation-by-speed")
                        }
                        _ => None,
                    };
                    if let Some(name) = feature
                        && !d
                            .features
                            .iter()
                            .flatten()
                            .any(|f| f.name == name && f.required && f.revision == 1)
                    {
                        return Err(format!("module needs required {name} revision 1"));
                    }
                    if let wire::ParticleModuleDataT::ShapeModule(s) = &module.data
                        && !matches!(s.geometry, wire::ShapeGeometryT::CircleShape(_))
                    {
                        let name = shape_feature(&s.geometry).ok_or("missing shape geometry")?;
                        if !d
                            .features
                            .iter()
                            .flatten()
                            .any(|f| f.name == name && f.required && f.revision == 1)
                        {
                            return Err(format!("shape needs required {name} revision 1"));
                        }
                    }
                }
                if p.main.space == wire::Space::Custom {
                    check(p.main.custom_space_node_id, "node")?;
                }
                if d.color_context.is_none() {
                    return Err("particle colors need source color context".into());
                }
            }
            wire::ComponentDataT::ParticleRenderer(r) => {
                check(r.system_component_id, "component")?;
                if !d.components.iter().flatten().any(|s| {
                    s.id == r.system_component_id
                        && s.node_id == c.node_id
                        && matches!(s.data, wire::ComponentDataT::ParticleSystem(_))
                }) {
                    return Err("renderer must reference its particle system".into());
                }
                if r.mesh_distribution != wire::MeshDistribution::UniformRandom
                    && r.mesh_distribution != wire::MeshDistribution::NonUniformRandom
                {
                    return Err("unknown mesh distribution".into());
                }
                if r.mode == wire::RenderMode::Mesh
                    && r.mesh_distribution == wire::MeshDistribution::NonUniformRandom
                {
                    if !d.features.iter().flatten().any(|f| {
                        f.name == "particle.mesh-distribution" && f.revision == 1 && f.required
                    }) {
                        return Err("weighted meshes need particle.mesh-distribution@1".into());
                    }
                    let weights = r.mesh_weights.as_deref().unwrap_or(&[]);
                    if weights.len() != r.mesh_asset_ids.as_deref().unwrap_or(&[]).len()
                        || weights.iter().any(|w| !w.is_finite() || *w < 0.0)
                        || !weights.iter().sum::<f32>().is_finite()
                    {
                        return Err("invalid mesh weights".into());
                    }
                }
                for m in &r.materials {
                    if *m != 0 {
                        check(*m, "material")?;
                    }
                }
                for m in r.mesh_asset_ids.iter().flatten() {
                    check(*m, "asset")?;
                    if !d
                        .assets
                        .iter()
                        .flatten()
                        .any(|a| a.id == *m && a.kind == wire::AssetKind::Mesh)
                    {
                        return Err("renderer mesh has wrong asset kind".into());
                    }
                }
            }
            wire::ComponentDataT::Animator(a) => {
                for l in &a.layers {
                    if !l.states.iter().any(|s| s.id == l.default_state_id) {
                        return Err("invalid default animator state".into());
                    }
                    for s in &l.states {
                        check(s.animation_id, "animation")?;
                    }
                }
            }
            wire::ComponentDataT::CustomComponent(s) => validate_fields(&s.fields, &ids, 0)?,
            _ => {}
        }
    }
    for m in d.materials.iter().flatten() {
        let legacy_feature = match m.model {
            wire::MaterialModel::LegacyParticleAdditive => {
                Some("material.unity-legacy-particle-additive")
            }
            wire::MaterialModel::LegacyParticleAlphaBlend => {
                Some("material.unity-legacy-particle-alpha-blend")
            }
            _ => None,
        };
        if let Some(name) = legacy_feature {
            for name in [name, "host.legacy-particle-environment"] {
                if !d
                    .features
                    .iter()
                    .flatten()
                    .any(|f| f.name == name && f.required && f.revision == 1)
                {
                    return Err(format!("legacy material needs required {name} revision 1"));
                }
            }
            if m.shader_data_asset_id == 0 {
                return Err("legacy material needs shader recipe".into());
            }
        }
        for t in m.textures.iter().flatten() {
            check(t.image_id, "asset")?;
            if !d
                .assets
                .iter()
                .flatten()
                .any(|a| a.id == t.image_id && a.kind == wire::AssetKind::Image)
            {
                return Err("texture has wrong asset kind".into());
            }
        }
        if m.shader_data_asset_id != 0 {
            check(m.shader_data_asset_id, "asset")?;
            if !d
                .assets
                .iter()
                .flatten()
                .any(|a| a.id == m.shader_data_asset_id && a.kind == wire::AssetKind::ShaderData)
            {
                return Err("shader data has wrong asset kind".into());
            }
        }
    }
    crate::rendering::validate(d)?;
    crate::animation::validate(d, &ids)?;
    for e in &d.entries {
        if e.roots.is_empty() {
            return Err("entry needs roots".into());
        }
        for r in &e.roots {
            check(*r, "node")?;
        }
        if e.camera_policy == wire::CameraPolicy::HostRequired
            && e.host_camera_slot.as_deref().is_none_or(str::is_empty)
        {
            return Err("host camera slot required".into());
        }
    }
    if let Some(c) = &d.color_context
        && c.project_color_space == wire::ColorSpace::Unspecified
        && c.host_project_color_space_slot
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err("missing source color context slot".into());
    }
    let mut names = BTreeSet::new();
    for f in d.features.iter().flatten() {
        if f.name.is_empty() || f.revision == 0 || !names.insert(&f.name) {
            return Err("invalid/duplicate feature requirement".into());
        }
    }
    let mut curve_result = Ok(());
    crate::curves::visit(d, |c| {
        if curve_result.is_ok() {
            curve_result = curve(c);
        }
    });
    curve_result?;
    if requires_unity_infinity(d)
        && !d
            .features
            .iter()
            .flatten()
            .any(|f| f.name == "curve.unity-infinity" && f.required && f.revision == 1)
    {
        return Err(
            "active Unity infinity curve needs required curve.unity-infinity revision 1".into(),
        );
    }
    crate::finite::validate(d)?;
    Ok(())
}
fn validate_fields(
    fields: &[wire::SerializedFieldT],
    ids: &BTreeMap<u32, &str>,
    depth: usize,
) -> Result<(), String> {
    if depth > 128 {
        return Err("script nesting exceeds 128".into());
    }
    for f in fields {
        validate_value(&f.value, ids, depth)?;
    }
    Ok(())
}
fn validate_value(
    v: &wire::SerializedValueT,
    ids: &BTreeMap<u32, &str>,
    depth: usize,
) -> Result<(), String> {
    if depth > 128 {
        return Err("script nesting exceeds 128".into());
    }
    match &v.data {
        wire::SerializedValueDataT::ReferenceValue(r) => {
            if let Some(t) = &r.target {
                let kind = match t.kind {
                    wire::ReferenceKind::Node => "node",
                    wire::ReferenceKind::Component => "component",
                    wire::ReferenceKind::Material => "material",
                    wire::ReferenceKind::Asset => "asset",
                    wire::ReferenceKind::Animation => "animation",
                    _ => return Err("unknown reference kind".into()),
                };
                if ids.get(&t.id) != Some(&kind) {
                    return Err("invalid script target".into());
                }
            }
        }
        wire::SerializedValueDataT::ArrayValue(a) => {
            for v in &a.values {
                validate_value(v, ids, depth + 1)?;
            }
        }
        wire::SerializedValueDataT::ObjectValue(o) => validate_fields(&o.fields, ids, depth + 1)?,
        wire::SerializedValueDataT::NONE => return Err("missing script value".into()),
        _ => {}
    }
    Ok(())
}

fn scalar(p: &wire::ScalarParameterT) -> Result<(), String> {
    match p.mode {
        wire::ScalarMode::Constant | wire::ScalarMode::TwoConstants => {}
        wire::ScalarMode::Curve | wire::ScalarMode::TwoCurves => {
            curve(p.curve_max.as_deref().ok_or("missing maximum curve")?)?;
            if p.mode == wire::ScalarMode::TwoCurves {
                curve(p.curve_min.as_deref().ok_or("missing minimum curve")?)?;
            }
        }
        _ => return Err("unknown scalar mode".into()),
    }
    Ok(())
}
fn curve(c: &wire::ScalarCurveT) -> Result<(), String> {
    if c.keys.is_empty() || c.keys.windows(2).any(|k| k[0].time >= k[1].time) {
        return Err("invalid curve key times".into());
    }
    for k in &c.keys {
        if !(0. ..=1.).contains(&k.in_weight) || !(0. ..=1.).contains(&k.out_weight) {
            return Err("invalid curve weights".into());
        }
    }
    Ok(())
}
fn vector(v: &wire::VectorParameterT) -> Result<(), String> {
    for p in [&v.x, &v.y, &v.z] {
        scalar(p.as_deref().ok_or("missing vector axis")?)?;
    }
    Ok(())
}
fn gradient(g: &wire::GradientT) -> Result<(), String> {
    if g.rgb.is_empty()
        || g.alpha.is_empty()
        || g.rgb.windows(2).any(|k| k[0].time >= k[1].time)
        || g.alpha.windows(2).any(|k| k[0].time >= k[1].time)
    {
        return Err("invalid gradient keys".into());
    }
    if g.rgb.iter().any(|k| !(0. ..=1.).contains(&k.time))
        || g.alpha.iter().any(|k| !(0. ..=1.).contains(&k.time))
    {
        return Err("gradient time outside 0..1".into());
    }
    Ok(())
}
fn color(p: &wire::ColorParameterT) -> Result<(), String> {
    match p.mode {
        wire::ColorMode::Constant | wire::ColorMode::TwoColors => {
            if p.color_max.is_none()
                || (p.mode == wire::ColorMode::TwoColors && p.color_min.is_none())
            {
                return Err("missing active color".into());
            }
        }
        wire::ColorMode::Gradient
        | wire::ColorMode::RandomColor
        | wire::ColorMode::TwoGradients => {
            gradient(p.gradient_max.as_deref().ok_or("missing gradient")?)?;
            if p.mode == wire::ColorMode::TwoGradients {
                gradient(p.gradient_min.as_deref().ok_or("missing gradient")?)?;
            }
        }
        _ => return Err("unknown color mode".into()),
    }
    Ok(())
}
fn particles(p: &wire::ParticleSystemT) -> Result<(), String> {
    let m = &p.main;
    if m.duration_seconds <= 0. || m.simulation_speed < 0. || m.max_particles == 0 {
        return Err("invalid particle main settings".into());
    }
    scalar(&m.lifetime)?;
    scalar(&m.speed)?;
    color(&m.color)?;
    vector(&m.size)?;
    vector(&m.rotation)?;
    for p in [&m.start_delay, &m.gravity_multiplier]
        .into_iter()
        .flatten()
    {
        scalar(p)?;
    }
    let mut types = BTreeSet::new();
    for module in p.modules.iter().flatten() {
        if !types.insert(module.data.particle_module_data_type().0) {
            return Err("duplicate particle module".into());
        }
        match &module.data {
            wire::ParticleModuleDataT::NONE => return Err("missing particle module payload".into()),
            wire::ParticleModuleDataT::EmissionModule(e) => {
                for p in [&e.rate_over_time, &e.rate_over_distance]
                    .into_iter()
                    .flatten()
                {
                    scalar(p)?;
                }
                for b in e.bursts.iter().flatten() {
                    scalar(&b.count)?;
                    if b.time_seconds < 0.
                        || b.interval_seconds < 0.
                        || !(0. ..=1.).contains(&b.probability)
                    {
                        return Err("invalid burst".into());
                    }
                }
            }
            wire::ParticleModuleDataT::ColorModule(c) => color(&c.color)?,
            wire::ParticleModuleDataT::SizeModule(s) => vector(&s.size)?,
            wire::ParticleModuleDataT::SizeBySpeedModule(s) => {
                vector(&s.size)?;
                speed_range(s.speed_range.as_ref())?;
            }
            wire::ParticleModuleDataT::RotationBySpeedModule(r) => {
                vector(&r.angular_velocity)?;
                speed_range(r.speed_range.as_ref())?;
            }
            wire::ParticleModuleDataT::RotationModule(r) => vector(&r.angular_velocity)?,
            wire::ParticleModuleDataT::VelocityModule(v) => {
                for v in [&v.linear, &v.orbital, &v.orbital_offset]
                    .into_iter()
                    .flatten()
                {
                    vector(v)?;
                }
                for p in [&v.radial, &v.speed_multiplier].into_iter().flatten() {
                    scalar(p)?;
                }
            }
            wire::ParticleModuleDataT::LimitVelocityModule(l) => {
                vector(&l.limit)?;
                if let Some(p) = &l.drag {
                    scalar(p)?;
                }
            }
            wire::ParticleModuleDataT::NoiseModule(n) => {
                if n.algorithm.is_empty() || n.algorithm_revision == 0 || n.octaves == 0 {
                    return Err("invalid noise contract".into());
                }
                vector(&n.strength)?;
                for p in [
                    &n.scroll_speed,
                    &n.position_amount,
                    &n.rotation_amount,
                    &n.size_amount,
                ]
                .into_iter()
                .flatten()
                {
                    scalar(p)?;
                }
                if n.remap_enabled {
                    vector(n.remap.as_deref().ok_or("missing noise remap")?)?;
                }
            }
            wire::ParticleModuleDataT::TextureSheetModule(t) => {
                if t.mode == wire::SheetMode::Grid && (t.columns == 0 || t.rows == 0) {
                    return Err("empty texture sheet".into());
                }
                for p in [&t.frame_over_time, &t.start_frame].into_iter().flatten() {
                    scalar(p)?;
                }
            }
            wire::ParticleModuleDataT::CustomDataModule(c) => {
                let mut slots = BTreeSet::new();
                for s in &c.streams {
                    if s.slot > 1 || !slots.insert(s.slot) {
                        return Err("invalid custom stream slot".into());
                    }
                    match s.mode {
                        wire::CustomDataMode::Disabled => {}
                        wire::CustomDataMode::Color => {
                            color(s.color.as_deref().ok_or("missing custom color")?)?
                        }
                        wire::CustomDataMode::Vector => {
                            if !(1..=4).contains(&s.vector_components)
                                || s.vector.as_ref().map_or(0, Vec::len)
                                    != s.vector_components as usize
                            {
                                return Err("custom vector count mismatch".into());
                            }
                            for p in s.vector.iter().flatten() {
                                scalar(p)?;
                            }
                        }
                        _ => return Err("unknown custom stream mode".into()),
                    }
                }
            }
            wire::ParticleModuleDataT::ShapeModule(s) => shape(s)?,
        }
    }
    Ok(())
}

/// Runtime capability for the actual geometry, including its emission region.
pub fn shape_feature(geometry: &wire::ShapeGeometryT) -> Option<&'static str> {
    Some(match geometry {
        wire::ShapeGeometryT::CircleShape(_) => "particle.shape.circle",
        wire::ShapeGeometryT::SphereShape(s) if s.hemisphere => "particle.shape.hemisphere",
        wire::ShapeGeometryT::SphereShape(_) => "particle.shape.sphere",
        wire::ShapeGeometryT::BoxShape(_) => "particle.shape.box",
        wire::ShapeGeometryT::ConeShape(c) if c.region == wire::ConeRegion::Volume => {
            "particle.shape.cone-volume"
        }
        wire::ShapeGeometryT::ConeShape(_) => "particle.shape.cone",
        wire::ShapeGeometryT::EdgeShape(_) => "particle.shape.edge",
        wire::ShapeGeometryT::DonutShape(_) => "particle.shape.donut",
        wire::ShapeGeometryT::SpriteShape(_) => "particle.shape.sprite",
        wire::ShapeGeometryT::NONE => return None,
    })
}

fn unit(value: f32) -> Result<(), String> {
    if !(0. ..=1.).contains(&value) {
        return Err("shape fraction outside [0,1]".into());
    }
    Ok(())
}
fn extent(parameter: Option<&wire::ScalarParameterT>) -> Result<(), String> {
    let p = parameter.ok_or("missing shape radius/half-length")?;
    scalar(p)?;
    if (p.mode == wire::ScalarMode::Constant && p.constant_max < 0.)
        || (p.mode == wire::ScalarMode::TwoConstants
            && (p.constant_min < 0. || p.constant_max < 0.))
    {
        return Err("negative shape radius/half-length".into());
    }
    Ok(())
}
fn arc(value: f32) -> Result<(), String> {
    if !(0. ..=std::f32::consts::TAU).contains(&value) {
        return Err("invalid shape arc".into());
    }
    Ok(())
}
fn shape(s: &wire::ShapeModuleT) -> Result<(), String> {
    match &s.geometry {
        wire::ShapeGeometryT::CircleShape(c) => {
            for p in [&c.radius, &c.radius_speed, &c.arc_speed]
                .into_iter()
                .flatten()
            {
                scalar(p)?;
            }
            extent(c.radius.as_deref())?;
            unit(c.radius_thickness)?;
            unit(c.radius_spread)?;
            unit(c.arc_spread)?;
            arc(c.arc_radians)?;
        }
        wire::ShapeGeometryT::SphereShape(s) => {
            extent(s.radius.as_deref())?;
            unit(s.radius_thickness)?;
        }
        wire::ShapeGeometryT::BoxShape(b) => {
            let t = b.thickness.as_ref().ok_or("missing box thickness")?;
            for v in [t.x, t.y, t.z] {
                unit(v)?;
            }
        }
        wire::ShapeGeometryT::ConeShape(c) => {
            for p in [&c.radius_speed, &c.arc_speed].into_iter().flatten() {
                scalar(p)?;
            }
            extent(c.radius.as_deref())?;
            unit(c.radius_thickness)?;
            unit(c.radius_spread)?;
            unit(c.arc_spread)?;
            arc(c.arc_radians)?;
            if !(0. ..=std::f32::consts::FRAC_PI_2).contains(&c.angle_radians) || c.length < 0. {
                return Err("invalid cone angle/length".into());
            }
        }
        wire::ShapeGeometryT::EdgeShape(e) => {
            extent(e.half_length.as_deref())?;
            unit(e.spread)?;
            if let Some(p) = &e.speed {
                scalar(p)?;
            }
        }
        wire::ShapeGeometryT::DonutShape(d) => {
            if d.radius < 0. || d.tube_radius < 0. {
                return Err("negative donut radius".into());
            }
            unit(d.radius_thickness)?;
            unit(d.arc_spread)?;
            arc(d.arc_radians)?;
            if let Some(p) = &d.arc_speed {
                scalar(p)?;
            }
        }
        wire::ShapeGeometryT::SpriteShape(s) => {
            if s.placement < wire::GeometryPlacement::Vertex
                || s.placement > wire::GeometryPlacement::Triangle
                || s.spawn_mode < wire::ShapeSweep::Random
                || s.spawn_mode > wire::ShapeSweep::BurstSpread
            {
                return Err("invalid Sprite emission placement/mode".into());
            }
            unit(s.spawn_spread)?;
            if let Some(p) = &s.spawn_speed {
                scalar(p)?;
            }
            if s.sprite_asset_id == 0 {
                return Err("missing shape sprite".into());
            }
        }
        wire::ShapeGeometryT::NONE => return Err("missing shape geometry".into()),
    }
    if s.transform.position.is_none()
        || s.transform.rotation.is_none()
        || s.transform.scale.is_none()
    {
        return Err("incomplete shape transform".into());
    }
    unit(s.random_direction)?;
    unit(s.spherical_direction)?;
    if s.random_position < 0. {
        return Err("negative shape random position".into());
    }
    Ok(())
}

fn speed_range(range: Option<&wire::Vec2T>) -> Result<(), String> {
    let r = range.ok_or("missing speed range")?;
    if !r.x.is_finite() || !r.y.is_finite() || r.x < 0. || r.y < r.x {
        return Err("speed range must satisfy 0 <= min <= max".into());
    }
    Ok(())
}
