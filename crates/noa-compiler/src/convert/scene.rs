use super::EntityIds;
use crate::{diagnostics::Error, unity::UnityFile};
use noa_format::ir::{EntityId, Node, Quaternion, RectLayout, Transform, Vec2, Vec3};
use serde::Serialize;
use serde_yaml_ng::Value;
use std::collections::BTreeSet;

/// A component whose runtime semantics have not been converted yet.
#[derive(Debug, Clone, Serialize)]
pub struct PendingComponent {
    pub file_id: i64,
    pub node_id: EntityId,
    pub type_name: String,
}

/// An intermediate fragment. Pending components prevent complete package output.
#[derive(Debug, Serialize)]
pub struct SceneFragment {
    pub nodes: Vec<Node>,
    pub layouts: Vec<RectLayout>,
    /// Root RectTransforms need an external parent layout; no dimensions guessed.
    pub host_layout_nodes: Vec<EntityId>,
    pub pending_components: Vec<PendingComponent>,
}

pub fn convert_scene(
    source: &str,
    file: &UnityFile,
    ids: &EntityIds,
) -> Result<SceneFragment, Error> {
    let result = convert(source, file, ids);
    result.map_err(|e| Error::new(e.code, format!("{source}: {e}")))
}

fn convert(source: &str, file: &UnityFile, ids: &EntityIds) -> Result<SceneFragment, Error> {
    let hierarchy = crate::unity::scene_nodes(file)?;
    let mut result = SceneFragment {
        nodes: Vec::new(),
        layouts: Vec::new(),
        host_layout_nodes: Vec::new(),
        pending_components: Vec::new(),
    };
    let mut listed = BTreeSet::new();
    for node in hierarchy {
        let game_object = &file.objects[&node.file_id];
        let transform = &file.objects[&node.transform_file_id];
        if game_object.stripped || transform.stripped {
            return Err(Error::new(
                "UNSUPPORTED_PREFAB_INSTANCE",
                "stripped objects need prefab expansion",
            ));
        }
        let expected_class = if transform.type_name == "RectTransform" {
            224
        } else {
            4
        };
        if game_object.class_id != 1 || transform.class_id != expected_class {
            return Err(Error::new(
                "CLASS_TYPE_MISMATCH",
                format!("fileID {}: unexpected scene classID", transform.file_id),
            ));
        }
        let id = ids.get(source, node.file_id)?;
        let data = &transform.data;
        let path = format!("fileID {}", transform.file_id);
        let position = vec3(data, "m_LocalPosition", &path)?;
        let rotation = Quaternion {
            x: number(data, "m_LocalRotation.x", &path)?,
            y: number(data, "m_LocalRotation.y", &path)?,
            z: number(data, "m_LocalRotation.z", &path)?,
            w: number(data, "m_LocalRotation.w", &path)?,
        };
        let norm = rotation.x * rotation.x
            + rotation.y * rotation.y
            + rotation.z * rotation.z
            + rotation.w * rotation.w;
        if (norm - 1.0).abs() > 0.0001 || !norm.is_finite() {
            return Err(Error::new(
                "INVALID_TRANSFORM",
                format!("{path}.m_LocalRotation: expected unit quaternion"),
            ));
        }
        let layer = game_object
            .data
            .get("m_Layer")
            .and_then(Value::as_u64)
            .filter(|v| *v < 32)
            .ok_or_else(|| {
                Error::new(
                    "INVALID_LAYER",
                    format!("fileID {}.m_Layer: expected 0..31", node.file_id),
                )
            })?;
        result.nodes.push(Node {
            id,
            name: node.name,
            parent_id: node
                .parent_file_id
                .map(|id| ids.get(source, id))
                .transpose()?,
            active: node.active_self,
            transform: Transform {
                position,
                rotation,
                scale: vec3(data, "m_LocalScale", &path)?,
            },
            layer_mask: 1u32 << layer,
        });
        if transform.type_name == "RectTransform" {
            let anchored = vec2(data, "m_AnchoredPosition", &path)?;
            result.layouts.push(RectLayout {
                id: ids.get(source, transform.file_id)?,
                node_id: id,
                anchor_min: vec2(data, "m_AnchorMin", &path)?,
                anchor_max: vec2(data, "m_AnchorMax", &path)?,
                pivot: vec2(data, "m_Pivot", &path)?,
                anchored_position: Vec3 {
                    x: anchored.x,
                    y: anchored.y,
                    z: position.z,
                },
                size_delta: vec2(data, "m_SizeDelta", &path)?,
            });
            if node.parent_file_id.is_none() {
                result.host_layout_nodes.push(id);
            }
        }
        for component_id in node.component_file_ids {
            listed.insert(component_id);
            if component_id != transform.file_id {
                let component = &file.objects[&component_id];
                result.pending_components.push(PendingComponent {
                    file_id: component_id,
                    node_id: id,
                    type_name: component.type_name.clone(),
                });
            }
        }
    }
    for object in file.objects.values() {
        if object.data.get("m_GameObject").is_some() && !listed.contains(&object.file_id) {
            return Err(Error::new(
                "INVALID_HIERARCHY",
                format!(
                    "fileID {}: component not listed by its GameObject",
                    object.file_id
                ),
            ));
        }
    }
    result.nodes.sort_by_key(|n| n.id);
    result.layouts.sort_by_key(|c| c.id);
    result.host_layout_nodes.sort();
    result.pending_components.sort_by_key(|c| c.file_id);
    Ok(result)
}

fn number(value: &Value, field: &str, source: &str) -> Result<f32, Error> {
    let value = field
        .split('.')
        .try_fold(value, |v, key| v.get(key))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            Error::new(
                "INVALID_TRANSFORM",
                format!("{source}.{field}: expected finite number"),
            )
        })?;
    let result = value as f32;
    if !result.is_finite() {
        return Err(Error::new(
            "INVALID_TRANSFORM",
            format!("{source}.{field}: outside float32 range"),
        ));
    }
    Ok(result)
}

fn vec2(value: &Value, field: &str, source: &str) -> Result<Vec2, Error> {
    Ok(Vec2 {
        x: number(value, &format!("{field}.x"), source)?,
        y: number(value, &format!("{field}.y"), source)?,
    })
}

fn vec3(value: &Value, field: &str, source: &str) -> Result<Vec3, Error> {
    Ok(Vec3 {
        x: number(value, &format!("{field}.x"), source)?,
        y: number(value, &format!("{field}.y"), source)?,
        z: number(value, &format!("{field}.z"), source)?,
    })
}
