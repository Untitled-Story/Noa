use crate::diagnostics::Error;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::{collections::BTreeMap, fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Guid(String);

impl Guid {
    pub fn is_zero(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }

    pub fn is_builtin(&self) -> bool {
        matches!(
            self.0.as_str(),
            "0000000000000000e000000000000000"
                | "0000000000000000f000000000000000"
                | "0000000000000000d000000000000000"
        )
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Guid {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::new(
                "INVALID_GUID",
                format!("invalid Unity GUID: {value}"),
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub class_id: u32,
    pub file_id: i64,
    pub stripped: bool,
    pub type_name: String,
    pub data: Value,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceKind {
    Local,
    External { guid: Guid },
    Builtin { guid: Guid },
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectReference {
    pub source_file_id: i64,
    pub property: String,
    pub target_file_id: i64,
    #[serde(flatten)]
    pub kind: ReferenceKind,
}

#[derive(Debug, Clone)]
pub struct UnityFile {
    pub objects: BTreeMap<i64, Object>,
}

impl UnityFile {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut objects = BTreeMap::new();
        let mut current: Option<(u32, i64, bool, usize)> = None;
        let mut body = String::new();
        for (index, line) in text.lines().enumerate() {
            if line.starts_with("---") {
                if let Some(header) = current.take() {
                    insert_object(&mut objects, header, &body)?;
                    body.clear();
                }
                let parts: Vec<_> = line.split_whitespace().collect();
                if !(parts.len() == 3 || (parts.len() == 4 && parts[3] == "stripped"))
                    || parts[0] != "---"
                    || !parts[1].starts_with("!u!")
                    || !parts[2].starts_with('&')
                {
                    return Err(Error::new(
                        "INVALID_UNITY_HEADER",
                        format!(
                            "line {}: expected --- !u!<classID> &<fileID> [stripped]",
                            index + 1
                        ),
                    ));
                }
                let class_id = parts[1][3..].parse().map_err(|_| {
                    Error::new(
                        "INVALID_CLASS_ID",
                        format!("line {}: {}", index + 1, parts[1]),
                    )
                })?;
                let file_id = parts[2][1..].parse().map_err(|_| {
                    Error::new(
                        "INVALID_FILE_ID",
                        format!("line {}: {}", index + 1, parts[2]),
                    )
                })?;
                current = Some((class_id, file_id, parts.len() == 4, index + 1));
            } else if current.is_some() {
                body.push_str(line);
                body.push('\n');
            } else if !(line.trim().is_empty() || line.starts_with('%') || line.starts_with('#')) {
                return Err(Error::new(
                    "INVALID_UNITY_YAML",
                    format!("line {}: content before Unity document header", index + 1),
                ));
            }
        }
        if let Some(header) = current {
            insert_object(&mut objects, header, &body)?;
        }
        if objects.is_empty() {
            return Err(Error::new("EMPTY_UNITY_FILE", "no Unity objects found"));
        }
        Ok(Self { objects })
    }

    pub fn references(&self) -> Result<Vec<ObjectReference>, Error> {
        let mut refs = Vec::new();
        for object in self.objects.values() {
            collect_refs(&object.data, object.file_id, &object.type_name, &mut refs)?;
        }
        Ok(refs)
    }
}

fn insert_object(
    objects: &mut BTreeMap<i64, Object>,
    header: (u32, i64, bool, usize),
    text: &str,
) -> Result<(), Error> {
    let (class_id, file_id, stripped, line) = header;
    if file_id == 0 {
        return Err(Error::new(
            "INVALID_FILE_ID",
            format!("line {line}: object fileID cannot be zero"),
        ));
    }
    let value: UnityValue = serde_yaml_ng::from_str(text).map_err(|e| {
        Error::new(
            "INVALID_UNITY_YAML",
            format!("document at line {line}: {e}"),
        )
    })?;
    let map = value
        .0
        .as_mapping()
        .filter(|m| m.len() == 1)
        .ok_or_else(|| {
            Error::new(
                "INVALID_UNITY_OBJECT",
                format!("line {line}: expected exactly one object type"),
            )
        })?;
    let (name, data) = map.iter().next().expect("one entry");
    let type_name = name
        .as_str()
        .ok_or_else(|| Error::new("INVALID_UNITY_OBJECT", "object type must be a string"))?
        .to_owned();
    if !data.is_mapping() {
        return Err(Error::new(
            "INVALID_UNITY_OBJECT",
            format!("line {line}: {type_name} must contain a mapping"),
        ));
    }
    let object = Object {
        class_id,
        file_id,
        stripped,
        type_name,
        data: data.clone(),
        line,
    };
    if objects.insert(file_id, object).is_some() {
        return Err(Error::new(
            "DUPLICATE_FILE_ID",
            format!("line {line}: repeated fileID {file_id}"),
        ));
    }
    Ok(())
}

fn collect_refs(
    value: &Value,
    file_id: i64,
    path: &str,
    refs: &mut Vec<ObjectReference>,
) -> Result<(), Error> {
    match value {
        Value::Mapping(map) => {
            // Only mappings entirely shaped as Unity PPtr are references.
            // Shader structures can contain unrelated fields named fileID.
            let is_pptr = map.contains_key(Value::String("fileID".into()))
                && map
                    .keys()
                    .all(|k| matches!(k.as_str(), Some("fileID" | "guid" | "type")));
            if is_pptr {
                let target = map
                    .get(Value::String("fileID".into()))
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        Error::new(
                            "INVALID_REFERENCE",
                            format!("fileID {file_id}, {path}: expected integer fileID"),
                        )
                    })?;
                if target == 0 {
                    return Ok(());
                }
                let guid = match map.get(Value::String("guid".into())) {
                    None => None,
                    Some(v) => Some(
                        v.as_str()
                            .ok_or_else(|| {
                                Error::new(
                                    "INVALID_REFERENCE",
                                    format!("fileID {file_id}, {path}: GUID must be a string"),
                                )
                            })?
                            .parse::<Guid>()?,
                    ),
                };
                let kind = match guid {
                    Some(guid) if guid.is_builtin() => ReferenceKind::Builtin { guid },
                    Some(guid) if !guid.is_zero() => ReferenceKind::External { guid },
                    _ => ReferenceKind::Local,
                };
                refs.push(ObjectReference {
                    source_file_id: file_id,
                    property: path.to_owned(),
                    target_file_id: target,
                    kind,
                });
            } else {
                for (key, val) in map {
                    let key = key
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{key:?}"));
                    collect_refs(val, file_id, &format!("{path}.{key}"), refs)?;
                }
            }
        }
        Value::Sequence(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_refs(item, file_id, &format!("{path}[{index}]"), refs)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn parse_meta_guid(text: &str) -> Result<Guid, Error> {
    #[derive(Deserialize)]
    struct Meta {
        guid: String,
    }
    let meta: Meta =
        serde_yaml_ng::from_str(text).map_err(|e| Error::new("INVALID_META", e.to_string()))?;
    meta.guid.parse()
}

// Unity GUIDs are unquoted hexadecimal strings. Ordinary YAML inference reads
// digit-only GUIDs as integers and builtin e000 GUIDs as exponential numbers.
// Request String directly from the YAML deserializer for GUID values, before
// inference loses leading zeroes. All other fields retain their YAML types.
struct UnityValue(Value);

impl<'de> Deserialize<'de> for UnityValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UnityValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a Unity YAML value")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::from(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::from(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::from(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UnityValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UnityValue(v)) = seq.next_element()? {
                    values.push(v);
                }
                Ok(UnityValue(Value::Sequence(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_yaml_ng::Mapping::new();
                while let Some(key) = map.next_key::<Value>()? {
                    let value = if matches!(
                        key.as_str(),
                        Some(
                            "guid"
                                | "m_VertexStreams"
                                | "m_TrailVertexStreams"
                                | "m_IndexBuffer"
                                | "_typelessdata"
                                | "compressedBlob"
                                | "m_BoneNameHashes"
                        )
                    ) {
                        Value::String(map.next_value::<String>()?)
                    } else if key.as_str() == Some("m_ParameterBlobIndices") {
                        Value::Sequence(
                            map.next_value::<Vec<String>>()?
                                .into_iter()
                                .map(Value::String)
                                .collect(),
                        )
                    } else if key.as_str() == Some("m_Events") {
                        Value::Sequence(
                            map.next_value::<Vec<UnityAnimationEvent>>()?
                                .into_iter()
                                .map(|v| v.0)
                                .collect(),
                        )
                    } else {
                        map.next_value::<UnityValue>()?.0
                    };
                    if values.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate mapping key"));
                    }
                }
                Ok(UnityValue(Value::Mapping(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

// Event string parameters are typed Unity strings even when the exported YAML
// scalar looks like a number/bool or is empty. Do not coerce script fields named
// "data": this visitor is used only for AnimationClip's m_Events array.
struct UnityAnimationEvent(Value);
impl<'de> Deserialize<'de> for UnityAnimationEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UnityAnimationEvent;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a Unity animation event")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = serde_yaml_ng::Mapping::new();
                while let Some(key) = map.next_key::<Value>()? {
                    let value = if matches!(key.as_str(), Some("data" | "functionName")) {
                        Value::String(map.next_value::<Option<String>>()?.unwrap_or_default())
                    } else {
                        map.next_value::<UnityValue>()?.0
                    };
                    if out.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate event key"));
                    }
                }
                Ok(UnityAnimationEvent(Value::Mapping(out)))
            }
        }
        d.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod t3_tests {
    use super::*;
    #[test]
    fn binary_yaml_fields_preserve_lexical_hex_even_when_they_look_like_numbers() {
        let f=UnityFile::parse("--- !u!48 &4800000\nShader:\n  compressedBlob: 001234\n  m_IndexBuffer: 001002\n  _typelessdata: 0000000000000000000000000000000000000000000000000000000000000000\n  m_ParameterBlobIndices: [00000000000000000000000000000000000000, ffffffff]\n").unwrap();
        let d = &f.objects[&4800000].data;
        assert_eq!(d["compressedBlob"].as_str(), Some("001234"));
        assert_eq!(d["m_IndexBuffer"].as_str(), Some("001002"));
        assert_eq!(d["_typelessdata"].as_str().unwrap().len(), 64);
        assert_eq!(d["m_ParameterBlobIndices"][0].as_str().unwrap().len(), 38);
    }
}
