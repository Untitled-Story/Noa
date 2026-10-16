//! Authored colors and interpolation metadata. The compiler does not evaluate
//! gradients or change RGB/alpha values into the host renderer's color space.

use super::EntityId;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    Linear,
    #[serde(rename = "srgb")]
    SRGB,
    Unspecified,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColorContext {
    pub project_color_space: ColorSpace,
    pub host_project_color_space_slot: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientMode {
    Blend,
    Fixed,
    PerceptualBlend,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RgbKey {
    pub time: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AlphaKey {
    pub time: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gradient {
    pub mode: GradientMode,
    pub rgb: Vec<RgbKey>,
    pub alpha: Vec<AlphaKey>,
    pub key_color_space: ColorSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    Constant,
    Gradient,
    TwoColors,
    TwoGradients,
    RandomColor,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColorParameter {
    pub mode: ColorMode,
    pub color_min: Option<Color>,
    pub color_max: Option<Color>,
    pub gradient_min: Option<Gradient>,
    pub gradient_max: Option<Gradient>,
}

/// Partial ParticleSystem conversion, not a complete component.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParticleColors {
    pub system_component_id: EntityId,
    pub start_color: ColorParameter,
    pub over_lifetime: Option<ColorParameter>,
    pub custom_streams: Vec<CustomStreamColor>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CustomStreamColor {
    pub slot: u8,
    pub color: ColorParameter,
}

/// Partial ParticleRenderer conversion; shader/render-state conversion follows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RendererColors {
    pub renderer_component_id: EntityId,
    pub apply_active_color_space: bool,
}
