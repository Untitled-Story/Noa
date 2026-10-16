use super::wire_util::*;
use noa_format::{ir, wire as w};
use std::collections::BTreeMap;
pub(crate) fn node(n: ir::Node) -> w::NodeT {
    n!(NodeT {
        id: n.id.get(),
        name: Some(n.name),
        parent_id: n.parent_id.map_or(0, |id| id.get()),
        active: n.active,
        layer_mask: n.layer_mask,
        transform: Box::new(n!(TransformT {
            position: Some(n!(Vec3T {
                x: n.transform.position.x,
                y: n.transform.position.y,
                z: n.transform.position.z
            })),
            rotation: Some(n!(QuaternionT {
                x: n.transform.rotation.x,
                y: n.transform.rotation.y,
                z: n.transform.rotation.z,
                w: n.transform.rotation.w
            })),
            scale: Some(n!(Vec3T {
                x: n.transform.scale.x,
                y: n.transform.scale.y,
                z: n.transform.scale.z
            }))
        }))
    })
}
pub(crate) fn layout(l: ir::RectLayout, host: bool) -> w::ComponentT {
    n!(ComponentT {
        id: l.id.get(),
        node_id: l.node_id.get(),
        data: w::ComponentDataT::RectLayout(Box::new(n!(RectLayoutT {
            anchor_min: Some(n!(Vec2T {
                x: l.anchor_min.x,
                y: l.anchor_min.y
            })),
            anchor_max: Some(n!(Vec2T {
                x: l.anchor_max.x,
                y: l.anchor_max.y
            })),
            pivot: Some(n!(Vec2T {
                x: l.pivot.x,
                y: l.pivot.y
            })),
            anchored_position: Some(n!(Vec3T {
                x: l.anchored_position.x,
                y: l.anchored_position.y,
                z: l.anchored_position.z
            })),
            size_delta: Some(n!(Vec2T {
                x: l.size_delta.x,
                y: l.size_delta.y
            })),
            host_layout_slot: host.then(|| "root-layout".into())
        })))
    })
}
pub(crate) fn animator(a: ir::Animator) -> w::ComponentT {
    n!(ComponentT {
        id: a.id.get(),
        node_id: a.node_id.get(),
        enabled: a.enabled,
        data: w::ComponentDataT::Animator(Box::new(n!(AnimatorT {
            keep_state_on_disable: a.keep_state_on_disable,
            write_default_values_on_disable: a.write_default_values_on_disable,
            layers: a
                .layers
                .into_iter()
                .map(|l| n!(AnimatorLayerT {
                    name: l.name,
                    default_weight: l.default_weight,
                    default_state_id: l.default_state_id.get(),
                    states: l
                        .states
                        .into_iter()
                        .map(|s| n!(AnimatorStateT {
                            id: s.id.get(),
                            name: s.name,
                            animation_id: s.animation_id.get(),
                            speed: s.speed,
                            cycle_offset: s.cycle_offset,
                            write_default_values: s.write_default_values
                        }))
                        .collect()
                }))
                .collect()
        })))
    })
}
pub(crate) fn animation(a: ir::Animation) -> w::AnimationT {
    n!(AnimationT {
        id: a.id.get(),
        name: Some(a.name),
        duration_seconds: a.duration_seconds,
        loop_: a.looped,
        tracks: Some(
            a.tracks
                .into_iter()
                .map(|t| n!(TrackT {
                    binding: Box::new(n!(BindingT {
                        target_id: t.target_id.get(),
                        property: w::AnimationProperty::NodeActive
                    })),
                    data: w::TrackDataT::BoolTrack(Box::new(n!(BoolTrackT {
                        keys: t
                            .keys
                            .into_iter()
                            .map(|k| n!(BoolKeyT {
                                time: k.time,
                                value: k.value
                            }))
                            .collect()
                    })))
                }))
                .collect()
        )
    })
}
pub(crate) fn custom(c: ir::CustomComponent) -> w::ComponentT {
    n!(ComponentT {
        id: c.id.get(),
        node_id: c.node_id.get(),
        enabled: c.enabled,
        data: w::ComponentDataT::CustomComponent(Box::new(n!(CustomComponentT {
            script: c.script.map(|s| Box::new(n!(ScriptIdentityT {
                assembly: s.assembly,
                namespace: s.namespace,
                class_name: s.class_name
            }))),
            source_script: Box::new(reference(c.source_script)),
            fields: fields(c.fields)
        })))
    })
}
fn reference(r: ir::SourceReference) -> w::SourceReferenceT {
    n!(SourceReferenceT {
        guid: r.guid,
        file_id: r.file_id
    })
}
fn fields(f: BTreeMap<String, ir::SerializedField>) -> Vec<w::SerializedFieldT> {
    f.into_iter()
        .map(|(name, f)| {
            n!(SerializedFieldT {
                name,
                declared_type: f.declared_type,
                value: Box::new(value(f.value))
            })
        })
        .collect()
}
fn value(v: ir::SerializedValue) -> w::SerializedValueT {
    let data = match v {
        ir::SerializedValue::Null => w::SerializedValueDataT::NullValue(Box::default()),
        ir::SerializedValue::Bool(value) => {
            w::SerializedValueDataT::BooleanValue(Box::new(n!(BooleanValueT { value })))
        }
        ir::SerializedValue::Int(value) => {
            w::SerializedValueDataT::SignedValue(Box::new(n!(SignedValueT { value })))
        }
        ir::SerializedValue::UInt(value) => {
            w::SerializedValueDataT::UnsignedValue(Box::new(n!(UnsignedValueT { value })))
        }
        ir::SerializedValue::Float(value) => {
            w::SerializedValueDataT::DoubleValue(Box::new(n!(DoubleValueT { value })))
        }
        ir::SerializedValue::String(value) => {
            w::SerializedValueDataT::StringValue(Box::new(n!(StringValueT { value })))
        }
        ir::SerializedValue::Array(a) => {
            w::SerializedValueDataT::ArrayValue(Box::new(n!(ArrayValueT {
                values: a.into_iter().map(value).collect()
            })))
        }
        ir::SerializedValue::Object(o) => {
            w::SerializedValueDataT::ObjectValue(Box::new(n!(ObjectValueT { fields: fields(o) })))
        }
        ir::SerializedValue::Reference(r) => {
            w::SerializedValueDataT::ReferenceValue(Box::new(n!(ReferenceValueT {
                source: Box::new(reference(r.source)),
                target: r.target.map(|t| Box::new(n!(TypedReferenceT {
                    id: t.id.get(),
                    kind: match t.kind {
                        ir::ReferenceKind::Node => w::ReferenceKind::Node,
                        ir::ReferenceKind::Component => w::ReferenceKind::Component,
                        ir::ReferenceKind::Material => w::ReferenceKind::Material,
                        ir::ReferenceKind::Asset => w::ReferenceKind::Asset,
                        ir::ReferenceKind::Animation => w::ReferenceKind::Animation,
                    }
                })))
            })))
        }
    };
    n!(SerializedValueT { data })
}
