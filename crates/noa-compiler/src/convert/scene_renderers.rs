//! Standard scene rendering components. Custom game scripts remain in Script Bridge.
use super::{EntityIds, materials::Resources, wire_util::*};
use crate::{
    diagnostics::Error,
    resolver::{ScriptReference, ScriptResolution, ScriptResolver},
    unity::{Object, UnityFile},
};
use noa_format::wire as w;
use serde_yaml_ng::Value as Y;

pub(crate) fn standard_ui(
    source: &str,
    o: &Object,
    scripts: &dyn ScriptResolver,
) -> Option<String> {
    if o.class_id != 114 {
        return None;
    }
    let r = o.data.get("m_Script")?;
    let reference = ScriptReference {
        guid: r.get("guid")?.as_str()?.parse().ok(),
        file_id: r.get("fileID")?.as_i64()?,
    };
    if let ScriptResolution::Resolved(t) = scripts.resolve(source, &reference)
        && t.assembly.trim_end_matches(".dll") == "UnityEngine.UI"
        && t.namespace == "UnityEngine.UI"
        && ["Image", "Mask", "RectMask2D"].contains(&t.class_name.as_str())
    {
        return Some(t.class_name);
    }
    None
}
fn color(v: &Y) -> Result<w::ColorT, Error> {
    Ok(n!(ColorT {
        r: num(v, "r")?,
        g: num(v, "g")?,
        b: num(v, "b")?,
        a: num(v, "a")?
    }))
}
fn materials(entry: &str, v: &Y, r: &mut Resources<'_>) -> Result<Vec<u32>, Error> {
    get(v, "m_Materials")?
        .as_sequence()
        .ok_or_else(|| err("Renderer", "materials must be a list"))?
        .iter()
        .map(|x| r.material(entry, x))
        .collect()
}
fn sprite(entry: &str, v: &Y, key: &str, r: &mut Resources<'_>) -> Result<u32, Error> {
    let value = get(v, key)?;
    if int(value, "fileID")? == 0 {
        Ok(0)
    } else {
        r.sprite(entry, value)
    }
}
pub(crate) fn convert(
    entry: &str,
    o: &Object,
    file: &UnityFile,
    ids: &EntityIds,
    r: &mut Resources<'_>,
    scripts: &dyn ScriptResolver,
) -> Result<Option<w::ComponentT>, Error> {
    let v = &o.data;
    let data = match o.class_id {
        33 => {
            let reference = get(v, "m_Mesh")?;
            let id = if int(reference, "fileID")? == 0 {
                0
            } else {
                r.mesh(entry, reference)?
            };
            w::ComponentDataT::MeshFilter(Box::new(n!(MeshFilterT { mesh_asset_id: id })))
        }
        23 => {
            let node = int(get(v, "m_GameObject")?, "fileID")?;
            let filters: Vec<_> = file
                .objects
                .values()
                .filter(|f| {
                    f.class_id == 33 && f.data["m_GameObject"]["fileID"].as_i64() == Some(node)
                })
                .collect();
            if filters.len() != 1 {
                return Err(err("MeshRenderer", "expected exactly one MeshFilter"));
            }
            w::ComponentDataT::MeshRenderer(Box::new(n!(MeshRendererT {
                mesh_filter_component_id: ids.get(entry, filters[0].file_id)?.get(),
                materials: materials(entry, v, r)?,
                sorting_layer_id: i32::try_from(int(v, "m_SortingLayerID")?)
                    .map_err(|_| err("m_SortingLayerID", "outside int32"))?,
                sorting_layer_index: i32::try_from(int(v, "m_SortingLayer")?)
                    .map_err(|_| err("m_SortingLayer", "outside int32"))?,
                sorting_order: i32::try_from(int(v, "m_SortingOrder")?)
                    .map_err(|_| err("m_SortingOrder", "outside int32"))?,
                environment: Some(Box::new(n!(RenderEnvironmentT {
                    cast_shadows: u8::try_from(int(v, "m_CastShadows")?)
                        .map_err(|_| err("shadows", "invalid mode"))?,
                    receive_shadows: flag(v, "m_ReceiveShadows")?,
                    light_probes: u8::try_from(uint(v, "m_LightProbeUsage")?)
                        .map_err(|_| err("m_LightProbeUsage", "outside uint8"))?,
                    reflection_probes: u8::try_from(uint(v, "m_ReflectionProbeUsage")?)
                        .map_err(|_| err("m_ReflectionProbeUsage", "outside uint8"))?
                })))
            })))
        }
        212 => w::ComponentDataT::SpriteRenderer(Box::new(n!(SpriteRendererT {
            sprite_asset_id: sprite(entry, v, "m_Sprite", r)?,
            materials: materials(entry, v, r)?,
            color: Some(color(get(v, "m_Color")?)?),
            flip_x: flag(v, "m_FlipX")?,
            flip_y: flag(v, "m_FlipY")?,
            draw_mode: enum_value(
                int(v, "m_DrawMode")?,
                &[
                    w::SpriteDrawMode::Simple,
                    w::SpriteDrawMode::Sliced,
                    w::SpriteDrawMode::Tiled
                ],
                "sprite draw mode"
            )?,
            size: Some(vec2(get(v, "m_Size")?)?),
            tile_mode: u8::try_from(uint(v, "m_SpriteTileMode")?)
                .map_err(|_| err("m_SpriteTileMode", "outside uint8"))?,
            adaptive_mode_threshold: num(v, "m_AdaptiveModeThreshold")?,
            mask_interaction: enum_value(
                int(v, "m_MaskInteraction")?,
                &[
                    w::MaskInteraction::None,
                    w::MaskInteraction::VisibleInside,
                    w::MaskInteraction::VisibleOutside
                ],
                "mask"
            )?,
            sorting_layer_id: i32::try_from(int(v, "m_SortingLayerID")?)
                .map_err(|_| err("m_SortingLayerID", "outside int32"))?,
            sorting_layer_index: i32::try_from(int(v, "m_SortingLayer")?)
                .map_err(|_| err("m_SortingLayer", "outside int32"))?,
            sorting_order: i32::try_from(int(v, "m_SortingOrder")?)
                .map_err(|_| err("m_SortingOrder", "outside int32"))?,
            sort_point: enum_value(
                int(v, "m_SpriteSortPoint")?,
                &[w::SpriteSortPoint::Center, w::SpriteSortPoint::Pivot],
                "sprite sort point"
            )?
        }))),
        331 => w::ComponentDataT::SpriteMask(Box::new(n!(SpriteMaskT {
            sprite_asset_id: sprite(entry, v, "m_Sprite", r)?,
            alpha_cutoff: num(v, "m_MaskAlphaCutoff")?,
            custom_range: flag(v, "m_IsCustomRangeActive")?,
            front_layer_id: i32::try_from(int(v, "m_FrontSortingLayerID")?)
                .map_err(|_| err("m_FrontSortingLayerID", "outside int32"))?,
            front_layer_index: i32::try_from(int(v, "m_FrontSortingLayer")?)
                .map_err(|_| err("m_FrontSortingLayer", "outside int32"))?,
            front_order: i32::try_from(int(v, "m_FrontSortingOrder")?)
                .map_err(|_| err("m_FrontSortingOrder", "outside int32"))?,
            back_layer_id: i32::try_from(int(v, "m_BackSortingLayerID")?)
                .map_err(|_| err("m_BackSortingLayerID", "outside int32"))?,
            back_layer_index: i32::try_from(int(v, "m_BackSortingLayer")?)
                .map_err(|_| err("m_BackSortingLayer", "outside int32"))?,
            back_order: i32::try_from(int(v, "m_BackSortingOrder")?)
                .map_err(|_| err("m_BackSortingOrder", "outside int32"))?,
            sort_point: enum_value(
                int(v, "m_SpriteSortPoint")?,
                &[w::SpriteSortPoint::Center, w::SpriteSortPoint::Pivot],
                "mask sort point"
            )?
        }))),
        223 => {
            let mode = enum_value(
                int(v, "m_RenderMode")?,
                &[
                    w::CanvasMode::ScreenOverlay,
                    w::CanvasMode::ScreenCamera,
                    w::CanvasMode::World,
                ],
                "canvas mode",
            )?;
            let camera = int(get(v, "m_Camera")?, "fileID")?;
            if camera != 0 {
                return Err(err("Canvas", "embedded camera conversion not implemented"));
            }
            w::ComponentDataT::Canvas(Box::new(n!(CanvasT {
                mode,
                host_camera_slot: if mode == w::CanvasMode::ScreenCamera {
                    Some("main-camera".into())
                } else {
                    None
                },
                plane_distance: num(v, "m_PlaneDistance")?,
                pixel_perfect: flag(v, "m_PixelPerfect")?,
                override_pixel_perfect: flag(v, "m_OverridePixelPerfect")?,
                override_sorting: flag(v, "m_OverrideSorting")?,
                sorting_layer_id: i32::try_from(int(v, "m_SortingLayerID")?)
                    .map_err(|_| err("m_SortingLayerID", "outside int32"))?,
                sorting_order: i32::try_from(int(v, "m_SortingOrder")?)
                    .map_err(|_| err("m_SortingOrder", "outside int32"))?,
                additional_shader_channels: uint(v, "m_AdditionalShaderChannelsFlag")?,
                target_display: uint(v, "m_TargetDisplay")?,
                vertex_color_gamma: flag(v, "m_VertexColorAlwaysGammaSpace")?
            })))
        }
        222 => w::ComponentDataT::CanvasRenderer(Box::new(n!(CanvasRendererT {
            cull_transparent_mesh: flag(v, "m_CullTransparentMesh")?
        }))),
        225 => w::ComponentDataT::CanvasGroup(Box::new(n!(CanvasGroupT {
            alpha: num(v, "m_Alpha")?,
            ignore_parent_groups: flag(v, "m_IgnoreParentGroups")?
        }))),
        114 => match standard_ui(entry, o, scripts).as_deref() {
            Some("Image") => {
                let material = get(v, "m_Material")?;
                let material_id = if int(material, "fileID")? == 0 {
                    r.sprite_default(("noa/ui-default".into(), 1), true)?
                } else {
                    r.material(entry, material)?
                };
                w::ComponentDataT::UIImage(Box::new(n!(UIImageT {
                    sprite_asset_id: sprite(entry, v, "m_Sprite", r)?,
                    override_sprite_asset_id: if v.get("m_OverrideSprite").is_some() {
                        sprite(entry, v, "m_OverrideSprite", r)?
                    } else {
                        0
                    },
                    material_id,
                    color: Some(color(get(v, "m_Color")?)?),
                    type_: enum_value(
                        int(v, "m_Type")?,
                        &[
                            w::ImageType::Simple,
                            w::ImageType::Sliced,
                            w::ImageType::Tiled,
                            w::ImageType::Filled
                        ],
                        "image type"
                    )?,
                    preserve_aspect: flag(v, "m_PreserveAspect")?,
                    fill_center: flag(v, "m_FillCenter")?,
                    fill_method: u8::try_from(uint(v, "m_FillMethod")?)
                        .map_err(|_| err("m_FillMethod", "outside uint8"))?,
                    fill_amount: num(v, "m_FillAmount")?,
                    fill_clockwise: flag(v, "m_FillClockwise")?,
                    fill_origin: u8::try_from(uint(v, "m_FillOrigin")?)
                        .map_err(|_| err("m_FillOrigin", "outside uint8"))?,
                    use_sprite_mesh: flag(v, "m_UseSpriteMesh")?,
                    pixels_per_unit_multiplier: num(v, "m_PixelsPerUnitMultiplier")?,
                    maskable: flag(v, "m_Maskable")?
                })))
            }
            Some("Mask") => {
                let node = int(get(v, "m_GameObject")?, "fileID")?;
                let graphic = file.objects.values().find(|c| {
                    c.data["m_GameObject"]["fileID"].as_i64() == Some(node)
                        && standard_ui(entry, c, scripts).as_deref() == Some("Image")
                });
                w::ComponentDataT::UIMask(Box::new(n!(UIMaskT {
                    graphic_component_id: graphic
                        .map(|g| ids.get(entry, g.file_id).map(|id| id.get()))
                        .transpose()?
                        .unwrap_or(0),
                    show_graphic: flag(v, "m_ShowMaskGraphic")?
                })))
            }
            Some("RectMask2D") => {
                let p = get(v, "m_Padding")?;
                w::ComponentDataT::RectMask2D(Box::new(n!(RectMask2DT {
                    padding: Some(n!(Vec4T {
                        x: num(p, "x")?,
                        y: num(p, "y")?,
                        z: num(p, "z")?,
                        w: num(p, "w")?
                    })),
                    softness: Some(vec2(get(v, "m_Softness")?)?)
                })))
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(n!(ComponentT {
        id: ids.get(entry, o.file_id)?.get(),
        node_id: ids
            .get(entry, int(get(v, "m_GameObject")?, "fileID")?)?
            .get(),
        enabled: if v.get("m_Enabled").is_some() {
            flag(v, "m_Enabled")?
        } else {
            true
        },
        data
    })))
}
