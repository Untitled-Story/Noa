use serde::Serialize;
use std::num::NonZeroU32;

/// Nonzero, package-local identity. This is never a vector index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct EntityId(NonZeroU32);

impl EntityId {
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// xyzw, in the coordinate convention of the NOIR schema.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quaternion,
    pub scale: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    pub id: EntityId,
    pub name: String,
    pub parent_id: Option<EntityId>,
    /// The node's own initial flag, independent of ancestors and animation.
    pub active: bool,
    pub transform: Transform,
    pub layer_mask: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RectLayout {
    pub id: EntityId,
    pub node_id: EntityId,
    pub anchor_min: Vec2,
    pub anchor_max: Vec2,
    pub pivot: Vec2,
    pub anchored_position: Vec3,
    pub size_delta: Vec2,
}
