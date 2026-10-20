//! T3 resource and renderer semantic validation (independent of GPU execution).
use crate::wire as w;
fn feature(d: &w::DocumentT, name: &str, revision: u32) -> Result<(), String> {
    if d.features
        .iter()
        .flatten()
        .any(|f| f.name == name && f.required && f.revision == revision)
    {
        Ok(())
    } else {
        Err(format!("missing required {name}@{revision}"))
    }
}
pub(crate) fn validate(d: &w::DocumentT) -> Result<(), String> {
    let asset = |id, kind| {
        d.assets
            .iter()
            .flatten()
            .any(|a| a.id == id && a.kind == kind)
    };
    let sprite = |id| {
        if id == 0 || asset(id, w::AssetKind::Sprite) {
            Ok(())
        } else {
            Err("invalid Sprite asset reference".to_owned())
        }
    };
    let materials = |ids: &[u32]| -> Result<(), String> {
        if ids
            .iter()
            .any(|id| *id != 0 && !d.materials.iter().flatten().any(|m| m.id == *id))
        {
            Err("invalid renderer materials".into())
        } else {
            Ok(())
        }
    };
    for a in d.assets.iter().flatten() {
        if a.kind == w::AssetKind::Mesh {
            feature(d, "mesh.nomesh", 2)?;
        }
        if a.kind == w::AssetKind::Sprite {
            feature(d, "asset.sprite", 1)?;
            let s = a.sprite.as_ref().ok_or("Sprite descriptor absent")?;
            if !asset(s.mesh_asset_id, w::AssetKind::Mesh)
                || !asset(s.image_asset_id, w::AssetKind::Image)
            {
                return Err("invalid Sprite dependencies".into());
            }
            if s.pixels_per_unit <= 0.
                || s.downscale_multiplier <= 0.
                || s.rect.is_none()
                || s.pivot.is_none()
                || s.border.is_none()
                || s.texture_rect.is_none()
                || s.sampler.is_none()
            {
                return Err("invalid/incomplete Sprite metadata".into());
            }
            // Unity exports can place the normalized pivot outside the rect.
            // Preserve it; global finite validation still rejects NaN/Infinity.
            let rect = s.rect.as_ref().unwrap();
            if rect.z <= 0. || rect.w <= 0. {
                return Err("invalid Sprite rect".into());
            }
        } else if a.sprite.is_some() {
            return Err("Sprite descriptor on wrong asset kind".into());
        }
    }
    for c in d.components.iter().flatten() {
        if matches!(
            c.data,
            w::ComponentDataT::MeshRenderer(_)
                | w::ComponentDataT::SpriteRenderer(_)
                | w::ComponentDataT::UIImage(_)
        ) {
            feature(d, "color.authored", 1)?;
            if d.color_context.is_none() {
                return Err("renderer needs source color context".into());
            }
        }
        if matches!(
            c.data,
            w::ComponentDataT::Canvas(_)
                | w::ComponentDataT::UIImage(_)
                | w::ComponentDataT::RectMask2D(_)
        ) && !d
            .components
            .iter()
            .flatten()
            .any(|v| v.node_id == c.node_id && matches!(v.data, w::ComponentDataT::RectLayout(_)))
        {
            return Err("UI component needs RectLayout".into());
        }
        match &c.data {
            w::ComponentDataT::MeshFilter(f) => {
                feature(d, "scene.mesh-renderer", 1)?;
                if f.mesh_asset_id != 0 && !asset(f.mesh_asset_id, w::AssetKind::Mesh) {
                    return Err("invalid MeshFilter geometry".into());
                }
            }
            w::ComponentDataT::MeshRenderer(r) => {
                feature(d, "scene.mesh-renderer", 1)?;
                materials(&r.materials)?;
                environment(d, r.environment.as_deref())?;
                if !d.components.iter().flatten().any(|f| {
                    f.id == r.mesh_filter_component_id
                        && f.node_id == c.node_id
                        && matches!(f.data, w::ComponentDataT::MeshFilter(_))
                }) {
                    return Err("MeshRenderer needs own MeshFilter".into());
                }
            }
            w::ComponentDataT::SpriteRenderer(r) => {
                feature(d, "scene.sprite-renderer", 1)?;
                sprite(r.sprite_asset_id)?;
                materials(&r.materials)?;
                if r.draw_mode < w::SpriteDrawMode::Simple
                    || r.draw_mode > w::SpriteDrawMode::Tiled
                    || r.tile_mode > 1
                    || !(0. ..=1.).contains(&r.adaptive_mode_threshold)
                    || r.color.is_none()
                    || r.size.is_none()
                {
                    return Err("invalid SpriteRenderer".into());
                }
                if ![w::SpriteSortPoint::Center, w::SpriteSortPoint::Pivot].contains(&r.sort_point)
                    || r.size.as_ref().is_some_and(|s| s.x < 0. || s.y < 0.)
                {
                    return Err("invalid SpriteRenderer size/sort point".into());
                }
                mask(d, r.mask_interaction)?;
            }
            w::ComponentDataT::SpriteMask(m) => {
                feature(d, "render.sprite-mask", 1)?;
                sprite(m.sprite_asset_id)?;
                if !(0. ..=1.).contains(&m.alpha_cutoff) {
                    return Err("invalid mask cutoff".into());
                }
                if ![w::SpriteSortPoint::Center, w::SpriteSortPoint::Pivot].contains(&m.sort_point)
                {
                    return Err("invalid SpriteMask sort point".into());
                }
                if m.custom_range
                    && (m.back_layer_index, m.back_order) > (m.front_layer_index, m.front_order)
                {
                    return Err("inverted mask sorting range".into());
                }
            }
            w::ComponentDataT::ParticleRenderer(r) => {
                if r.mode < w::RenderMode::Billboard || r.mode > w::RenderMode::None {
                    return Err("invalid particle render mode".into());
                }
                if r.mode == w::RenderMode::None {
                    feature(d, "particle.renderer-none", 1)?;
                }
                mask(d, r.mask_interaction)?;
                environment(d, r.environment.as_deref())?;
            }
            w::ComponentDataT::ParticleSystem(p) => {
                for m in p.modules.iter().flatten() {
                    if let w::ParticleModuleDataT::TextureSheetModule(t) = &m.data {
                        if t.mode == w::SheetMode::Sprites {
                            feature(d, "particle.texture-sheet-sprites", 1)?;
                            let ids = t.sprite_asset_ids.as_ref().ok_or("missing sheet sprites")?;
                            if ids.is_empty() {
                                return Err("empty sheet sprites".into());
                            }
                            for id in ids {
                                if *id == 0 {
                                    return Err("null sheet Sprite".into());
                                }
                                sprite(*id)?;
                            }
                        } else if t.mode != w::SheetMode::Grid {
                            return Err("invalid sheet mode".into());
                        }
                    }

                    if let w::ParticleModuleDataT::ShapeModule(s) = &m.data
                        && let w::ShapeGeometryT::SpriteShape(s) = &s.geometry
                    {
                        if s.sprite_asset_id == 0 {
                            return Err("missing shape Sprite".into());
                        }
                        sprite(s.sprite_asset_id)?;
                    }
                }
            }
            w::ComponentDataT::Canvas(v) => {
                feature(d, "ui.canvas", 1)?;
                if v.mode < w::CanvasMode::ScreenOverlay || v.mode > w::CanvasMode::World {
                    return Err("invalid canvas mode".into());
                }
                if v.plane_distance < 0. || v.additional_shader_channels & !31 != 0 {
                    return Err("invalid canvas plane/channels".into());
                }
                if v.camera_component_id != 0
                    && !d.components.iter().flatten().any(|c| {
                        c.id == v.camera_component_id
                            && matches!(c.data, w::ComponentDataT::Camera(_))
                    })
                {
                    return Err("invalid Canvas camera".into());
                }
                if v.mode == w::CanvasMode::ScreenCamera
                    && v.camera_component_id == 0
                    && v.host_camera_slot.as_deref().is_none_or(str::is_empty)
                {
                    return Err("camera canvas needs host camera".into());
                }
            }
            w::ComponentDataT::CanvasRenderer(_) => {
                feature(d, "ui.canvas", 1)?;
            }
            w::ComponentDataT::UIImage(i) => {
                feature(d, "ui.canvas", 1)?;
                sprite(i.sprite_asset_id)?;
                sprite(i.override_sprite_asset_id)?;
                materials(&[i.material_id])?;
                if i.type_ < w::ImageType::Simple
                    || i.type_ > w::ImageType::Filled
                    || i.fill_method > 4
                    || i.fill_origin > 3
                    || !(0. ..=1.).contains(&i.fill_amount)
                    || i.pixels_per_unit_multiplier <= 0.
                    || i.color.is_none()
                {
                    return Err("invalid UI Image".into());
                }
                if !d.components.iter().flatten().any(|v| {
                    v.node_id == c.node_id && matches!(v.data, w::ComponentDataT::CanvasRenderer(_))
                }) {
                    return Err("UI Image needs CanvasRenderer".into());
                }
            }
            w::ComponentDataT::UIMask(m) => {
                feature(d, "ui.canvas", 1)?;
                if m.graphic_component_id != 0
                    && !d.components.iter().flatten().any(|v| {
                        v.id == m.graphic_component_id
                            && v.node_id == c.node_id
                            && matches!(v.data, w::ComponentDataT::UIImage(_))
                    })
                {
                    return Err("UI Mask needs supported Graphic".into());
                }
            }
            w::ComponentDataT::RectMask2D(m) => {
                feature(d, "ui.canvas", 1)?;
                if m.padding.is_none() || m.softness.as_ref().is_none_or(|s| s.x < 0. || s.y < 0.) {
                    return Err("invalid RectMask2D".into());
                }
            }
            w::ComponentDataT::CanvasGroup(g) => {
                feature(d, "ui.canvas", 1)?;
                if !(0. ..=1.).contains(&g.alpha) {
                    return Err("invalid CanvasGroup alpha".into());
                }
            }
            _ => {}
        }
    }
    for m in d.materials.iter().flatten() {
        if let Some(s) = &m.render_state.stencil {
            for f in [&s.front, &s.back] {
                if f.compare < w::CompareOp::Never
                    || f.compare > w::CompareOp::Always
                    || [f.pass, f.fail, f.depth_fail]
                        .iter()
                        .any(|o| *o < w::StencilOp::Keep || *o > w::StencilOp::DecrementWrap)
                {
                    return Err("invalid stencil state".into());
                }
            }
        }
        if m.model == w::MaterialModel::TextureVertexColor {
            feature(d, "material.texture-vertex-color", 1)?;
        }
        if m.render_state.depth_test_source != w::DepthTestSource::Fixed
            && (m.render_state.depth_test_source != w::DepthTestSource::Canvas
                || m.model != w::MaterialModel::UIDefault)
        {
            return Err("invalid dynamic depth source".into());
        }
        if m.model == w::MaterialModel::SpriteDefault {
            feature(d, "material.unity-sprite-default", 1)?;
        }
        if m.model == w::MaterialModel::UIDefault {
            feature(d, "material.unity-ui-default", 1)?;
        }
    }
    Ok(())
}
fn mask(d: &w::DocumentT, v: w::MaskInteraction) -> Result<(), String> {
    if v < w::MaskInteraction::None || v > w::MaskInteraction::VisibleOutside {
        return Err("invalid mask interaction".into());
    }
    if v != w::MaskInteraction::None {
        feature(d, "render.sprite-mask", 1)?;
    }
    Ok(())
}

fn environment(d: &w::DocumentT, e: Option<&w::RenderEnvironmentT>) -> Result<(), String> {
    if let Some(e) = e {
        if e.cast_shadows > 3 || e.light_probes > 2 || e.reflection_probes > 3 {
            return Err("invalid render environment".into());
        }
        if e.cast_shadows != 0
            || e.receive_shadows
            || e.light_probes != 0
            || e.reflection_probes != 0
        {
            feature(d, "render.environment", 1)?;
        }
    }
    Ok(())
}
