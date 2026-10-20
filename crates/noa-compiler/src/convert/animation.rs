//! Resolve authored bindings before runtime. No editor curves or guessed hashes.
use super::{
    ClipPlan, UnitySources, boolean, empty, float, integer, null_ref, required, sequence, string,
    unsupported, zero,
};
use crate::{
    convert::{SourceIdentity, curves::curve, wire_util::*},
    diagnostics::Error,
    unity::SceneNode,
};
use noa_format::wire as w;
use serde_yaml_ng::Value as Y;
use std::collections::BTreeMap;

fn target_path(path: &str, root: i64, nodes: &[SceneNode]) -> Result<i64, Error> {
    let matches: Vec<_> = nodes
        .iter()
        .filter(|node| {
            let mut current = *node;
            let mut parts = Vec::new();
            while current.file_id != root {
                let Some(parent) = current.parent_file_id else {
                    return false;
                };
                parts.push(current.name.as_str());
                current = nodes
                    .iter()
                    .find(|n| n.file_id == parent)
                    .expect("validated parent");
            }
            parts.reverse();
            parts.join("/") == path
        })
        .collect();
    match matches.as_slice() {
        [node] => Ok(node.file_id),
        [] => Err(Error::new(
            "MISSING_ANIMATION_TARGET",
            format!("node path {path:?} is absent under Animator owner"),
        )),
        _ => Err(Error::new(
            "AMBIGUOUS_ANIMATION_TARGET",
            format!("multiple nodes match {path:?}"),
        )),
    }
}
fn component(node: i64, class: u32, owner: &str, sources: &UnitySources) -> Result<i64, Error> {
    let candidates: Vec<_> = sources.files[owner]
        .objects
        .values()
        .filter(|o| o.class_id == class && o.data["m_GameObject"]["fileID"].as_i64() == Some(node))
        .collect();
    match candidates.as_slice() {
        [o] => Ok(o.file_id),
        _ => Err(unsupported(
            "animation target needs exactly one matching component",
        )),
    }
}
type GroupKey = (i64, u16, u16, String, u32);
struct Group {
    binding: w::BindingT,
    channels: BTreeMap<char, w::ScalarCurveT>,
    rotation: Option<w::RotationEncoding>,
}
fn add(
    groups: &mut BTreeMap<GroupKey, Group>,
    target: i64,
    binding: w::BindingT,
    channel: char,
    curve: w::ScalarCurveT,
    rotation: Option<w::RotationEncoding>,
) -> Result<(), Error> {
    let key = (
        target,
        binding.property.0,
        binding.parameter.0,
        binding.property_name.clone().unwrap_or_default(),
        binding.slot_index,
    );
    let g = groups.entry(key).or_insert_with(|| Group {
        binding,
        channels: BTreeMap::new(),
        rotation,
    });
    if g.rotation != rotation || g.channels.insert(channel, curve).is_some() {
        return Err(unsupported("conflicting animation tracks"));
    }
    Ok(())
}
fn scalar_curve(v: &Y, scale: f32) -> Result<w::ScalarCurveT, Error> {
    let mut c = curve(v, w::CurveDomain::Seconds)?;
    for k in &mut c.keys {
        k.value *= scale;
        k.in_slope *= scale;
        k.out_slope *= scale;
    }
    if c.keys.iter().any(|k| k.time < 0.) {
        return Err(unsupported("negative animation key time"));
    }
    Ok(c)
}
fn split_curve(v: &Y, axis: char, scale: f32) -> Result<w::ScalarCurveT, Error> {
    let mut v = v.clone();
    for k in v["m_Curve"]
        .as_sequence_mut()
        .ok_or_else(|| unsupported("expected vector keys"))?
    {
        for field in ["value", "inSlope", "outSlope", "inWeight", "outWeight"] {
            k[field] = required(required(k, field)?, &axis.to_string())?.clone();
        }
    }
    scalar_curve(&v, scale)
}
pub(super) fn parse_clip(
    data: &Y,
    root: i64,
    nodes: &[SceneNode],
    owner: &str,
    clip_source: &str,
    sources: &UnitySources,
) -> Result<ClipPlan, Error> {
    for field in ["m_CompressedRotationCurves", "m_PPtrCurves"] {
        empty(data, field)?;
    }
    for field in [
        "m_Legacy",
        "m_Compressed",
        "m_HasGenericRootTransform",
        "m_HasMotionFloatCurves",
    ] {
        zero(data, field)?;
    }
    let settings = required(data, "m_AnimationClipSettings")?;
    for field in [
        "m_StartTime",
        "m_OrientationOffsetY",
        "m_Level",
        "m_CycleOffset",
        "m_HasAdditiveReferencePose",
        "m_LoopBlend",
        "m_Mirror",
    ] {
        zero(settings, field)?;
    }
    null_ref(settings, "m_AdditiveReferencePoseClip")?;
    let duration = float(settings, "m_StopTime")?;
    if duration < 0. {
        return Err(unsupported("negative clip duration"));
    }
    let mut groups = BTreeMap::new();
    let mut tracks = Vec::new();
    for (field, property, axes, rotation, scale) in [
        (
            "m_PositionCurves",
            w::AnimationProperty::NodePosition,
            "xyz",
            None,
            1.,
        ),
        (
            "m_ScaleCurves",
            w::AnimationProperty::NodeScale,
            "xyz",
            None,
            1.,
        ),
        (
            "m_EulerCurves",
            w::AnimationProperty::NodeRotation,
            "xyz",
            Some(w::RotationEncoding::EulerZXY),
            std::f32::consts::PI / 180.,
        ),
        (
            "m_RotationCurves",
            w::AnimationProperty::NodeRotation,
            "xyzw",
            Some(w::RotationEncoding::QuaternionComponents),
            1.,
        ),
    ] {
        for entry in sequence(data, field)? {
            let target = target_path(string(entry, "path")?, root, nodes)?;
            let c = required(entry, "curve")?;
            if rotation == Some(w::RotationEncoding::EulerZXY)
                && integer(c, "m_RotationOrder")? != 4
            {
                return Err(unsupported(
                    "Euler animation requires Unity ZXY rotation order",
                ));
            }
            for axis in axes.chars() {
                add(
                    &mut groups,
                    target,
                    n!(BindingT { property }),
                    axis,
                    split_curve(c, axis, scale)?,
                    rotation,
                )?;
            }
        }
    }
    for entry in sequence(data, "m_FloatCurves")? {
        null_ref(entry, "script")?;
        zero(entry, "flags")?;
        let node = target_path(string(entry, "path")?, root, nodes)?;
        let class = u32::try_from(integer(entry, "classID")?)
            .map_err(|_| unsupported("invalid binding class"))?;
        let attr = string(entry, "attribute")?;
        let c = required(entry, "curve")?;
        if (class == 1 && attr == "m_IsActive")
            || (class == 198 && attr == "looping")
            || (matches!(class, 95 | 199 | 23 | 212) && attr == "m_Enabled")
        {
            let target = if class == 1 {
                node
            } else {
                component(node, class, owner, sources)?
            };
            let keys = sequence(c, "m_Curve")?
                .iter()
                .map(|k| {
                    Ok(n!(BoolKeyT {
                        time: float(k, "time")?,
                        value: boolean(k, "value")?
                    }))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            if keys.is_empty()
                || keys.iter().any(|k| k.time < 0.)
                || keys.windows(2).any(|k| k[0].time >= k[1].time)
            {
                return Err(unsupported("invalid bool key times"));
            }
            tracks.push((
                target,
                n!(TrackT {
                    binding: Box::new(n!(BindingT {
                        property: if class == 1 {
                            w::AnimationProperty::NodeActive
                        } else if attr == "looping" {
                            w::AnimationProperty::ParticleLoop
                        } else if class == 199 {
                            w::AnimationProperty::RendererEnabled
                        } else {
                            w::AnimationProperty::ComponentEnabled
                        }
                    })),
                    data: w::TrackDataT::BoolTrack(Box::new(n!(BoolTrackT { keys })))
                }),
            ));
            continue;
        }
        let (target, binding, axis, rotation, scale) = binding(node, class, attr, owner, sources)?;
        add(
            &mut groups,
            target,
            binding,
            axis,
            scalar_curve(c, scale)?,
            rotation,
        )?;
    }
    for ((target, _, _, _, _), mut g) in groups {
        let mut take = |axis| g.channels.remove(&axis).map(Box::new);
        let data = match g.binding.property {
            w::AnimationProperty::NodeRotation => {
                w::TrackDataT::RotationTrack(Box::new(n!(RotationTrackT {
                    encoding: g.rotation.unwrap(),
                    x: take('x'),
                    y: take('y'),
                    z: take('z'),
                    w: take('w')
                })))
            }
            w::AnimationProperty::RendererMaterialTint
            | w::AnimationProperty::SpriteColor
            | w::AnimationProperty::ParticleColor
            | w::AnimationProperty::ParticleCustomColor => {
                w::TrackDataT::ColorTrack(Box::new(n!(ColorTrackT {
                    r: take('r'),
                    g: take('g'),
                    b: take('b'),
                    a: take('a')
                })))
            }
            w::AnimationProperty::RendererTextureST => {
                w::TrackDataT::Vector4Track(Box::new(n!(Vector4TrackT {
                    x: take('x'),
                    y: take('y'),
                    z: take('z'),
                    w: take('w')
                })))
            }
            w::AnimationProperty::ParticleParameter
            | w::AnimationProperty::RendererMaterialFloat => {
                w::TrackDataT::FloatTrack(Box::new(n!(FloatTrackT {
                    curve: take('v').unwrap()
                })))
            }
            _ => w::TrackDataT::Vector3Track(Box::new(n!(Vector3TrackT {
                x: take('x'),
                y: take('y'),
                z: take('z')
            }))),
        };
        tracks.push((
            target,
            n!(TrackT {
                binding: Box::new(g.binding),
                data
            }),
        ));
    }
    tracks.sort_by_key(|(id, t)| {
        (
            *id,
            t.binding.property.0,
            t.binding.parameter.0,
            t.binding.property_name.clone(),
            t.binding.slot_index,
        )
    });
    for pair in tracks.windows(2) {
        if pair[0].0 == pair[1].0
            && pair[0].1.binding.property == pair[1].1.binding.property
            && pair[0].1.binding.parameter == pair[1].1.binding.parameter
            && pair[0].1.binding.property_name == pair[1].1.binding.property_name
            && pair[0].1.binding.slot_index == pair[1].1.binding.slot_index
        {
            return Err(unsupported("conflicting animation tracks"));
        }
    }
    let mut events = Vec::new();
    for e in sequence(data, "m_Events")? {
        let time = float(e, "time")?;
        if time < 0.
            || events
                .last()
                .is_some_and(|(p, _): &(w::AnimationEventT, _)| p.time_seconds > time)
        {
            return Err(unsupported("events must have nonnegative ordered times"));
        }
        let name = string(e, "functionName")?;
        if name.is_empty() {
            return Err(unsupported("empty event function name"));
        }
        let source = required(e, "objectReferenceParameter")?;
        let fid = integer(source, "fileID")?;
        let (object_parameter, reference) = if fid == 0 {
            (None, None)
        } else if fid == 2800000
            && source
                .get("guid")
                .and_then(Y::as_str)
                .and_then(|g| g.parse().ok())
                .and_then(|g| sources.guid_sources.get(&g))
                .is_some_and(|p| p.ends_with(".png"))
        {
            // PNGs are indexed resources, not serialized Unity objects. The
            // resource converter assigns their ID during final compilation.
            (
                Some(Box::new(n!(ReferenceValueT {
                    source: Box::new(n!(SourceReferenceT {
                        file_id: fid,
                        guid: source.get("guid").and_then(Y::as_str).map(str::to_owned)
                    }))
                }))),
                None,
            )
        } else {
            let (asset, o) = sources.resolve(clip_source, source)?;
            let kind = match o.class_id {
                1 | 4 => w::ReferenceKind::Node,
                95 | 114 | 198 | 199 | 224 | 33 | 23 | 212 | 331 | 222 | 223 | 225 => {
                    w::ReferenceKind::Component
                }
                21 => w::ReferenceKind::Material,
                28 | 43 | 213 => w::ReferenceKind::Asset,
                _ => return Err(unsupported("event object reference type is not converted")),
            };
            if matches!(kind, w::ReferenceKind::Node | w::ReferenceKind::Component)
                && asset != owner
            {
                return Err(unsupported(
                    "event cannot reference another prefab instance",
                ));
            }
            (
                Some(Box::new(n!(ReferenceValueT {
                    source: Box::new(n!(SourceReferenceT {
                        file_id: fid,
                        guid: source.get("guid").and_then(Y::as_str).map(str::to_owned)
                    }))
                }))),
                Some((
                    SourceIdentity::new(
                        asset,
                        if o.class_id == 4 {
                            integer(required(&o.data, "m_GameObject")?, "fileID")?
                        } else {
                            fid
                        },
                    ),
                    kind,
                )),
            )
        };
        events.push((
            n!(AnimationEventT {
                time_seconds: time,
                name: name.into(),
                string_parameter: Some(string(e, "data")?.into()),
                float_parameter: float(e, "floatParameter")?,
                int_parameter: i32::try_from(integer(e, "intParameter")?)
                    .map_err(|_| unsupported("event int outside int32"))?,
                message_options: enum_value(
                    integer(e, "messageOptions")?,
                    &[
                        w::AnimationMessageOptions::RequireReceiver,
                        w::AnimationMessageOptions::DontRequireReceiver
                    ],
                    "event message options"
                )?,
                object_parameter
            }),
            reference,
        ));
    }
    Ok(ClipPlan {
        name: string(data, "m_Name")?.into(),
        duration,
        looped: boolean(settings, "m_LoopTime")?,
        tracks,
        events,
    })
}

fn binding(
    node: i64,
    class: u32,
    attr: &str,
    owner: &str,
    sources: &UnitySources,
) -> Result<(i64, w::BindingT, char, Option<w::RotationEncoding>, f32), Error> {
    if matches!(class, 4 | 224) {
        let (base, axis) = attr
            .rsplit_once('.')
            .ok_or_else(|| unsupported("expected transform component binding"))?;
        let axis = axis
            .chars()
            .next()
            .filter(|_| axis.len() == 1)
            .ok_or_else(|| unsupported("invalid transform channel"))?;
        let (property, rotation, scale) = match base {
            "m_LocalPosition" => (w::AnimationProperty::NodePosition, None, 1.),
            "m_LocalScale" => (w::AnimationProperty::NodeScale, None, 1.),
            "m_LocalRotation" => (
                w::AnimationProperty::NodeRotation,
                Some(w::RotationEncoding::QuaternionComponents),
                1.,
            ),
            "localEulerAnglesRaw" => (
                w::AnimationProperty::NodeRotation,
                Some(w::RotationEncoding::EulerZXY),
                std::f32::consts::PI / 180.,
            ),
            "m_AnchoredPosition" if class == 224 => {
                (w::AnimationProperty::RectAnchoredPosition, None, 1.)
            }
            "m_SizeDelta" if class == 224 => (w::AnimationProperty::RectSizeDelta, None, 1.),
            "m_AnchorMin" if class == 224 => (w::AnimationProperty::RectAnchorMin, None, 1.),
            "m_AnchorMax" if class == 224 => (w::AnimationProperty::RectAnchorMax, None, 1.),
            "m_Pivot" if class == 224 => (w::AnimationProperty::RectPivot, None, 1.),
            _ => {
                return Err(unsupported(&format!(
                    "unsupported transform property {attr}"
                )));
            }
        };
        let allowed = if rotation == Some(w::RotationEncoding::QuaternionComponents) {
            "xyzw"
        } else if matches!(
            property,
            w::AnimationProperty::RectAnchoredPosition
                | w::AnimationProperty::RectSizeDelta
                | w::AnimationProperty::RectAnchorMin
                | w::AnimationProperty::RectAnchorMax
                | w::AnimationProperty::RectPivot
        ) {
            "xy"
        } else {
            "xyz"
        };
        if !allowed.contains(axis) {
            return Err(unsupported("invalid transform axis"));
        }
        let target = if matches!(
            property,
            w::AnimationProperty::NodePosition
                | w::AnimationProperty::NodeRotation
                | w::AnimationProperty::NodeScale
        ) {
            node
        } else {
            component(node, 224, owner, sources)?
        };
        return Ok((target, n!(BindingT { property }), axis, rotation, scale));
    }
    if class == 212
        && attr.starts_with("m_Color.")
        && attr.len() == 9
        && "rgba".contains(&attr[8..])
    {
        return Ok((
            component(node, 212, owner, sources)?,
            n!(BindingT {
                property: w::AnimationProperty::SpriteColor
            }),
            attr.chars().last().unwrap(),
            None,
            1.,
        ));
    }
    if matches!(class, 199 | 212 | 23) && attr.starts_with("material.") {
        let target = component(node, class, owner, sources)?;
        let name = attr.strip_prefix("material.").unwrap();
        if let Some(axis) = name
            .strip_prefix("_Color.")
            .or_else(|| name.strip_prefix("_TintColor."))
            .filter(|s| s.len() == 1 && "rgba".contains(*s))
        {
            return Ok((
                target,
                n!(BindingT {
                    property: w::AnimationProperty::RendererMaterialTint,
                    property_name: Some(name.rsplit_once('.').unwrap().0.into())
                }),
                axis.chars().next().unwrap(),
                None,
                1.,
            ));
        }
        if let Some(axis) = name
            .strip_prefix("_MainTex_ST.")
            .filter(|s| s.len() == 1 && "xyzw".contains(*s))
        {
            return Ok((
                target,
                n!(BindingT {
                    property: w::AnimationProperty::RendererTextureST,
                    property_name: Some("_MainTex".into())
                }),
                axis.chars().next().unwrap(),
                None,
                1.,
            ));
        }
        if !name.contains('.') && !name.is_empty() {
            // Verify the property against an imported material before packaging.
            return Ok((
                target,
                n!(BindingT {
                    property: w::AnimationProperty::RendererMaterialFloat,
                    property_name: Some(name.into())
                }),
                'v',
                None,
                1.,
            ));
        }
    }
    if class == 198 {
        let target = component(node, 198, owner, sources)?;
        for (prefix, property) in [
            (
                "InitialModule.startColor.",
                w::AnimationProperty::ParticleColor,
            ),
            (
                "CustomDataModule.color0.",
                w::AnimationProperty::ParticleCustomColor,
            ),
            (
                "CustomDataModule.color1.",
                w::AnimationProperty::ParticleCustomColor,
            ),
        ] {
            if let Some(tail) = attr.strip_prefix(prefix)
                && let Some((branch, axis)) = tail.rsplit_once('.')
                && matches!(branch, "minColor" | "maxColor")
                && axis.len() == 1
                && "rgba".contains(axis)
            {
                return Ok((
                    target,
                    n!(BindingT {
                        property,
                        property_name: Some(branch.into()),
                        slot_index: if prefix.contains("color1") { 1 } else { 0 }
                    }),
                    axis.chars().next().unwrap(),
                    None,
                    1.,
                ));
            }
        }
        let (parameter, field) = particle_parameter(attr).ok_or_else(|| {
            unsupported(&format!("unsupported particle animation property {attr}"))
        })?;
        return Ok((
            component(node, 198, owner, sources)?,
            n!(BindingT {
                property: w::AnimationProperty::ParticleParameter,
                parameter,
                property_name: Some(field.into())
            }),
            'v',
            None,
            1.,
        ));
    }
    Err(unsupported(&format!(
        "unsupported animation binding class={class} attribute={attr}"
    )))
}
fn particle_parameter(attr: &str) -> Option<(w::ParticleParameter, &str)> {
    let (base, field) = attr.rsplit_once('.')?;
    let field = match field {
        "scalar" => "scalar",
        "minScalar" => "minScalar",
        _ => return None,
    };
    let p = match base {
        "InitialModule.startLifetime" => w::ParticleParameter::StartLifetime,
        "InitialModule.startSpeed" => w::ParticleParameter::StartSpeed,
        "InitialModule.startSize" => w::ParticleParameter::StartSizeX,
        "InitialModule.startSizeY" => w::ParticleParameter::StartSizeY,
        "InitialModule.startSizeZ" => w::ParticleParameter::StartSizeZ,
        "InitialModule.startRotationX" => w::ParticleParameter::StartRotationX,
        "InitialModule.startRotationY" => w::ParticleParameter::StartRotationY,
        "InitialModule.startRotation" => w::ParticleParameter::StartRotationZ,
        "InitialModule.gravityModifier" => w::ParticleParameter::GravityMultiplier,
        "EmissionModule.rateOverTime" => w::ParticleParameter::RateOverTime,
        "EmissionModule.rateOverDistance" => w::ParticleParameter::RateOverDistance,
        "VelocityModule.x" => w::ParticleParameter::VelocityX,
        "VelocityModule.y" => w::ParticleParameter::VelocityY,
        "VelocityModule.z" => w::ParticleParameter::VelocityZ,
        _ => return None,
    };
    Some((p, field))
}
