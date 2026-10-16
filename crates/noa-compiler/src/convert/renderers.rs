use super::{EntityIds, materials::Resources, wire_util::*};
use crate::{
    assets::MeshBinding,
    diagnostics::Error,
    unity::{Object, UnityFile},
};
use noa_format::wire as w;
pub(crate) fn renderer(
    entry: &str,
    o: &Object,
    file: &UnityFile,
    ids: &EntityIds,
    meshes: &[MeshBinding],
    resources: &mut Resources<'_>,
) -> Result<w::ComponentT, Error> {
    let v = &o.data;
    let node = int(get(v, "m_GameObject")?, "fileID")?;
    let systems: Vec<_> = file
        .objects
        .values()
        .filter(|o| o.class_id == 198 && o.data["m_GameObject"]["fileID"].as_i64() == Some(node))
        .collect();
    if systems.len() != 1 {
        return Err(err("Renderer", "expected one ParticleSystem on same node"));
    }
    for k in [
        "m_CastShadows",
        "m_ReceiveShadows",
        "m_LightProbeUsage",
        "m_ReflectionProbeUsage",
        "m_MaskInteraction",
    ] {
        if int(v, k)? != 0 {
            return Err(err(k, "feature unsupported"));
        }
    }
    let mode = enum_value(
        int(v, "m_RenderMode")?,
        &[
            w::RenderMode::Billboard,
            w::RenderMode::StretchedBillboard,
            w::RenderMode::HorizontalBillboard,
            w::RenderMode::VerticalBillboard,
            w::RenderMode::Mesh,
        ],
        "render mode",
    )?;
    let mut materials = Vec::new();
    for r in get(v, "m_Materials")?
        .as_sequence()
        .ok_or_else(|| err("m_Materials", "expected list"))?
    {
        materials.push(resources.material(entry, r)?);
    }
    if materials.iter().all(|m| *m == 0) {
        return Err(err("Renderer", "no material"));
    }
    let component_id = ids.get(entry, o.file_id)?;
    let bindings: Vec<_> = meshes
        .iter()
        .filter(|b| b.component_id == component_id)
        .collect();
    if mode == w::RenderMode::Mesh && bindings.is_empty() {
        return Err(err("Renderer", "missing Mesh geometry"));
    }
    let mut mesh_ids = Vec::new();
    let mut weights = Vec::new();
    for b in bindings {
        mesh_ids.push(b.mesh_asset_id.get());
        let key = if b.slot == 0 {
            "m_MeshWeighting".into()
        } else {
            format!("m_MeshWeighting{}", b.slot)
        };
        weights.push(num(v, &key)?);
    }
    if mode == w::RenderMode::Mesh && int(v, "m_MeshDistribution")? != 0 {
        return Err(err("Mesh distribution", "nonuniform selection unsupported"));
    }
    let stream_bytes = get(v, "m_VertexStreams")?
        .as_str()
        .ok_or_else(|| err("vertex streams", "expected hex bytes"))?;
    if !stream_bytes.is_ascii() || !stream_bytes.len().is_multiple_of(2) {
        return Err(err("vertex streams", "invalid bytes"));
    }
    let mut streams = Vec::new();
    for i in (0..stream_bytes.len()).step_by(2) {
        let code = u8::from_str_radix(&stream_bytes[i..i + 2], 16)
            .map_err(|_| err("vertex streams", "invalid byte"))?;
        let (semantic, components) = match code {
            0 => (w::VertexSemantic::Position, 3),
            1 => (w::VertexSemantic::Normal, 3),
            3 => (w::VertexSemantic::Color, 4),
            4 => (w::VertexSemantic::UV0, 2),
            5 => (w::VertexSemantic::UV1, 2),
            6 => (w::VertexSemantic::UV2, 2),
            7 => (w::VertexSemantic::UV3, 2),
            10 => (w::VertexSemantic::Center, 3),
            11 => (w::VertexSemantic::VertexId, 1),
            12..=14 => (w::VertexSemantic::Size, code - 11),
            15 => (w::VertexSemantic::Rotation, 1),
            16 => (w::VertexSemantic::Rotation, 3),
            19 => (w::VertexSemantic::Velocity, 3),
            21 => (w::VertexSemantic::AgeNormalized, 1),
            31..=34 => (w::VertexSemantic::Custom1, code - 30),
            35..=38 => (w::VertexSemantic::Custom2, code - 34),
            _ => return Err(err("vertex streams", format!("unsupported stream {code}"))),
        };
        streams.push(n!(VertexStreamT {
            semantic,
            components
        }));
    }
    let data = n!(ParticleRendererT {
        system_component_id: ids.get(entry, systems[0].file_id)?.get(),
        materials,
        mode,
        alignment: enum_value(
            int(v, "m_RenderAlignment")?,
            &[
                w::Alignment::View,
                w::Alignment::World,
                w::Alignment::Local,
                w::Alignment::Facing,
                w::Alignment::Velocity
            ],
            "alignment"
        )?,
        mesh_asset_ids: (!mesh_ids.is_empty()).then_some(mesh_ids),
        mesh_weights: (!weights.is_empty()).then_some(weights),
        sorting_layer_rank: i32::try_from(int(v, "m_SortingLayer")?)
            .map_err(|_| err("sorting layer", "outside int32"))?,
        source_sorting_layer_id: i32::try_from(int(v, "m_SortingLayerID")?)
            .map_err(|_| err("sorting layer ID", "outside int32"))?,
        sorting_order: i32::try_from(int(v, "m_SortingOrder")?)
            .map_err(|_| err("sorting order", "outside int32"))?,
        sorting_fudge: num(v, "m_SortingFudge")?,
        sort: enum_value(
            int(v, "m_SortMode")?,
            &[
                w::ParticleSort::None,
                w::ParticleSort::Distance,
                w::ParticleSort::OldestFirst,
                w::ParticleSort::YoungestFirst,
                w::ParticleSort::Depth
            ],
            "particle sort"
        )?,
        pivot: Some(vec3(get(v, "m_Pivot")?)?),
        flip: Some(vec3(get(v, "m_Flip")?)?),
        min_size: num(v, "m_MinParticleSize")?,
        max_size: num(v, "m_MaxParticleSize")?,
        camera_velocity_scale: num(v, "m_CameraVelocityScale")?,
        velocity_scale: num(v, "m_VelocityScale")?,
        length_scale: num(v, "m_LengthScale")?,
        allow_roll: flag(v, "m_AllowRoll")?,
        vertex_streams: Some(streams),
        apply_active_color_space: flag(v, "m_ApplyActiveColorSpace")?,
        normal_direction: num(v, "m_NormalDirection")?,
        freeform_stretching: flag(v, "m_FreeformStretching")?,
        rotate_with_stretch_direction: flag(v, "m_RotateWithStretchDirection")?
    });
    Ok(n!(ComponentT {
        id: component_id.get(),
        node_id: ids.get(entry, node)?.get(),
        enabled: flag(v, "m_Enabled")?,
        data: w::ComponentDataT::ParticleRenderer(Box::new(data))
    }))
}
