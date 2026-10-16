use super::EntityId;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ScriptIdentity {
    pub assembly: String,
    pub namespace: String,
    pub class_name: String,
}

/// Provenance for an unresolved reference, never a substitute for a target ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceReference {
    pub guid: Option<String>,
    pub file_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Node,
    Component,
    Material,
    Asset,
    Animation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TypedReference {
    pub kind: ReferenceKind,
    pub id: EntityId,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReferenceValue {
    pub source: SourceReference,
    /// None means unresolved, and is distinct from SerializedValue::Null.
    pub target: Option<TypedReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SerializedField {
    /// C# declared type, only when available from authoritative type metadata.
    pub declared_type: Option<String>,
    pub value: SerializedValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SerializedValue {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Array(Vec<SerializedValue>),
    Object(BTreeMap<String, SerializedField>),
    Reference(ReferenceValue),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CustomComponent {
    pub id: EntityId,
    pub node_id: EntityId,
    pub enabled: bool,
    /// Missing or conflicting identity is represented explicitly, never guessed.
    pub script: Option<ScriptIdentity>,
    pub source_script: SourceReference,
    pub fields: BTreeMap<String, SerializedField>,
}
