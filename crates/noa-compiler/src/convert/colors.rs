use super::EntityIds;
use crate::{diagnostics::Error, unity::UnityFile};
use noa_format::ir::{
    AlphaKey, Color, ColorMode, ColorParameter, ColorSpace, CustomStreamColor, Gradient,
    GradientMode, ParticleColors, RendererColors, RgbKey,
};
use serde_yaml_ng::Value;

#[derive(Debug, Default)]
pub struct ColorFragment {
    pub systems: Vec<ParticleColors>,
    pub renderers: Vec<RendererColors>,
}

fn error(at: &str, message: impl std::fmt::Display) -> Error {
    Error::new("INVALID_COLOR_DATA", format!("{at}: {message}"))
}

fn field<'a>(value: &'a Value, name: &str, at: &str) -> Result<&'a Value, Error> {
    value
        .get(name)
        .ok_or_else(|| error(at, format!("missing {name}")))
}

fn integer(value: &Value, name: &str, at: &str) -> Result<i64, Error> {
    field(value, name, at)?
        .as_i64()
        .ok_or_else(|| error(at, format!("{name} must be an integer")))
}

fn number(value: &Value, name: &str, at: &str) -> Result<f32, Error> {
    let number = field(value, name, at)?
        .as_f64()
        .ok_or_else(|| error(at, format!("{name} must be a number")))?;
    let result = number as f32;
    if !number.is_finite() || !result.is_finite() {
        return Err(error(at, format!("{name} must be finite float32")));
    }
    Ok(result)
}

fn flag(value: &Value, name: &str, at: &str) -> Result<bool, Error> {
    match field(value, name, at)? {
        Value::Bool(v) => Ok(*v),
        v if v.as_i64() == Some(0) => Ok(false),
        v if v.as_i64() == Some(1) => Ok(true),
        _ => Err(error(at, format!("{name} must be bool or 0/1"))),
    }
}

fn color(value: &Value, at: &str) -> Result<Color, Error> {
    // HDR and negative finite values are preserved, never clamped/premultiplied.
    Ok(Color {
        r: number(value, "r", at)?,
        g: number(value, "g", at)?,
        b: number(value, "b", at)?,
        a: number(value, "a", at)?,
    })
}

pub(crate) fn unity_color_space(value: i64, at: &str) -> Result<ColorSpace, Error> {
    match value {
        -1 => Ok(ColorSpace::Unspecified),
        0 => Ok(ColorSpace::SRGB),
        1 => Ok(ColorSpace::Linear),
        _ => Err(Error::new(
            "UNSUPPORTED_COLOR_SPACE",
            format!("{at}: unknown Unity color space {value}"),
        )),
    }
}

/// Converts only the active semantic branches. Inactive Unity fields are stale
/// serialized editor data, not additional gradients to evaluate or normalize.
pub fn convert_color_parameter(value: &Value, at: &str) -> Result<ColorParameter, Error> {
    let mode = match integer(value, "minMaxState", at)? {
        0 => ColorMode::Constant,
        1 => ColorMode::Gradient,
        2 => ColorMode::TwoColors,
        3 => ColorMode::TwoGradients,
        4 => ColorMode::RandomColor,
        unknown => {
            return Err(Error::new(
                "UNSUPPORTED_COLOR_MODE",
                format!("{at}.minMaxState: {unknown}"),
            ));
        }
    };
    let get_color = |name| color(field(value, name, at)?, &format!("{at}.{name}"));
    let get_gradient = |name| convert_gradient(field(value, name, at)?, &format!("{at}.{name}"));
    Ok(ColorParameter {
        mode,
        color_min: if mode == ColorMode::TwoColors {
            Some(get_color("minColor")?)
        } else {
            None
        },
        color_max: if matches!(mode, ColorMode::Constant | ColorMode::TwoColors) {
            Some(get_color("maxColor")?)
        } else {
            None
        },
        gradient_min: if mode == ColorMode::TwoGradients {
            Some(get_gradient("minGradient")?)
        } else {
            None
        },
        gradient_max: if matches!(
            mode,
            ColorMode::Gradient | ColorMode::TwoGradients | ColorMode::RandomColor
        ) {
            Some(get_gradient("maxGradient")?)
        } else {
            None
        },
    })
}

pub fn convert_gradient(value: &Value, at: &str) -> Result<Gradient, Error> {
    let mode = match integer(value, "m_Mode", at)? {
        0 => GradientMode::Blend,
        1 => GradientMode::Fixed,
        2 => GradientMode::PerceptualBlend,
        unknown => {
            return Err(Error::new(
                "UNSUPPORTED_GRADIENT_MODE",
                format!("{at}.m_Mode: {unknown}"),
            ));
        }
    };
    let key_color_space = unity_color_space(
        integer(value, "m_ColorSpace", at)?,
        &format!("{at}.m_ColorSpace"),
    )?;
    let count = |name| -> Result<usize, Error> {
        let v = integer(value, name, at)?;
        if !(1..=8).contains(&v) {
            return Err(error(at, format!("{name} must be 1..8")));
        }
        Ok(v as usize)
    };
    let time = |name: &str, previous: &mut Option<u16>| -> Result<f32, Error> {
        let t = u16::try_from(integer(value, name, at)?)
            .map_err(|_| error(at, format!("{name} must be uint16")))?;
        if previous.is_some_and(|v| t <= v) {
            return Err(error(
                at,
                format!("{name}: duplicate or decreasing gradient key times are unsupported"),
            ));
        }
        *previous = Some(t);
        Ok(f32::from(t) / 65535.0)
    };
    let mut rgb = Vec::new();
    let mut previous = None;
    for i in 0..count("m_NumColorKeys")? {
        let key = field(value, &format!("key{i}"), at)?;
        let path = format!("{at}.key{i}");
        rgb.push(RgbKey {
            time: time(&format!("ctime{i}"), &mut previous)?,
            r: number(key, "r", &path)?,
            g: number(key, "g", &path)?,
            b: number(key, "b", &path)?,
        });
    }
    let mut alpha = Vec::new();
    let mut previous = None;
    for i in 0..count("m_NumAlphaKeys")? {
        let key = field(value, &format!("key{i}"), at)?;
        alpha.push(AlphaKey {
            time: time(&format!("atime{i}"), &mut previous)?,
            alpha: number(key, "a", &format!("{at}.key{i}"))?,
        });
    }
    Ok(Gradient {
        mode,
        rgb,
        alpha,
        key_color_space,
    })
}

/// Extract color fragments without claiming the rest of a particle component is
/// supported. Pending components must remain pending until all modules convert.
pub fn convert_particle_colors(
    source: &str,
    file: &UnityFile,
    ids: &EntityIds,
) -> Result<ColorFragment, Error> {
    let mut result = ColorFragment::default();
    for object in file.objects.values() {
        let at = format!("{source} fileID={} {}", object.file_id, object.type_name);
        if object.type_name == "ParticleSystemRenderer" {
            if object.class_id != 199 || object.stripped {
                return Err(error(&at, "expected expanded ParticleSystemRenderer"));
            }
            result.renderers.push(RendererColors {
                renderer_component_id: ids.get(source, object.file_id)?,
                apply_active_color_space: flag(&object.data, "m_ApplyActiveColorSpace", &at)?,
            });
        } else if object.type_name == "ParticleSystem" {
            if object.class_id != 198 || object.stripped {
                return Err(error(&at, "expected expanded ParticleSystem"));
            }
            let data = &object.data;
            let initial = field(data, "InitialModule", &at)?;
            let start_color = convert_color_parameter(
                field(initial, "startColor", &at)?,
                &format!("{at}.InitialModule.startColor"),
            )?;
            let over = field(data, "ColorModule", &at)?;
            let over_lifetime = if flag(over, "enabled", &format!("{at}.ColorModule"))? {
                Some(convert_color_parameter(
                    field(over, "gradient", &at)?,
                    &format!("{at}.ColorModule.gradient"),
                )?)
            } else {
                None
            };
            let by_speed = field(data, "ColorBySpeedModule", &at)?;
            if flag(by_speed, "enabled", &format!("{at}.ColorBySpeedModule"))? {
                return Err(Error::new(
                    "UNSUPPORTED_COLOR_BY_SPEED",
                    format!("{at}.ColorBySpeedModule: conversion is not implemented"),
                ));
            }
            let custom = field(data, "CustomDataModule", &at)?;
            let mut custom_streams = Vec::new();
            if flag(custom, "enabled", &format!("{at}.CustomDataModule"))? {
                for slot in 0..2 {
                    match integer(
                        custom,
                        &format!("mode{slot}"),
                        &format!("{at}.CustomDataModule"),
                    )? {
                        0 | 1 => {} // disabled/vector streams have no active color data
                        2 => custom_streams.push(CustomStreamColor {
                            slot,
                            color: convert_color_parameter(
                                field(custom, &format!("color{slot}"), &at)?,
                                &format!("{at}.CustomDataModule.color{slot}"),
                            )?,
                        }),
                        other => {
                            return Err(Error::new(
                                "UNSUPPORTED_CUSTOM_DATA_MODE",
                                format!("{at}.CustomDataModule.mode{slot}: {other}"),
                            ));
                        }
                    }
                }
            }
            result.systems.push(ParticleColors {
                system_component_id: ids.get(source, object.file_id)?,
                start_color,
                over_lifetime,
                custom_streams,
            });
        }
    }
    Ok(result)
}
