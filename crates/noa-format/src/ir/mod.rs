//! Compiler-side IR. No Unity YAML or rendering backend types belong here.

mod animation;
mod color;
mod scene;
mod script;

pub use animation::{Animation, Animator, AnimatorLayer, AnimatorState, BoolKey, NodeActiveTrack};
pub use color::{
    AlphaKey, Color, ColorContext, ColorMode, ColorParameter, ColorSpace, CustomStreamColor,
    Gradient, GradientMode, ParticleColors, RendererColors, RgbKey,
};

pub use scene::{EntityId, Node, Quaternion, RectLayout, Transform, Vec2, Vec3};
pub use script::{
    CustomComponent, ReferenceKind, ReferenceValue, ScriptIdentity, SerializedField,
    SerializedValue, SourceReference, TypedReference,
};
