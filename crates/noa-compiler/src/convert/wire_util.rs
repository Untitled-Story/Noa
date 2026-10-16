use crate::diagnostics::Error;
use noa_format::{ir, wire as w};
use serde_yaml_ng::Value as Y;

macro_rules! native {
    ($ty:ident { $($field:ident $(: $value:expr)?),* $(,)? }) => {{
        let mut value = noa_format::wire::$ty::default();
        $(value.$field = n!(@value $field $(: $value)?);)*
        value
    }};
    (@value $field:ident : $value:expr) => { $value };
    (@value $field:ident) => { $field };
}
pub(crate) use native as n;
pub(crate) fn err(at: &str, message: impl std::fmt::Display) -> Error {
    Error::new("UNSUPPORTED_EFFECT_DATA", format!("{at}: {message}"))
}
pub(crate) fn get<'a>(v: &'a Y, k: &str) -> Result<&'a Y, Error> {
    v.get(k).ok_or_else(|| err(k, "missing field"))
}
pub(crate) fn num(v: &Y, k: &str) -> Result<f32, Error> {
    let x = get(v, k)?
        .as_f64()
        .ok_or_else(|| err(k, "expected number"))?;
    let f = x as f32;
    if !x.is_finite() || !f.is_finite() {
        return Err(err(k, "expected finite float32"));
    }
    Ok(f)
}
pub(crate) fn int(v: &Y, k: &str) -> Result<i64, Error> {
    get(v, k)?
        .as_i64()
        .ok_or_else(|| err(k, "expected integer"))
}
pub(crate) fn uint(v: &Y, k: &str) -> Result<u32, Error> {
    u32::try_from(int(v, k)?).map_err(|_| err(k, "expected uint32"))
}
pub(crate) fn flag(v: &Y, k: &str) -> Result<bool, Error> {
    match get(v, k)? {
        Y::Bool(b) => Ok(*b),
        x if x.as_i64() == Some(0) => Ok(false),
        x if x.as_i64() == Some(1) => Ok(true),
        _ => Err(err(k, "expected bool or 0/1")),
    }
}
pub(crate) fn vec3(v: &Y) -> Result<w::Vec3T, Error> {
    Ok(n!(Vec3T {
        x: num(v, "x")?,
        y: num(v, "y")?,
        z: num(v, "z")?
    }))
}
pub(crate) fn vec2(v: &Y) -> Result<w::Vec2T, Error> {
    Ok(n!(Vec2T {
        x: num(v, "x")?,
        y: num(v, "y")?
    }))
}
pub(crate) fn constant(x: f32) -> w::ScalarParameterT {
    n!(ScalarParameterT {
        constant_max: x,
        constant_min: x
    })
}
pub(crate) fn vector(
    x: w::ScalarParameterT,
    y: w::ScalarParameterT,
    z: w::ScalarParameterT,
) -> w::VectorParameterT {
    n!(VectorParameterT {
        x: Some(Box::new(x)),
        y: Some(Box::new(y)),
        z: Some(Box::new(z))
    })
}
pub(crate) fn uniform(v: w::ScalarParameterT) -> w::VectorParameterT {
    vector(v.clone(), v.clone(), v)
}
pub(crate) fn enum_value<T: Copy>(v: i64, choices: &[T], at: &str) -> Result<T, Error> {
    usize::try_from(v)
        .ok()
        .and_then(|i| choices.get(i))
        .copied()
        .ok_or_else(|| err(at, format!("unsupported enum {v}")))
}
pub(crate) fn color(c: ir::Color) -> w::ColorT {
    n!(ColorT {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a
    })
}
pub(crate) fn color_space(c: ir::ColorSpace) -> w::ColorSpace {
    match c {
        ir::ColorSpace::Linear => w::ColorSpace::Linear,
        ir::ColorSpace::SRGB => w::ColorSpace::SRGB,
        ir::ColorSpace::Unspecified => w::ColorSpace::Unspecified,
    }
}
fn gradient(g: ir::Gradient) -> w::GradientT {
    n!(GradientT {
        mode: match g.mode {
            ir::GradientMode::Blend => w::GradientMode::Blend,
            ir::GradientMode::Fixed => w::GradientMode::Fixed,
            ir::GradientMode::PerceptualBlend => w::GradientMode::PerceptualBlend,
        },
        rgb: g
            .rgb
            .into_iter()
            .map(|k| n!(RgbKeyT {
                time: k.time,
                r: k.r,
                g: k.g,
                b: k.b
            }))
            .collect(),
        alpha: g
            .alpha
            .into_iter()
            .map(|k| n!(AlphaKeyT {
                time: k.time,
                alpha: k.alpha
            }))
            .collect(),
        key_color_space: color_space(g.key_color_space)
    })
}
pub(crate) fn color_param(c: ir::ColorParameter) -> w::ColorParameterT {
    n!(ColorParameterT {
        mode: match c.mode {
            ir::ColorMode::Constant => w::ColorMode::Constant,
            ir::ColorMode::Gradient => w::ColorMode::Gradient,
            ir::ColorMode::TwoColors => w::ColorMode::TwoColors,
            ir::ColorMode::TwoGradients => w::ColorMode::TwoGradients,
            ir::ColorMode::RandomColor => w::ColorMode::RandomColor,
        },
        color_min: c.color_min.map(color),
        color_max: c.color_max.map(color),
        gradient_min: c.gradient_min.map(|g| Box::new(gradient(g))),
        gradient_max: c.gradient_max.map(|g| Box::new(gradient(g)))
    })
}
