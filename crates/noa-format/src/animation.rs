//! Animation binding, payload, reference and required-capability validation.
use crate::wire as w;
use std::collections::{BTreeMap, BTreeSet};

fn required(d: &w::DocumentT, name: &str) -> Result<(), String> {
    if d.features
        .iter()
        .flatten()
        .any(|f| f.name == name && f.required && f.revision == 1)
    {
        Ok(())
    } else {
        Err(format!("animation needs required {name} revision 1"))
    }
}
pub(crate) fn validate(d: &w::DocumentT, ids: &BTreeMap<u32, &str>) -> Result<(), String> {
    for a in d.animations.iter().flatten() {
        if !a.duration_seconds.is_finite() || a.duration_seconds < 0. {
            return Err("invalid animation duration".into());
        }
        let mut bindings = BTreeSet::new();
        for t in a.tracks.iter().flatten() {
            let b = &t.binding;
            if !bindings.insert((
                b.target_id,
                b.property.0,
                b.parameter.0,
                b.property_name.as_deref(),
                b.slot_index,
            )) {
                return Err("conflicting animation bindings".into());
            }
            let kind = ids
                .get(&b.target_id)
                .copied()
                .ok_or("missing animation target")?;
            let component = d.components.iter().flatten().find(|c| c.id == b.target_id);
            let is_renderer =
                component.is_some_and(|c| matches!(c.data, w::ComponentDataT::ParticleRenderer(_)));
            let is_particle =
                component.is_some_and(|c| matches!(c.data, w::ComponentDataT::ParticleSystem(_)));
            let is_layout =
                component.is_some_and(|c| matches!(c.data, w::ComponentDataT::RectLayout(_)));
            let has_vector = matches!(&t.data,w::TrackDataT::Vector3Track(v) if v.x.is_some()||v.y.is_some()||v.z.is_some());
            let has_color = matches!(&t.data,w::TrackDataT::ColorTrack(v) if v.r.is_some()||v.g.is_some()||v.b.is_some()||v.a.is_some());
            let valid = match b.property {
                w::AnimationProperty::NodeActive => {
                    kind == "node" && matches!(t.data, w::TrackDataT::BoolTrack(_))
                }
                w::AnimationProperty::NodePosition | w::AnimationProperty::NodeScale => {
                    required(d, "animation.transform")?;
                    kind == "node" && has_vector
                }
                w::AnimationProperty::NodeRotation => {
                    required(d, "animation.transform")?;
                    kind == "node"
                        && matches!(&t.data,w::TrackDataT::RotationTrack(r)
                        if (r.encoding==w::RotationEncoding::EulerZXY && r.w.is_none() || r.encoding==w::RotationEncoding::QuaternionComponents) && (r.x.is_some()||r.y.is_some()||r.z.is_some()||r.w.is_some()))
                }
                w::AnimationProperty::RectAnchoredPosition
                | w::AnimationProperty::RectSizeDelta
                | w::AnimationProperty::RectAnchorMin
                | w::AnimationProperty::RectAnchorMax
                | w::AnimationProperty::RectPivot => {
                    required(d, "animation.layout")?;
                    is_layout
                        && has_vector
                        && (b.property == w::AnimationProperty::RectAnchoredPosition
                            || matches!(&t.data,w::TrackDataT::Vector3Track(v) if v.z.is_none()))
                }
                w::AnimationProperty::RendererEnabled | w::AnimationProperty::ComponentEnabled => {
                    required(d, "animation.component-enabled")?;
                    kind == "component"
                        && (b.property != w::AnimationProperty::RendererEnabled || is_renderer)
                        && matches!(t.data, w::TrackDataT::BoolTrack(_))
                }
                w::AnimationProperty::ParticleLoop => {
                    required(d, "animation.particle")?;
                    is_particle && matches!(t.data, w::TrackDataT::BoolTrack(_))
                }
                w::AnimationProperty::ParticleParameter => {
                    required(d, "animation.particle")?;
                    if let Some(w::ComponentDataT::ParticleSystem(p)) = component.map(|c| &c.data) {
                        match b.parameter {
                            w::ParticleParameter::GravityMultiplier
                                if p.main
                                    .host_gravity_slot
                                    .as_deref()
                                    .is_none_or(str::is_empty)
                                    && p.main.gravity.is_none() =>
                            {
                                return Err("animated gravity needs explicit gravity source".into());
                            }
                            w::ParticleParameter::RateOverTime
                            | w::ParticleParameter::RateOverDistance
                                if !p.modules.iter().flatten().any(|m| {
                                    matches!(m.data, w::ParticleModuleDataT::EmissionModule(_))
                                }) =>
                            {
                                return Err("animated emission needs emission module".into());
                            }
                            w::ParticleParameter::VelocityX
                            | w::ParticleParameter::VelocityY
                            | w::ParticleParameter::VelocityZ
                                if !p.modules.iter().flatten().any(|m| {
                                    matches!(m.data, w::ParticleModuleDataT::VelocityModule(_))
                                }) =>
                            {
                                return Err("animated velocity needs velocity module".into());
                            }
                            w::ParticleParameter::StartSizeY | w::ParticleParameter::StartSizeZ
                                if !p.main.size_3d =>
                            {
                                return Err("animated size axis needs size3D".into());
                            }
                            w::ParticleParameter::StartRotationX
                            | w::ParticleParameter::StartRotationY
                                if !p.main.rotation_3d =>
                            {
                                return Err("animated rotation axis needs rotation3D".into());
                            }
                            _ => {}
                        }
                    }
                    is_particle
                        && matches!(
                            b.parameter,
                            w::ParticleParameter::StartLifetime
                                | w::ParticleParameter::StartSpeed
                                | w::ParticleParameter::StartSizeX
                                | w::ParticleParameter::StartSizeY
                                | w::ParticleParameter::StartSizeZ
                                | w::ParticleParameter::StartRotationX
                                | w::ParticleParameter::StartRotationY
                                | w::ParticleParameter::StartRotationZ
                                | w::ParticleParameter::GravityMultiplier
                                | w::ParticleParameter::RateOverTime
                                | w::ParticleParameter::RateOverDistance
                                | w::ParticleParameter::VelocityX
                                | w::ParticleParameter::VelocityY
                                | w::ParticleParameter::VelocityZ
                        )
                        && matches!(b.property_name.as_deref(), Some("scalar" | "minScalar"))
                        && matches!(t.data, w::TrackDataT::FloatTrack(_))
                }
                w::AnimationProperty::ParticleColor | w::AnimationProperty::ParticleCustomColor => {
                    required(d, "animation.particle")?;
                    if b.property == w::AnimationProperty::ParticleCustomColor
                        && let Some(w::ComponentDataT::ParticleSystem(p)) =
                            component.map(|c| &c.data)
                        && !p.modules.iter().flatten().any(|m| match &m.data {
                            w::ParticleModuleDataT::CustomDataModule(c) => {
                                c.streams.iter().any(|s| {
                                    u32::from(s.slot) == b.slot_index
                                        && s.mode == w::CustomDataMode::Color
                                })
                            }
                            _ => false,
                        })
                    {
                        return Err("animated custom color needs color stream".into());
                    }
                    is_particle
                        && has_color
                        && matches!(b.property_name.as_deref(), Some("minColor" | "maxColor"))
                        && (b.property != w::AnimationProperty::ParticleCustomColor
                            || b.slot_index <= 1)
                }
                w::AnimationProperty::RendererMaterialTint
                | w::AnimationProperty::RendererMaterialFloat
                | w::AnimationProperty::RendererTextureST => {
                    required(d, "animation.renderer-material")?;
                    if !is_renderer {
                        return Err("material animation needs particle renderer".into());
                    }
                    let Some(w::ComponentDataT::ParticleRenderer(r)) = component.map(|c| &c.data)
                    else {
                        unreachable!()
                    };
                    let id = *r
                        .materials
                        .get(b.slot_index as usize)
                        .ok_or("missing animated material slot")?;
                    let m = d
                        .materials
                        .iter()
                        .flatten()
                        .find(|m| m.id == id)
                        .ok_or("missing animated material")?;
                    match b.property {
                        w::AnimationProperty::RendererMaterialTint => {
                            // _Color is not used by our particle-add-alpha or legacy shaders.
                            // Saved unused properties must not become a tint operation.
                            has_color
                                && b.property_name.as_deref() == Some("_TintColor")
                                && matches!(
                                    m.model,
                                    w::MaterialModel::LegacyParticleAdditive
                                        | w::MaterialModel::LegacyParticleAlphaBlend
                                )
                        }
                        w::AnimationProperty::RendererMaterialFloat => {
                            matches!(t.data, w::TrackDataT::FloatTrack(_))
                                && m.floats
                                    .iter()
                                    .flatten()
                                    .any(|f| Some(f.name.as_str()) == b.property_name.as_deref())
                        }
                        _ => {
                            matches!(&t.data,w::TrackDataT::Vector4Track(v) if v.x.is_some()||v.y.is_some()||v.z.is_some()||v.w.is_some())
                                && m.textures
                                    .iter()
                                    .flatten()
                                    .any(|v| Some(v.slot.as_str()) == b.property_name.as_deref())
                        }
                    }
                }
                // Existing schema properties remain constrained to typed targets/payloads.
                w::AnimationProperty::MaterialTint => kind == "material" && has_color,
                w::AnimationProperty::MaterialFloat => {
                    kind == "material" && matches!(t.data, w::TrackDataT::FloatTrack(_))
                }
                w::AnimationProperty::RendererMaterial | w::AnimationProperty::RendererMesh => {
                    is_renderer && matches!(t.data, w::TrackDataT::ReferenceTrack(_))
                }
                w::AnimationProperty::MaterialTexture => {
                    kind == "material" && matches!(t.data, w::TrackDataT::ReferenceTrack(_))
                }
                w::AnimationProperty::CameraActive => {
                    component.is_some_and(|c| matches!(c.data, w::ComponentDataT::Camera(_)))
                        && matches!(t.data, w::TrackDataT::BoolTrack(_))
                }
                w::AnimationProperty::CameraFov
                | w::AnimationProperty::CameraOrthoSize
                | w::AnimationProperty::CameraNear
                | w::AnimationProperty::CameraFar => {
                    component.is_some_and(|c| matches!(c.data, w::ComponentDataT::Camera(_)))
                        && matches!(t.data, w::TrackDataT::FloatTrack(_))
                }
                _ => false,
            };
            if !valid {
                return Err(format!(
                    "invalid animation binding/payload {:?} on {}",
                    b.property, b.target_id
                ));
            }
            if let w::TrackDataT::BoolTrack(v) = &t.data
                && (v.keys.is_empty()
                    || v.keys.iter().any(|k| !k.time.is_finite() || k.time < 0.)
                    || v.keys.windows(2).any(|k| k[0].time >= k[1].time))
            {
                return Err("invalid bool track keys".into());
            }
            if let w::TrackDataT::ReferenceTrack(v) = &t.data {
                if v.keys.is_empty() || v.keys.windows(2).any(|k| k[0].time >= k[1].time) {
                    return Err("invalid reference track keys".into());
                }
                for k in &v.keys {
                    let expected = if b.property == w::AnimationProperty::RendererMaterial {
                        "material"
                    } else {
                        "asset"
                    };
                    if k.time < 0. || ids.get(&k.ref_id) != Some(&expected) {
                        return Err("invalid animation reference key".into());
                    }
                }
            }
        }
        let events = a.events.as_deref().unwrap_or(&[]);
        if !events.is_empty() {
            required(d, "animation.events-typed")?
        }
        for e in events {
            if e.name.is_empty()
                || e.time_seconds < 0.
                || e.time_seconds > a.duration_seconds
                || !matches!(
                    e.message_options,
                    w::AnimationMessageOptions::RequireReceiver
                        | w::AnimationMessageOptions::DontRequireReceiver
                )
            {
                return Err("invalid animation event".into());
            }
            if !d.components.iter().flatten().any(|c| {
                c.id == e.animator_component_id && matches!(c.data, w::ComponentDataT::Animator(_))
            }) {
                return Err("event needs owning Animator".into());
            }
            if let Some(r) = &e.object_parameter {
                if r.source.file_id == 0 {
                    return Err("event object reference cannot have zero source ID".into());
                }
                let t = r
                    .target
                    .as_ref()
                    .ok_or("unresolved event object reference")?;
                let kind = match t.kind {
                    w::ReferenceKind::Node => "node",
                    w::ReferenceKind::Component => "component",
                    w::ReferenceKind::Material => "material",
                    w::ReferenceKind::Asset => "asset",
                    w::ReferenceKind::Animation => "animation",
                    _ => return Err("unknown event reference kind".into()),
                };
                if ids.get(&t.id) != Some(&kind) {
                    return Err("invalid event object target".into());
                }
            }
        }
        if events
            .windows(2)
            .any(|e| e[0].time_seconds > e[1].time_seconds)
        {
            return Err("events must be time ordered".into());
        }
    }
    Ok(())
}
