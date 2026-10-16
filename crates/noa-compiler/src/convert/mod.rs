//! Semantic conversion. Fragments cannot be emitted as complete NOIR packages.

mod animator;
mod colors;
mod ids;
mod scene;
mod scripts;

pub use animator::{AnimatorPlan, UnitySources, plan_animators};
pub(crate) use colors::unity_color_space;
pub use colors::{
    ColorFragment, convert_color_parameter, convert_gradient, convert_particle_colors,
};
pub use ids::{EntityIds, SourceIdentity};
pub use scene::{PendingComponent, SceneFragment, convert_scene};
pub use scripts::{CustomFragment, convert_custom_components};

pub mod curves;
pub mod particles;
pub(crate) mod wire_util;

pub(crate) mod materials;

pub(crate) mod document;
pub(crate) mod renderers;
