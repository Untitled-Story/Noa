//! Exact Unity builtin GUID/fileID recipes, independent of material names.
//! Unity 2022.3.21f1 DefaultResourcesExtra/Particle Add.shader and
//! Particle Alpha Blend.shader define the formula and immutable render state.
use super::*;

impl Resources<'_> {
    pub(super) fn legacy_particle_material(
        &mut self,
        key: (String, i64),
        id: u32,
        v: &Y,
        shader_key: (String, i64),
    ) -> Result<u32, Error> {
        let (model, name, dst) = builtin(&shader_key)?;
        // Material-local variants which change the program must be understood first.
        for field in ["m_ValidKeywords", "disabledShaderPasses"] {
            if v.get(field)
                .and_then(Y::as_sequence)
                .is_some_and(|x| !x.is_empty())
            {
                return Err(err(field, "builtin particle variant not supported"));
            }
        }
        if v.get("m_ShaderKeywords")
            .and_then(Y::as_str)
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(err(
                "m_ShaderKeywords",
                "builtin particle variant not supported",
            ));
        }
        let sid = if let Some(target) = self.targets.get(&shader_key) {
            target.id.get()
        } else {
            let sid = self.allocate()?;
            let recipe = json!({
                "format":"noa-shader-data", "version":1, "revision":1,
                "model": if model == w::MaterialModel::LegacyParticleAdditive {"unity-legacy-particle-additive"} else {"unity-legacy-particle-alpha-blend"},
                "source_builtin":{"guid":"0000000000000000f000000000000000","file_id":shader_key.1.to_string()},
                "source_revision":"unity-2022.3.21f1",
                "color_formula":"c = 2 * vertexColor * TintColor * texture(MainTex, uv); c.a = saturate(c.a)",
                "rgb_premultiply":false,"output_policy":"primary-color","outputs":[0],
                "environment":{
                    "fog":{"host_slot":"source-scene-fog","required":true,
                        "target":if dst == w::BlendFactor::One {"black"} else {"scene-fog-color"}},
                    "soft_particles":{"host_slot":"source-soft-particles","required":true,
                        "formula":"vertexColor.a *= saturate(InvFade * (sceneEyeDepth - particleEyeDepth))",
                        "depth_required_when_enabled":true}
                }
            });
            self.asset(
                sid,
                w::AssetKind::ShaderData,
                "shader.json",
                serde_json::to_vec(&recipe).map_err(|e| err("builtin shader data", e))?,
            );
            self.target(shader_key, sid, ReferenceKind::Asset);
            sid
        };
        let saved = get(v, "m_SavedProperties")?;
        let tint = if let Some(t) = saved.get("m_Colors").and_then(|c| c.get("_TintColor")) {
            n!(ColorT {
                r: num(t, "r")?,
                g: num(t, "g")?,
                b: num(t, "b")?,
                a: num(t, "a")?
            })
        } else {
            n!(ColorT {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 0.5
            })
        };
        let binding = saved.get("m_TexEnvs").and_then(|t| t.get("_MainTex"));
        let r = binding.map(|b| get(b, "m_Texture")).transpose()?;
        let (image_id, space, sampler) =
            if r.map(|r| int(r, "fileID")).transpose()?.unwrap_or(0) == 0 {
                (
                    self.white_image()?,
                    w::ColorSpace::SRGB,
                    n!(SamplerT {
                        wrap_u: w::AddressMode::Repeat,
                        wrap_v: w::AddressMode::Repeat
                    }),
                )
            } else {
                self.image(&key.0, r.expect("non-null image reference"))?
            };
        let queue = int(v, "m_CustomRenderQueue")?;
        let inv_fade = saved
            .get("m_Floats")
            .and_then(|f| f.get("_InvFade"))
            .map(|_| num(get(saved, "m_Floats")?, "_InvFade"))
            .transpose()?
            .unwrap_or(1.);
        let material = n!(MaterialT {
            id,
            name: v.get("m_Name").and_then(Y::as_str).map(str::to_owned),
            model,
            tint: Some(tint),
            textures: Some(vec![n!(TextureBindingT {
                slot: "_MainTex".into(),
                image_id,
                scale: Some(
                    binding
                        .map(|b| vec2(get(b, "m_Scale")?))
                        .transpose()?
                        .unwrap_or_else(|| n!(Vec2T { x: 1., y: 1. }))
                ),
                offset: Some(
                    binding
                        .map(|b| vec2(get(b, "m_Offset")?))
                        .transpose()?
                        .unwrap_or_default()
                ),
                color_space: space,
                sampler: Some(Box::new(sampler))
            })]),
            floats: Some(vec![n!(FloatPropertyT {
                name: "_InvFade".into(),
                value: inv_fade
            })]),
            render_state: Box::new(n!(RenderStateT {
                src_rgb: w::BlendFactor::SrcAlpha,
                dst_rgb: dst,
                src_alpha: w::BlendFactor::SrcAlpha,
                dst_alpha: dst,
                depth_test: w::CompareOp::LessEqual,
                cull: w::CullMode::None,
                color_write_mask: 7
            })),
            render_queue: if queue == -1 {
                3000
            } else {
                i32::try_from(queue).map_err(|_| err("queue", "outside int32"))?
            },
            source_shader: Some(name.into()),
            shader_data_asset_id: sid
        });
        self.materials.insert(id, material);
        self.target(key, id, ReferenceKind::Material);
        Ok(id)
    }
    fn white_image(&mut self) -> Result<u32, Error> {
        let key = ("generated/solid-white".into(), 1);
        if let Some(target) = self.targets.get(&key) {
            return Ok(target.id.get());
        }
        let id = self.allocate()?;
        self.asset(
            id,
            w::AssetKind::Image,
            "png",
            include_bytes!("../../../resources/white.png").to_vec(),
        );
        self.target(key, id, ReferenceKind::Asset);
        Ok(id)
    }
}
fn builtin(key: &(String, i64)) -> Result<(w::MaterialModel, &'static str, w::BlendFactor), Error> {
    if key.0 != "builtin/0000000000000000f000000000000000" {
        return Err(err("builtin shader", format!("unknown {key:?}")));
    }
    match key.1 {
        10720 => Ok((
            w::MaterialModel::LegacyParticleAdditive,
            "Legacy Shaders/Particles/Additive",
            w::BlendFactor::One,
        )),
        10721 => Ok((
            w::MaterialModel::LegacyParticleAlphaBlend,
            "Legacy Shaders/Particles/Alpha Blended",
            w::BlendFactor::OneMinusSrcAlpha,
        )),
        _ => Err(err("builtin shader", format!("unknown {key:?}"))),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_shader_identity_requires_both_guid_and_file_id() {
        assert!(builtin(&("builtin/0000000000000000f000000000000000".into(), 10720)).is_ok());
        assert!(builtin(&("builtin/0000000000000000e000000000000000".into(), 10720)).is_err());
        assert!(builtin(&("builtin/0000000000000000f000000000000000".into(), 999)).is_err());
    }
}
