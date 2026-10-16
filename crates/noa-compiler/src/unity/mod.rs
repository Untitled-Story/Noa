mod scene;
mod yaml;

pub use yaml::{Guid, Object, ObjectReference, ReferenceKind, UnityFile, parse_meta_guid};

pub use scene::{SceneNode, scene_nodes};

pub(crate) fn is_script_engine_field(name: &str) -> bool {
    matches!(
        name,
        "m_ObjectHideFlags"
            | "m_CorrespondingSourceObject"
            | "m_PrefabInstance"
            | "m_PrefabAsset"
            | "m_GameObject"
            | "m_Enabled"
            | "m_EditorHideFlags"
            | "m_Script"
            | "m_Name"
            | "m_EditorClassIdentifier"
    )
}
