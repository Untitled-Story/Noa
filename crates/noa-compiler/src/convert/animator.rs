use super::{EntityIds, SourceIdentity};
use crate::{
    diagnostics::Error,
    unity::{Guid, Object, UnityFile},
};
use noa_format::ir::{Animation, Animator, AnimatorLayer, AnimatorState, BoolKey, NodeActiveTrack};
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct UnitySources {
    pub files: BTreeMap<String, UnityFile>,
    pub guid_sources: BTreeMap<Guid, String>,
}

impl UnitySources {
    pub fn resolve<'a>(
        &'a self,
        owner: &'a str,
        reference: &Value,
    ) -> Result<(&'a str, &'a Object), Error> {
        let id = reference
            .get("fileID")
            .and_then(Value::as_i64)
            .filter(|v| *v != 0)
            .ok_or_else(|| Error::new("INVALID_ANIMATOR_REFERENCE", "expected non-null fileID"))?;
        let source = match reference.get("guid") {
            None => owner,
            Some(v) => {
                let guid: Guid = v
                    .as_str()
                    .ok_or_else(|| Error::new("INVALID_ANIMATOR_REFERENCE", "GUID must be string"))?
                    .parse()?;
                if guid.is_zero() {
                    owner
                } else {
                    self.guid_sources
                        .get(&guid)
                        .map(String::as_str)
                        .ok_or_else(|| {
                            Error::new("MISSING_ASSET", format!("cannot resolve GUID {guid}"))
                        })?
                }
            }
        };
        let object = self
            .files
            .get(source)
            .and_then(|f| f.objects.get(&id))
            .ok_or_else(|| Error::new("MISSING_OBJECT", format!("{source}, fileID {id}")))?;
        Ok((source, object))
    }
}

pub struct AnimatorPlan {
    owner_asset: String,
    source_id: i64,
    node_file_id: i64,
    enabled: bool,
    keep_state: bool,
    write_defaults: bool,
    layer_name: String,
    default_weight: f32,
    default_state: SourceIdentity,
    states: Vec<StatePlan>,
    clips: BTreeMap<SourceIdentity, ClipPlan>,
}

struct StatePlan {
    identity: SourceIdentity,
    name: String,
    clip: SourceIdentity,
    speed: f32,
    cycle_offset: f32,
    write_defaults: bool,
}

struct ClipPlan {
    name: String,
    looped: bool,
    tracks: Vec<(i64, bool)>,
}

impl AnimatorPlan {
    pub fn id_sources(&self) -> impl Iterator<Item = SourceIdentity> + '_ {
        self.states
            .iter()
            .map(|s| s.identity.clone())
            .chain(self.clips.keys().cloned())
    }

    pub fn lower(&self, ids: &EntityIds) -> Result<(Animator, Vec<Animation>), Error> {
        let states = self
            .states
            .iter()
            .map(|state| {
                Ok(AnimatorState {
                    id: ids.resolve(&state.identity)?,
                    name: state.name.clone(),
                    animation_id: ids.resolve(&state.clip)?,
                    speed: state.speed,
                    cycle_offset: state.cycle_offset,
                    write_default_values: state.write_defaults,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let animations = self
            .clips
            .iter()
            .map(|(identity, clip)| {
                Ok(Animation {
                    id: ids.resolve(identity)?,
                    name: clip.name.clone(),
                    duration_seconds: 0.0,
                    looped: clip.looped,
                    tracks: clip
                        .tracks
                        .iter()
                        .map(|(target, value)| {
                            Ok(NodeActiveTrack {
                                target_id: ids.get(&self.owner_asset, *target)?,
                                keys: vec![BoolKey {
                                    time: 0.0,
                                    value: *value,
                                }],
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok((
            Animator {
                id: ids.get(&self.owner_asset, self.source_id)?,
                node_id: ids.get(&self.owner_asset, self.node_file_id)?,
                enabled: self.enabled,
                keep_state_on_disable: self.keep_state,
                write_default_values_on_disable: self.write_defaults,
                layers: vec![AnimatorLayer {
                    name: self.layer_name.clone(),
                    default_weight: self.default_weight,
                    default_state_id: ids.resolve(&self.default_state)?,
                    states,
                }],
            },
            animations,
        ))
    }
}

pub fn plan_animators(owner: &str, sources: &UnitySources) -> Result<Vec<AnimatorPlan>, Error> {
    let file = sources
        .files
        .get(owner)
        .ok_or_else(|| Error::new("MISSING_ASSET", owner))?;
    let nodes = crate::unity::scene_nodes(file)?;
    let mut result = Vec::new();
    for object in file.objects.values().filter(|o| o.type_name == "Animator") {
        let plan = (|| {
            if object.class_id != 95 || object.stripped {
                return Err(unsupported("expanded Animator required"));
            }
            let data = &object.data;
            for field in [
                "m_CullingMode",
                "m_UpdateMode",
                "m_ApplyRootMotion",
                "m_LinearVelocityBlending",
                "m_StabilizeFeet",
            ] {
                zero(data, field)?;
            }
            null_ref(data, "m_Avatar")?;
            let node_id = integer(required(data, "m_GameObject")?, "fileID")?;
            if !nodes
                .iter()
                .any(|n| n.file_id == node_id && n.component_file_ids.contains(&object.file_id))
            {
                return Err(unsupported("inconsistent Animator owner"));
            }
            let (controller_source, controller) =
                sources.resolve(owner, required(data, "m_Controller")?)?;
            if controller.type_name != "AnimatorController" || controller.class_id != 91 {
                return Err(unsupported("AnimatorController required"));
            }
            empty(&controller.data, "m_AnimatorParameters")?;
            let layers = sequence(&controller.data, "m_AnimatorLayers")?;
            if layers.len() != 1 {
                return Err(unsupported(
                    "only one Controller layer is currently supported",
                ));
            }
            let layer = &layers[0];
            null_ref(layer, "m_Mask")?;
            empty(layer, "m_Motions")?;
            empty(layer, "m_Behaviours")?;
            zero(layer, "m_BlendingMode")?;
            zero(layer, "m_IKPass")?;
            zero(layer, "m_SyncedLayerAffectsTiming")?;
            if integer(layer, "m_SyncedLayerIndex")? != -1 {
                return Err(unsupported("synced layers are not supported"));
            }
            let (machine_source, machine) =
                sources.resolve(controller_source, required(layer, "m_StateMachine")?)?;
            if machine.type_name != "AnimatorStateMachine" || machine.class_id != 1107 {
                return Err(unsupported("AnimatorStateMachine required"));
            }
            for field in [
                "m_ChildStateMachines",
                "m_AnyStateTransitions",
                "m_EntryTransitions",
                "m_StateMachineTransitions",
                "m_StateMachineBehaviours",
            ] {
                empty(&machine.data, field)?;
            }
            let (default_source, default) =
                sources.resolve(machine_source, required(&machine.data, "m_DefaultState")?)?;
            let default_state =
                SourceIdentity::bound_to(default_source, default.file_id, owner, object.file_id);
            let mut states = Vec::new();
            let mut clips = BTreeMap::new();
            let mut state_ids = BTreeSet::new();
            let mut names = BTreeSet::new();
            for child in sequence(&machine.data, "m_ChildStates")? {
                let (state_source, state) =
                    sources.resolve(machine_source, required(child, "m_State")?)?;
                if state.type_name != "AnimatorState" || state.class_id != 1102 {
                    return Err(unsupported("AnimatorState required"));
                }
                let identity =
                    SourceIdentity::bound_to(state_source, state.file_id, owner, object.file_id);
                if !state_ids.insert(identity.clone()) {
                    return Err(unsupported("duplicate Animator state"));
                }
                for field in ["m_Transitions", "m_StateMachineBehaviours"] {
                    empty(&state.data, field)?;
                }
                for field in [
                    "m_IKOnFeet",
                    "m_Mirror",
                    "m_SpeedParameterActive",
                    "m_MirrorParameterActive",
                    "m_CycleOffsetParameterActive",
                    "m_TimeParameterActive",
                ] {
                    zero(&state.data, field)?;
                }
                let (clip_source, clip) =
                    sources.resolve(state_source, required(&state.data, "m_Motion")?)?;
                if clip.type_name != "AnimationClip" || clip.class_id != 74 {
                    return Err(unsupported("state Motion must be an AnimationClip"));
                }
                let clip_id =
                    SourceIdentity::bound_to(clip_source, clip.file_id, owner, object.file_id);
                if !clips.contains_key(&clip_id) {
                    let parsed = parse_clip(&clip.data, node_id, &nodes).map_err(|e| {
                        Error::new(
                            e.code,
                            format!("{clip_source}, fileID {}: {e}", clip.file_id),
                        )
                    })?;
                    clips.insert(clip_id.clone(), parsed);
                }
                let name = string(&state.data, "m_Name")?;
                if !names.insert(name.to_owned()) {
                    return Err(unsupported("duplicate Animator state name"));
                }
                states.push(StatePlan {
                    identity,
                    name: name.into(),
                    clip: clip_id,
                    speed: float(&state.data, "m_Speed")?,
                    cycle_offset: float(&state.data, "m_CycleOffset")?,
                    write_defaults: boolean(&state.data, "m_WriteDefaultValues")?,
                });
            }
            if !state_ids.contains(&default_state) {
                return Err(unsupported(
                    "default state is not listed in Controller layer",
                ));
            }
            Ok(AnimatorPlan {
                owner_asset: owner.into(),
                source_id: object.file_id,
                node_file_id: node_id,
                enabled: boolean(data, "m_Enabled")?,
                keep_state: boolean(data, "m_KeepAnimatorStateOnDisable")?,
                write_defaults: boolean(data, "m_WriteDefaultValuesOnDisable")?,
                layer_name: string(layer, "m_Name")?.into(),
                default_weight: float(layer, "m_DefaultWeight")?,
                default_state,
                states,
                clips,
            })
        })()
        .map_err(|e: Error| {
            Error::new(
                e.code,
                format!("{owner}, Animator fileID {}: {e}", object.file_id),
            )
        })?;
        result.push(plan);
    }
    Ok(result)
}

fn parse_clip(
    data: &Value,
    root: i64,
    nodes: &[crate::unity::SceneNode],
) -> Result<ClipPlan, Error> {
    for field in [
        "m_RotationCurves",
        "m_CompressedRotationCurves",
        "m_EulerCurves",
        "m_PositionCurves",
        "m_ScaleCurves",
        "m_PPtrCurves",
        "m_Events",
    ] {
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
        "m_StopTime",
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
    let mut by_path = BTreeMap::<String, Vec<i64>>::new();
    for node in nodes {
        let mut current = node;
        let mut segments = Vec::new();
        while current.file_id != root {
            let Some(parent) = current.parent_file_id else {
                break;
            };
            segments.push(current.name.as_str());
            current = nodes
                .iter()
                .find(|n| n.file_id == parent)
                .expect("validated parent");
        }
        if current.file_id == root {
            segments.reverse();
            by_path
                .entry(segments.join("/"))
                .or_default()
                .push(node.file_id);
        }
    }
    let mut targets = BTreeSet::new();
    let mut tracks = Vec::new();
    for curve in sequence(data, "m_FloatCurves")? {
        if string(curve, "attribute")? != "m_IsActive" || integer(curve, "classID")? != 1 {
            return Err(unsupported(
                "only initialization m_IsActive tracks are currently supported",
            ));
        }
        null_ref(curve, "script")?;
        zero(curve, "flags")?;
        let path = string(curve, "path")?;
        let candidates = by_path.get(path).ok_or_else(|| {
            Error::new(
                "MISSING_ANIMATION_TARGET",
                format!("node path {path:?} is absent under Animator owner"),
            )
        })?;
        if candidates.len() != 1 {
            return Err(Error::new(
                "AMBIGUOUS_ANIMATION_TARGET",
                format!("multiple nodes match {path:?}"),
            ));
        }
        let target = candidates[0];
        if !targets.insert(target) {
            return Err(unsupported("conflicting animation tracks"));
        }
        let keys = sequence(required(curve, "curve")?, "m_Curve")?;
        if keys.len() != 1 || float(&keys[0], "time")? != 0.0 {
            return Err(unsupported(
                "only one time=0 initialization key is currently supported",
            ));
        }
        let value = boolean(&keys[0], "value")?;
        tracks.push((target, value));
    }
    tracks.sort_by_key(|(id, _)| *id);
    Ok(ClipPlan {
        name: string(data, "m_Name")?.into(),
        looped: boolean(settings, "m_LoopTime")?,
        tracks,
    })
}

fn unsupported(message: &str) -> Error {
    Error::new("UNSUPPORTED_ANIMATOR", message)
}
fn required<'a>(v: &'a Value, key: &str) -> Result<&'a Value, Error> {
    v.get(key)
        .ok_or_else(|| Error::new("INVALID_ANIMATOR", format!("missing {key}")))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    required(v, key)?
        .as_str()
        .ok_or_else(|| Error::new("INVALID_ANIMATOR", format!("{key}: expected string")))
}
fn sequence<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], Error> {
    required(v, key)?
        .as_sequence()
        .map(Vec::as_slice)
        .ok_or_else(|| Error::new("INVALID_ANIMATOR", format!("{key}: expected array")))
}
fn integer(v: &Value, key: &str) -> Result<i64, Error> {
    required(v, key)?
        .as_i64()
        .ok_or_else(|| Error::new("INVALID_ANIMATOR", format!("{key}: expected integer")))
}
fn float(v: &Value, key: &str) -> Result<f32, Error> {
    required(v, key)?
        .as_f64()
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            Error::new(
                "INVALID_ANIMATOR",
                format!("{key}: expected finite float32"),
            )
        })
}
fn boolean(v: &Value, key: &str) -> Result<bool, Error> {
    match required(v, key)? {
        Value::Bool(v) => Ok(*v),
        v if v.as_f64() == Some(0.0) => Ok(false),
        v if v.as_f64() == Some(1.0) => Ok(true),
        _ => Err(Error::new(
            "INVALID_ANIMATOR",
            format!("{key}: expected bool or 0/1"),
        )),
    }
}
fn empty(v: &Value, key: &str) -> Result<(), Error> {
    let value = required(v, key)?;
    if value.as_sequence().is_some_and(Vec::is_empty)
        || value.as_mapping().is_some_and(|m| m.is_empty())
    {
        Ok(())
    } else {
        Err(unsupported(&format!(
            "{key} must be empty in the current supported subset"
        )))
    }
}
fn zero(v: &Value, key: &str) -> Result<(), Error> {
    let value = required(v, key)?;
    if value.as_f64() == Some(0.0) || value.as_bool() == Some(false) {
        Ok(())
    } else {
        Err(unsupported(&format!(
            "{key} must be zero in the current supported subset"
        )))
    }
}
fn null_ref(v: &Value, key: &str) -> Result<(), Error> {
    if integer(required(v, key)?, "fileID")? == 0 {
        Ok(())
    } else {
        Err(unsupported(&format!(
            "{key} must be null in the current supported subset"
        )))
    }
}
