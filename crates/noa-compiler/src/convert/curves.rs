//! Unity serialized MinMaxCurve conversion. No evaluation or resampling.
use super::wire_util::*;
use crate::diagnostics::Error;
use noa_format::wire as w;
use serde_yaml_ng::Value as Y;

pub fn scalar(v: &Y, domain: w::CurveDomain) -> Result<w::ScalarParameterT, Error> {
    let mode = int(v, "minMaxState")?;
    let mut p = constant(num(v, "scalar")?);
    match mode {
        0 => {}
        1 | 2 => {
            p.mode = if mode == 1 {
                w::ScalarMode::Curve
            } else {
                w::ScalarMode::TwoCurves
            };
            p.multiplier = p.constant_max;
            p.constant_min = 0.;
            p.constant_max = 0.;
            p.curve_max = Some(Box::new(curve(get(v, "maxCurve")?, domain)?));
            if mode == 2 {
                p.curve_min = Some(Box::new(curve(get(v, "minCurve")?, domain)?));
            }
        }
        3 => {
            p.mode = w::ScalarMode::TwoConstants;
            p.constant_min = num(v, "minScalar")?;
        }
        _ => return Err(err("minMaxState", mode)),
    }
    Ok(p)
}
pub fn curve(v: &Y, domain: w::CurveDomain) -> Result<w::ScalarCurveT, Error> {
    let mut keys = Vec::new();
    for k in get(v, "m_Curve")?
        .as_sequence()
        .ok_or_else(|| err("m_Curve", "expected keys"))?
    {
        let time = num(k, "time")?;
        if keys.last().is_some_and(|k: &w::FloatKeyT| k.time >= time) {
            return Err(err("m_Curve", "times must strictly increase"));
        }
        let tangent = |name| slope(get(k, name)?);
        let incoming = tangent("inSlope")?;
        let outgoing = tangent("outSlope")?;
        let weighted = int(k, "weightedMode")?;
        if !(0..=3).contains(&weighted) {
            return Err(err("weightedMode", weighted));
        }
        let wi = num(k, "inWeight")?;
        let wo = num(k, "outWeight")?;
        if !(0. ..=1.).contains(&wi) || !(0. ..=1.).contains(&wo) {
            return Err(err("weight", "outside 0..1"));
        }
        keys.push(n!(FloatKeyT {
            time,
            value: num(k, "value")?,
            in_slope: if incoming.is_infinite() { 0. } else { incoming },
            out_slope: if outgoing.is_infinite() { 0. } else { outgoing },
            in_weight: wi,
            out_weight: wo,
            weighted_in: weighted & 1 != 0,
            weighted_out: weighted & 2 != 0,
            interpolation: if outgoing.is_infinite() {
                w::Interpolation::Step
            } else {
                w::Interpolation::CubicHermite
            }
        }));
    }
    if keys.is_empty() {
        return Err(err("m_Curve", "active curve is empty"));
    }
    // A segment may be stepped by the next key's incoming tangent. Weighted
    // interpolation uses the left out / right in flags, not the editor tangent mode.
    for i in 0..keys.len().saturating_sub(1) {
        let source = get(v, "m_Curve")?.as_sequence().unwrap();
        if slope(&source[i + 1]["inSlope"])?.is_infinite() {
            keys[i].interpolation = w::Interpolation::Step;
        } else if keys[i].interpolation != w::Interpolation::Step
            && (keys[i].weighted_out || keys[i + 1].weighted_in)
        {
            keys[i].interpolation = w::Interpolation::WeightedBezier;
        }
    }
    let pre = i32::try_from(int(v, "m_PreInfinity")?)
        .map_err(|_| err("m_PreInfinity", "outside int32"))?;
    let post = i32::try_from(int(v, "m_PostInfinity")?)
        .map_err(|_| err("m_PostInfinity", "outside int32"))?;
    Ok(n!(ScalarCurveT {
        domain,
        pre_wrap: w::WrapMode::UnitySerialized,
        post_wrap: w::WrapMode::UnitySerialized,
        source_pre_infinity: pre,
        source_post_infinity: post,
        keys
    }))
}

// AssetRipper emits bare Infinity/-Infinity, which YAML treats as strings.
// Accept only the exact exported tokens; other strings and NaN are errors.
fn slope(v: &Y) -> Result<f32, Error> {
    let x = match v.as_str() {
        Some("Infinity" | "+Infinity") => f64::INFINITY,
        Some("-Infinity") => f64::NEG_INFINITY,
        Some(_) => return Err(err("slope", "invalid slope token")),
        None => v.as_f64().ok_or_else(|| err("slope", "expected slope"))?,
    };
    if x.is_nan() || (x.is_finite() && !(x as f32).is_finite()) {
        return Err(err("slope", "invalid slope"));
    }
    Ok(x as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_infinity_tangents_become_finite_step_segments() {
        let mut c: Y = serde_yaml_ng::from_str(
            "m_PreInfinity: 2\nm_PostInfinity: 2\nm_Curve:\n- {time: 0, value: 1, inSlope: 0, outSlope: 0, weightedMode: 0, inWeight: 0, outWeight: 0}\n- {time: 1, value: 2, inSlope: Infinity, outSlope: -Infinity, weightedMode: 0, inWeight: 0, outWeight: 0}\n"
        ).unwrap();
        let parsed = curve(&c, w::CurveDomain::Seconds).unwrap();
        assert_eq!(parsed.keys[0].interpolation, w::Interpolation::Step);
        assert_eq!(parsed.keys[1].in_slope, 0.0);
        assert_eq!(parsed.keys[1].out_slope, 0.0);
        for token in ["NaN", "inf", "unknown"] {
            c["m_Curve"][1]["inSlope"] = token.into();
            assert!(curve(&c, w::CurveDomain::Seconds).is_err());
        }
    }
}
