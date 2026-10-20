use super::*;
impl Resources<'_> {
    pub(crate) fn sprite_default(&mut self, key: (String, i64), ui: bool) -> Result<u32, Error> {
        if let Some(t) = self.targets.get(&key) {
            return Ok(t.id.get());
        }
        let id = self.allocate()?;
        let sid = self.allocate()?;
        let model = if ui {
            w::MaterialModel::UIDefault
        } else {
            w::MaterialModel::SpriteDefault
        };
        let recipe = json!({"format":"noa-shader-data","version":1,"revision":1,"model":if ui{"unity-ui-default"}else{"unity-sprites-default"},"source_revision":"unity-2022.3.21f1",
    "texture_source":"renderer-sprite-or-white","formula":if ui{"v = vertexColor (apply UIGammaToLinear when Canvas.vertex_color_gamma and linear project); v *= tint; v.a = round(v.a * 255) / 255; c = (sample + TextureSampleAdd) * v; c.a *= softClipRectCoverage; optional clip(c.a - 0.001); c.rgb *= c.a"}else{"c = sample * meshVertexColor * materialTint * SpriteRenderer.color; c.rgb *= c.a"},
    "canvas_depth_test":if ui {Some("ScreenOverlay=Always; ScreenCamera/World=LessEqual")}else{None},"premultiplied_output":true,"lighting":"unlit","stencil":if ui{"mask-stack"}else{"sprite-mask"}});
        self.asset(
            sid,
            w::AssetKind::ShaderData,
            "shader.json",
            serde_json::to_vec(&recipe).map_err(|e| err("shader recipe", e))?,
        );
        self.materials.insert(
            id,
            n!(MaterialT {
                id,
                model,
                tint: Some(n!(ColorT {
                    r: 1.,
                    g: 1.,
                    b: 1.,
                    a: 1.
                })),
                shader_data_asset_id: sid,
                render_state: Box::new(n!(RenderStateT {
                    src_rgb: w::BlendFactor::One,
                    dst_rgb: w::BlendFactor::OneMinusSrcAlpha,
                    src_alpha: w::BlendFactor::One,
                    dst_alpha: w::BlendFactor::OneMinusSrcAlpha,
                    cull: w::CullMode::None,
                    depth_test_source: if ui {
                        w::DepthTestSource::Canvas
                    } else {
                        w::DepthTestSource::Fixed
                    },
                    depth_test: w::CompareOp::LessEqual
                }))
            }),
        );
        self.target(key, id, ReferenceKind::Material);
        Ok(id)
    }
}
