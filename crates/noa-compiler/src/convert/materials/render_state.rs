//! Resolve ShaderLab material property bindings from saved values or shader defaults.
use super::*;
pub(super) fn convert(source: &Y, material: &Y, shader: &Y) -> Result<w::RenderStateT, Error> {
    let saved = &material["m_SavedProperties"]["m_Floats"];
    let props = shader["m_PropInfo"]["m_Props"]
        .as_sequence()
        .ok_or_else(|| err("Shader", "missing property declarations"))?;
    fn resolve(v: &mut Y, saved: &Y, props: &[Y]) -> Result<(), Error> {
        if let Some(m) = v.as_mapping_mut() {
            if m.contains_key("val") {
                if let Some(name) = m
                    .get("name")
                    .and_then(Y::as_str)
                    .filter(|n| !n.is_empty() && *n != "<noninit>")
                {
                    let p = saved
                        .get(name)
                        .or_else(|| {
                            props
                                .iter()
                                .find(|p| p["m_Name"].as_str() == Some(name))
                                .and_then(|p| p.get("m_DefValue[0]"))
                        })
                        .and_then(Y::as_f64)
                        .ok_or_else(|| err("render state", format!("missing binding {name}")))?;
                    if !p.is_finite()
                        || p.fract() != 0.
                        || !(-2147483648. ..=2147483647.).contains(&p)
                    {
                        return Err(err("render state", "binding must be finite int32"));
                    }
                    m.insert("val".into(), (p as i64).into());
                    m.insert("name".into(), "<noninit>".into());
                }
            } else {
                for (k, v) in m.iter_mut() {
                    // Fog is not consumed by the verified simple unlit programs.
                    if !k.as_str().is_some_and(|k| k.starts_with("fog")) {
                        resolve(v, saved, props)?;
                    }
                }
            }
        }
        Ok(())
    }
    let mut state = source.clone();
    resolve(&mut state, saved, props)?;
    let mut out = super::render_state(&state)?;
    let intval = |v: &Y, k: &str| int(get(v, k)?, "val");
    let compare = |v| {
        enum_value(
            v,
            &[
                w::CompareOp::Always,
                w::CompareOp::Never,
                w::CompareOp::Less,
                w::CompareOp::Equal,
                w::CompareOp::LessEqual,
                w::CompareOp::Greater,
                w::CompareOp::NotEqual,
                w::CompareOp::GreaterEqual,
                w::CompareOp::Always,
            ],
            "stencil compare",
        )
    };
    let face = |v: &Y| -> Result<w::StencilFaceT, Error> {
        let op = |k| {
            enum_value(
                intval(v, k)?,
                &[
                    w::StencilOp::Keep,
                    w::StencilOp::Zero,
                    w::StencilOp::Replace,
                    w::StencilOp::IncrementSaturate,
                    w::StencilOp::DecrementSaturate,
                    w::StencilOp::Invert,
                    w::StencilOp::IncrementWrap,
                    w::StencilOp::DecrementWrap,
                ],
                "stencil operation",
            )
        };
        Ok(n!(StencilFaceT {
            compare: compare(intval(v, "comp")?)?,
            pass: op("pass")?,
            fail: op("fail")?,
            depth_fail: op("zFail")?
        }))
    };
    let common = face(get(&state, "stencilOp")?)?;
    let front = face(get(&state, "stencilOpFront")?)?;
    let back = face(get(&state, "stencilOpBack")?)?;
    let default = w::StencilFaceT::default();
    if front != default || back != default {
        return Err(err(
            "stencil",
            "separate face source states need a verified conversion",
        ));
    }
    let byte = |k| u8::try_from(intval(&state, k)?).map_err(|_| err("stencil", "outside uint8"));
    if intval(get(&state, "stencilOp")?, "comp")? != 0 {
        out.stencil = Some(Box::new(n!(StencilStateT {
            reference: byte("stencilRef")?,
            read_mask: byte("stencilReadMask")?,
            write_mask: byte("stencilWriteMask")?,
            front: Box::new(common.clone()),
            back: Box::new(common)
        })));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn property_defaults_are_used_but_fractional_and_out_of_range_states_are_rejected() {
        let mut v:Y=serde_yaml_ng::from_str("rtBlend0: {srcBlend: {val: 5}, destBlend: {val: 1}, srcBlendAlpha: {val: 5}, destBlendAlpha: {val: 1}, blendOp: {val: 0}, blendOpAlpha: {val: 0}, colMask: {val: 15}}\nzTest: {val: 0, name: _ZTest}\nzWrite: {val: 0}\nculling: {val: 0}\nstencilOp: {comp: {val: 8}, pass: {val: 0}, fail: {val: 0}, zFail: {val: 0}}\nstencilOpFront: {comp: {val: 8}, pass: {val: 0}, fail: {val: 0}, zFail: {val: 0}}\nstencilOpBack: {comp: {val: 8}, pass: {val: 0}, fail: {val: 0}, zFail: {val: 0}}\nstencilRef: {val: 0}\nstencilReadMask: {val: 255}\nstencilWriteMask: {val: 255}\n").unwrap();
        let mut material: Y = serde_yaml_ng::from_str("m_SavedProperties: {m_Floats: {}}").unwrap();
        let shader: Y = serde_yaml_ng::from_str(
            "m_PropInfo: {m_Props: [{m_Name: _ZTest, 'm_DefValue[0]': 4}]}",
        )
        .unwrap();
        assert_eq!(
            convert(&v, &material, &shader).unwrap().depth_test,
            w::CompareOp::LessEqual
        );
        material["m_SavedProperties"]["m_Floats"]["_ZTest"] = 0.5.into();
        assert!(convert(&v, &material, &shader).is_err());
        material["m_SavedProperties"]["m_Floats"]["_ZTest"] = 8.into();
        assert_eq!(
            convert(&v, &material, &shader).unwrap().depth_test,
            w::CompareOp::Always
        );
        v["stencilWriteMask"]["val"] = 256.into();
        assert!(convert(&v, &material, &shader).is_err());
    }
}
