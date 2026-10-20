use super::EntityId;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Animation {
    pub id: EntityId,
    pub name: String,
    pub duration_seconds: f32,
    pub looped: bool,
    pub tracks: Vec<crate::wire::TrackT>,
    pub events: Vec<crate::wire::AnimationEventT>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimatorState {
    pub id: EntityId,
    pub name: String,
    pub animation_id: EntityId,
    pub speed: f32,
    pub cycle_offset: f32,
    pub write_default_values: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimatorLayer {
    pub name: String,
    pub default_weight: f32,
    pub default_state_id: EntityId,
    pub states: Vec<AnimatorState>,
}

/// Initial scope: one normal-update layer, no transitions, avatar or root motion.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Animator {
    pub id: EntityId,
    pub node_id: EntityId,
    pub enabled: bool,
    pub keep_state_on_disable: bool,
    pub write_default_values_on_disable: bool,
    pub layers: Vec<AnimatorLayer>,
}
