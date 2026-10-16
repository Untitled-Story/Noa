use super::UnityFile;
use crate::diagnostics::Error;
use serde::Serialize;
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct SceneNode {
    pub file_id: i64,
    pub transform_file_id: i64,
    pub parent_file_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub root_file_id: i64,
    pub active_self: bool,
    pub active_in_hierarchy: bool,
    pub component_file_ids: Vec<i64>,
}

/// Read source hierarchy without applying clips, layout, or script behaviour.
pub fn scene_nodes(file: &UnityFile) -> Result<Vec<SceneNode>, Error> {
    let mut transform_by_node = BTreeMap::new();
    let mut node_by_transform = BTreeMap::new();
    for object in file
        .objects
        .values()
        .filter(|o| matches!(o.type_name.as_str(), "Transform" | "RectTransform"))
    {
        let node = reference(&object.data, "m_GameObject")?;
        let target = file
            .objects
            .get(&node)
            .filter(|o| o.type_name == "GameObject")
            .ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!(
                        "transform {} refers to nonexistent GameObject {node}",
                        object.file_id
                    ),
                )
            })?;
        if transform_by_node.insert(target.file_id, object).is_some() {
            return Err(Error::new(
                "INVALID_HIERARCHY",
                format!("GameObject {node} has multiple transforms"),
            ));
        }
        node_by_transform.insert(object.file_id, target.file_id);
    }
    let mut nodes = BTreeMap::new();
    for object in file
        .objects
        .values()
        .filter(|o| o.type_name == "GameObject")
    {
        let transform = transform_by_node.get(&object.file_id).ok_or_else(|| {
            Error::new(
                "INVALID_HIERARCHY",
                format!("GameObject {} has no transform", object.file_id),
            )
        })?;
        let parent_transform = reference(&transform.data, "m_Father")?;
        let parent_file_id = if parent_transform == 0 {
            None
        } else {
            Some(*node_by_transform.get(&parent_transform).ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!(
                        "transform {} has missing parent {parent_transform}",
                        transform.file_id
                    ),
                )
            })?)
        };
        let name = object
            .data
            .get("m_Name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!("GameObject {} has no string m_Name", object.file_id),
                )
            })?
            .to_owned();
        let active_self = match object.data.get("m_IsActive").and_then(Value::as_i64) {
            Some(0) => false,
            Some(1) => true,
            _ => {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!("GameObject {} has invalid m_IsActive", object.file_id),
                ));
            }
        };
        let components = object
            .data
            .get("m_Component")
            .and_then(Value::as_sequence)
            .ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!("GameObject {} has no component list", object.file_id),
                )
            })?;
        let mut component_file_ids = Vec::new();
        let mut seen = BTreeSet::new();
        for value in components {
            let id = reference(value, "component")?;
            if !seen.insert(id) {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!("GameObject {} repeats component {id}", object.file_id),
                ));
            }
            let component = file.objects.get(&id).ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!("GameObject {} has missing component {id}", object.file_id),
                )
            })?;
            if reference(&component.data, "m_GameObject")? != object.file_id {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!("component {id} has inconsistent GameObject ownership"),
                ));
            }
            component_file_ids.push(id);
        }
        if !component_file_ids.contains(&transform.file_id) {
            return Err(Error::new(
                "INVALID_HIERARCHY",
                format!("GameObject {} does not list its transform", object.file_id),
            ));
        }
        // Validate the opposite edge, not just m_Father.
        let children = transform
            .data
            .get("m_Children")
            .and_then(Value::as_sequence)
            .ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    format!("transform {} has no child list", transform.file_id),
                )
            })?;
        let mut child_set = BTreeSet::new();
        for child in children {
            let child_id = child.get("fileID").and_then(Value::as_i64).ok_or_else(|| {
                Error::new(
                    "INVALID_HIERARCHY",
                    "child must be a local transform reference",
                )
            })?;
            if !child_set.insert(child_id) {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!("transform {} repeats child {child_id}", transform.file_id),
                ));
            }
            let child = file
                .objects
                .get(&child_id)
                .filter(|o| matches!(o.type_name.as_str(), "Transform" | "RectTransform"))
                .ok_or_else(|| {
                    Error::new(
                        "INVALID_HIERARCHY",
                        format!("missing child transform {child_id}"),
                    )
                })?;
            if reference(&child.data, "m_Father")? != transform.file_id {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!("transform {child_id} has inconsistent parent"),
                ));
            }
        }
        if parent_transform != 0 {
            let parent = &file.objects[&parent_transform];
            let listed = parent
                .data
                .get("m_Children")
                .and_then(Value::as_sequence)
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|v| v.get("fileID").and_then(Value::as_i64) == Some(transform.file_id))
                });
            if !listed {
                return Err(Error::new(
                    "INVALID_HIERARCHY",
                    format!(
                        "parent {parent_transform} does not list child {}",
                        transform.file_id
                    ),
                ));
            }
        }
        nodes.insert(
            object.file_id,
            SceneNode {
                file_id: object.file_id,
                transform_file_id: transform.file_id,
                parent_file_id,
                name,
                path: String::new(),
                root_file_id: 0,
                active_self,
                active_in_hierarchy: active_self,
                component_file_ids,
            },
        );
    }
    for id in nodes.keys().copied().collect::<Vec<_>>() {
        let mut seen = BTreeSet::from([id]);
        let mut current = id;
        let mut segments = Vec::new();
        let mut active = nodes[&id].active_self;
        while let Some(parent) = nodes[&current].parent_file_id {
            if !seen.insert(parent) {
                return Err(Error::new(
                    "HIERARCHY_CYCLE",
                    format!("cycle involving GameObject {parent}"),
                ));
            }
            segments.push(nodes[&current].name.clone());
            active &= nodes[&parent].active_self;
            current = parent;
        }
        segments.reverse();
        let node = nodes.get_mut(&id).expect("existing node");
        node.path = segments.join("/");
        node.root_file_id = current;
        node.active_in_hierarchy = active;
    }
    Ok(nodes.into_values().collect())
}

fn reference(value: &Value, field: &str) -> Result<i64, Error> {
    let reference = value
        .get(field)
        .ok_or_else(|| Error::new("INVALID_HIERARCHY", format!("missing {field}")))?;
    if reference.get("guid").is_some() {
        return Err(Error::new(
            "UNSUPPORTED_HIERARCHY",
            format!("external hierarchy reference at {field} needs prefab expansion"),
        ));
    }
    reference
        .get("fileID")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            Error::new(
                "INVALID_HIERARCHY",
                format!("{field} must contain an integer fileID"),
            )
        })
}
