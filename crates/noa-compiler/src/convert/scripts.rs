use super::EntityIds;
use crate::{
    diagnostics::{Diagnostic, Error, Severity},
    resolver::{ScriptReference, ScriptResolution, ScriptResolver},
    unity::{Guid, UnityFile},
};
use noa_format::ir::{
    CustomComponent, ReferenceValue, SerializedField, SerializedValue, SourceReference,
    TypedReference,
};
use serde_yaml_ng::Value;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct CustomFragment {
    pub components: Vec<CustomComponent>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Convert data without inferring any script behavior. The caller supplies the
/// targets that have actually been converted, not IDs of pending source objects.
pub fn convert_custom_components(
    source: &str,
    file: &UnityFile,
    ids: &EntityIds,
    scripts: &dyn ScriptResolver,
    targets: &dyn Fn(&SourceReference) -> Option<TypedReference>,
) -> Result<CustomFragment, Error> {
    let hierarchy = crate::unity::scene_nodes(file)?;
    let mut output = CustomFragment {
        components: Vec::new(),
        diagnostics: Vec::new(),
    };
    for object in file
        .objects
        .values()
        .filter(|o| o.type_name == "MonoBehaviour")
    {
        if object.stripped || object.class_id != 114 {
            return Err(Error::new(
                "UNSUPPORTED_SCRIPT_OBJECT",
                format!(
                    "{source}, fileID {}: expected expanded MonoBehaviour",
                    object.file_id
                ),
            ));
        }
        let at = format!("{source}, fileID {}", object.file_id);
        let owner = object
            .data
            .get("m_GameObject")
            .and_then(|v| v.get("fileID"))
            .and_then(Value::as_i64)
            .filter(|id| {
                hierarchy
                    .iter()
                    .any(|n| n.file_id == *id && n.component_file_ids.contains(&object.file_id))
            })
            .ok_or_else(|| {
                Error::new(
                    "INVALID_SCRIPT_OWNER",
                    format!("{at}: missing or inconsistent owner"),
                )
            })?;
        let source_script = object
            .data
            .get("m_Script")
            .ok_or_else(|| Error::new("INVALID_REFERENCE", format!("{at}: missing m_Script")))?;
        let source_script = source_reference(source_script)?
            .ok_or_else(|| Error::new("INVALID_REFERENCE", format!("{at}: invalid m_Script")))?;
        let reference = ScriptReference {
            guid: source_script.guid.as_ref().map(|s| s.parse()).transpose()?,
            file_id: source_script.file_id,
        };
        let script = match scripts.resolve(source, &reference) {
            ScriptResolution::Resolved(identity) => Some(identity),
            resolution => {
                output.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    code: if matches!(resolution, ScriptResolution::Conflict(_)) {
                        "SCRIPT_METADATA_CONFLICT"
                    } else {
                        "SCRIPT_METADATA_MISSING"
                    },
                    asset: source.into(),
                    file_id: Some(object.file_id),
                    property: Some("MonoBehaviour.m_Script".into()),
                    message: format!(
                        "{reference:?}: {resolution:?}; original script reference retained"
                    ),
                });
                None
            }
        };
        let enabled = match object.data.get("m_Enabled") {
            Some(Value::Bool(v)) => *v,
            Some(v) if v.as_i64() == Some(0) => false,
            Some(v) if v.as_i64() == Some(1) => true,
            _ => {
                return Err(Error::new(
                    "INVALID_SCRIPT_ENABLED",
                    format!("{at}.m_Enabled: expected bool or 0/1"),
                ));
            }
        };
        let mut fields = BTreeMap::new();
        for (key, value) in object.data.as_mapping().expect("parsed object mapping") {
            let name = key.as_str().ok_or_else(|| {
                Error::new(
                    "INVALID_SCRIPT_FIELD",
                    format!("{at}: field name must be string"),
                )
            })?;
            if crate::unity::is_script_engine_field(name) {
                continue;
            }
            let path = format!("MonoBehaviour.{name}");
            let value = field_value(
                value,
                source,
                object.file_id,
                &path,
                targets,
                &mut output.diagnostics,
                0,
            )?;
            fields.insert(
                name.into(),
                SerializedField {
                    declared_type: None,
                    value,
                },
            );
        }
        output.components.push(CustomComponent {
            id: ids.get(source, object.file_id)?,
            node_id: ids.get(source, owner)?,
            enabled,
            script,
            source_script,
            fields,
        });
    }
    output.components.sort_by_key(|c| c.id);
    Ok(output)
}

/// Some means a PPtr-shaped value, including explicit Unity null (fileID 0).
fn source_reference(value: &Value) -> Result<Option<SourceReference>, Error> {
    let Some(map) = value.as_mapping() else {
        return Ok(None);
    };
    if !map.contains_key(Value::String("fileID".into()))
        || !map
            .keys()
            .all(|k| matches!(k.as_str(), Some("fileID" | "guid" | "type")))
    {
        return Ok(None);
    }
    let file_id = value
        .get("fileID")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::new("INVALID_REFERENCE", "reference.fileID must be int64"))?;
    let guid = value
        .get("guid")
        .map(|v| {
            v.as_str()
                .ok_or_else(|| Error::new("INVALID_REFERENCE", "reference.guid must be string"))?
                .parse::<Guid>()
                .map(|g| g.to_string())
        })
        .transpose()?;
    Ok(Some(SourceReference { guid, file_id }))
}

fn field_value(
    value: &Value,
    source: &str,
    file_id: i64,
    path: &str,
    targets: &dyn Fn(&SourceReference) -> Option<TypedReference>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Result<SerializedValue, Error> {
    let invalid = |message: &str| {
        Error::new(
            "INVALID_SCRIPT_FIELD",
            format!("{source}, fileID {file_id}, {path}: {message}"),
        )
    };
    if depth > 128 {
        return Err(invalid("field nesting exceeds 128"));
    }
    if let Some(reference) = source_reference(value)
        .map_err(|e| Error::new(e.code, format!("{source}, fileID {file_id}, {path}: {e}")))?
    {
        if reference.file_id == 0 {
            return Ok(SerializedValue::Null);
        }
        let target = targets(&reference);
        if target.is_none() {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "UNCONVERTED_REFERENCE_TARGET",
                asset: source.into(),
                file_id: Some(file_id),
                property: Some(path.into()),
                message: format!(
                    "{reference:?}: no converted NOIR target; original reference retained"
                ),
            });
        }
        return Ok(SerializedValue::Reference(ReferenceValue {
            source: reference,
            target,
        }));
    }
    Ok(match value {
        Value::Null => SerializedValue::Null,
        Value::Bool(v) => SerializedValue::Bool(*v),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                SerializedValue::Int(v)
            } else if let Some(v) = v.as_u64() {
                SerializedValue::UInt(v)
            } else {
                SerializedValue::Float(
                    v.as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| invalid("expected finite number"))?,
                )
            }
        }
        Value::String(v) => SerializedValue::String(v.clone()),
        Value::Sequence(items) => SerializedValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    field_value(
                        v,
                        source,
                        file_id,
                        &format!("{path}[{i}]"),
                        targets,
                        diagnostics,
                        depth + 1,
                    )
                })
                .collect::<Result<_, _>>()?,
        ),
        Value::Mapping(items) => {
            let mut fields = BTreeMap::new();
            for (key, value) in items {
                let name = key
                    .as_str()
                    .ok_or_else(|| invalid("object key must be string"))?;
                let value = field_value(
                    value,
                    source,
                    file_id,
                    &format!("{path}.{name}"),
                    targets,
                    diagnostics,
                    depth + 1,
                )?;
                fields.insert(
                    name.into(),
                    SerializedField {
                        declared_type: None,
                        value,
                    },
                );
            }
            SerializedValue::Object(fields)
        }
        Value::Tagged(_) => return Err(invalid("unsupported YAML tag")),
    })
}
