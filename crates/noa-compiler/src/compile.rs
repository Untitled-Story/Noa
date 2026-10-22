//! Complete Prefab -> NOIR + assets -> atomic deterministic NOAP compilation.
use crate::{
    Compilation,
    convert::{self, materials::Resources, wire_util::*},
    diagnostics::{Diagnostic, Error, Severity},
    inspect::InspectOptions,
    prepare::prepare_indexed,
    resolver::{AssetIndex, MetadataCatalog, ScriptResolver},
};
use noa_format::{
    ir::{ReferenceKind, TypedReference},
    wire as w,
};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Serialize)]
pub struct CompileReport {
    pub output: String,
    pub nodes: usize,
    pub components: usize,
    pub assets: usize,
    pub materials: usize,
    pub animations: usize,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn compile(prefab: &Path, options: &InspectOptions) -> Result<Compilation, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    let metadata = MetadataCatalog::discover(&index.roots)?;
    compile_indexed(prefab, &index, &metadata)
}
pub fn compile_with_script_resolver(
    prefab: &Path,
    options: &InspectOptions,
    scripts: &dyn ScriptResolver,
) -> Result<Compilation, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    compile_indexed(prefab, &index, scripts)
}
fn compile_indexed(
    prefab: &Path,
    index: &AssetIndex,
    scripts: &dyn ScriptResolver,
) -> Result<Compilation, Error> {
    let (mut draft, sources, ids) = prepare_indexed(prefab, index, scripts)?;
    if draft.source_inspection.has_errors() {
        return Err(Error::new(
            "SOURCE_INSPECTION_FAILED",
            "source errors; use inspect --json",
        ));
    }
    let path = fs::canonicalize(prefab).map_err(|e| Error::io(prefab, e))?;
    let entry = index.label(&path);
    let file = &sources.files[&entry];
    let mut resources = Resources::new(index, &sources, &ids, ids.next_id()?);
    let mut components = Vec::new();
    for l in draft.layouts {
        let host = draft.host_layout_nodes.contains(&l.node_id);
        components.push(convert::document::layout(l, host));
    }
    components.extend(draft.animators.into_iter().map(convert::document::animator));
    for o in file.objects.values() {
        match o.class_id {
            198 => components.push(
                convert::particles::convert_system(&entry, o, &ids, file).map_err(|e| {
                    Error::new(
                        e.code,
                        format!("{} fileID={}: {}", entry, o.file_id, e.message),
                    )
                })?,
            ),
            199 => components.push(convert::renderers::renderer(
                &entry,
                o,
                file,
                &ids,
                &draft.mesh_bindings,
                &mut resources,
            )?),
            _ => {
                if let Some(c) = convert::scene_renderers::convert(
                    &entry,
                    o,
                    file,
                    &ids,
                    &mut resources,
                    scripts,
                )? {
                    components.push(c);
                }
            }
        }
    }
    // Shape resources are resolved after semantic particle conversion.
    for c in &mut components {
        if let w::ComponentDataT::ParticleSystem(p) = &mut c.data {
            let source = file
                .objects
                .values()
                .find(|o| ids.get(&entry, o.file_id).is_ok_and(|id| id.get() == c.id))
                .unwrap();
            for module in p.modules.iter_mut().flatten() {
                if let w::ParticleModuleDataT::ShapeModule(s) = &mut module.data
                    && let w::ShapeGeometryT::SpriteShape(sprite) = &mut s.geometry
                {
                    sprite.sprite_asset_id = resources
                        .sprite(&entry, get(get(&source.data, "ShapeModule")?, "m_Sprite")?)?;
                }
                if let w::ParticleModuleDataT::TextureSheetModule(t) = &mut module.data
                    && t.mode == w::SheetMode::Sprites
                {
                    let sprites = get(get(&source.data, "UVModule")?, "sprites")?
                        .as_sequence()
                        .ok_or_else(|| err("UVModule", "sprites must be a list"))?;
                    t.sprite_asset_ids = Some(
                        sprites
                            .iter()
                            .map(|s| resources.sprite(&entry, get(s, "sprite")?))
                            .collect::<Result<Vec<_>, Error>>()?,
                    );
                }
            }
        }
    }
    for p in &draft.pending_components {
        if !matches!(
            p.type_name.as_str(),
            "ParticleSystem"
                | "ParticleSystemRenderer"
                | "MeshFilter"
                | "MeshRenderer"
                | "SpriteRenderer"
                | "SpriteMask"
                | "Canvas"
                | "CanvasRenderer"
                | "CanvasGroup"
        ) {
            return Err(err("component", format!("unsupported {}", p.type_name)));
        }
    }
    for mesh in draft.meshes {
        let id = mesh.id.get();
        resources.assets.insert(
            id,
            n!(AssetT {
                id,
                kind: w::AssetKind::Mesh
            }),
        );
        resources.files.insert(
            format!("assets/{id}.noms"),
            mesh.geometry.encode().map_err(|e| err("mesh", e))?,
        );
        resources.targets.insert(
            (
                format!("builtin/{}", mesh.source.guid.unwrap()),
                mesh.source.file_id,
            ),
            TypedReference {
                kind: ReferenceKind::Asset,
                id: mesh.id,
            },
        );
    }
    // Event-only materials/images are real dependencies, not unresolved debug refs.
    for a in &mut draft.animations {
        for e in &mut a.events {
            if let Some(reference) = &mut e.object_parameter
                && reference.target.is_none()
            {
                let mut raw = serde_yaml_ng::Mapping::new();
                raw.insert("fileID".into(), reference.source.file_id.into());
                if let Some(guid) = &reference.source.guid {
                    raw.insert("guid".into(), guid.clone().into());
                }
                let (id, _, _) = resources.image(&entry, &raw.into())?;
                reference.target = Some(Box::new(n!(TypedReferenceT {
                    id,
                    kind: w::ReferenceKind::Asset
                })));
            }
            if let Some(target) = e.object_parameter.as_ref().and_then(|r| r.target.as_ref())
                && matches!(
                    target.kind,
                    w::ReferenceKind::Material | w::ReferenceKind::Asset
                )
            {
                if target.kind == w::ReferenceKind::Asset
                    && resources.assets.contains_key(&target.id)
                {
                    continue;
                }
                let mut found = false;
                for (source, file) in &sources.files {
                    for object in file.objects.values() {
                        if ids.get(source, object.file_id)?.get() != target.id {
                            continue;
                        }
                        let reference: serde_yaml_ng::Value =
                            serde_yaml_ng::from_str(&format!("{{fileID: {}}}", object.file_id))
                                .unwrap();
                        match object.class_id {
                            21 => {
                                resources.material(source, &reference)?;
                            }
                            28 => {
                                resources.image(source, &reference)?;
                            }
                            43 => {
                                resources.mesh(source, &reference)?;
                            }
                            213 => {
                                resources.sprite(source, &reference)?;
                            }
                            _ => return Err(err("event object", "unsupported resource type")),
                        }
                        found = true;
                    }
                }
                if !found {
                    return Err(err("event object", "missing source resource"));
                }
            }
        }
    }
    // Gravity can become active through animation even when its initial value is zero.
    for a in &draft.animations {
        for t in &a.tracks {
            if t.binding.property == w::AnimationProperty::ParticleParameter
                && t.binding.parameter == w::ParticleParameter::GravityMultiplier
                && let Some(w::ComponentDataT::ParticleSystem(p)) = components
                    .iter_mut()
                    .find(|c| c.id == t.binding.target_id)
                    .map(|c| &mut c.data)
            {
                p.main.host_gravity_slot = Some("world-gravity".into());
            }
        }
    }
    let mut targets = resources.targets.clone();
    for o in file.objects.values() {
        let target = match o.class_id {
            1 => Some(TypedReference {
                kind: ReferenceKind::Node,
                id: ids.get(&entry, o.file_id)?,
            }),
            4 => Some(TypedReference {
                kind: ReferenceKind::Node,
                id: ids.get(&entry, int(get(&o.data, "m_GameObject")?, "fileID")?)?,
            }),
            224 | 95 | 114 | 198 | 199 | 33 | 23 | 212 | 331 | 223 | 222 | 225 => {
                Some(TypedReference {
                    kind: ReferenceKind::Component,
                    id: ids.get(&entry, o.file_id)?,
                })
            }
            _ => None,
        };
        if let Some(t) = target {
            targets.insert((entry.clone(), o.file_id), t);
        }
    }
    let converted = convert::convert_custom_components(&entry, file, &ids, scripts, &|r| {
        let source = match &r.guid {
            None => entry.clone(),
            Some(g) => {
                let guid: crate::unity::Guid = g.parse().ok()?;
                if guid.is_zero() {
                    entry.clone()
                } else if guid.is_builtin() {
                    format!("builtin/{guid}")
                } else {
                    sources.guid_sources.get(&guid)?.clone()
                }
            }
        };
        targets.get(&(source, r.file_id)).copied()
    })?;
    let mut diagnostics: Vec<_> = draft
        .source_inspection
        .diagnostics
        .into_iter()
        .filter(|d| d.code != "BUILTIN_RESOURCE")
        .collect();
    diagnostics.extend(
        draft
            .diagnostics
            .into_iter()
            .filter(|d| d.code == "HOST_PROJECT_COLOR_SPACE_REQUIRED"),
    );
    diagnostics.extend(converted.diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(err(
            "compilation",
            "conversion errors; inspect --ir --json for details",
        ));
    }
    components.extend(
        converted
            .components
            .into_iter()
            .map(convert::document::custom),
    );
    components.sort_by_key(|c| c.id);
    if draft.color_context.is_none()
        && components.iter().any(|c| {
            matches!(
                c.data,
                w::ComponentDataT::MeshRenderer(_)
                    | w::ComponentDataT::SpriteRenderer(_)
                    | w::ComponentDataT::UIImage(_)
            )
        })
    {
        draft.color_context = Some(crate::resolver::resolve_color_context(index)?);
        if draft
            .color_context
            .as_ref()
            .is_some_and(|c| c.host_project_color_space_slot.is_some())
        {
            diagnostics.push(Diagnostic { severity: Severity::Warning, code:"HOST_PROJECT_COLOR_SPACE_REQUIRED", asset:entry.clone(), file_id:None,
                property:Some("ColorContext.project_color_space".into()), message:"source project color space is unavailable; Natsume must receive source-project-color-space from the host".into() });
        }
    }
    let nodes: Vec<_> = draft
        .nodes
        .into_iter()
        .map(convert::document::node)
        .collect();
    let roots = nodes
        .iter()
        .filter(|n| n.parent_id == 0)
        .map(|n| n.id)
        .collect();
    let entry_id = resources.allocate()?;
    let ep = n!(EntryPointT {
        id: entry_id,
        name: prefab
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("effect")
            .into(),
        roots,
        camera_policy: w::CameraPolicy::HostRequired,
        host_camera_slot: Some("main-camera".into())
    });
    let mut features = BTreeMap::<String, u32>::new();
    features.insert("scene.trs".into(), 1);
    if draft.color_context.is_some() {
        features.insert("color.authored".into(), 1);
    }
    for a in &draft.animations {
        if !a.events.is_empty() {
            features.insert("animation.events-typed".into(), 1);
        }
        for t in &a.tracks {
            let capability = match t.binding.property {
                w::AnimationProperty::NodeActive => "animation.node-active",
                w::AnimationProperty::NodePosition
                | w::AnimationProperty::NodeRotation
                | w::AnimationProperty::NodeScale => "animation.transform",
                w::AnimationProperty::RectAnchoredPosition
                | w::AnimationProperty::RectSizeDelta
                | w::AnimationProperty::RectAnchorMin
                | w::AnimationProperty::RectAnchorMax
                | w::AnimationProperty::RectPivot => "animation.layout",
                w::AnimationProperty::ParticleParameter
                | w::AnimationProperty::ParticleLoop
                | w::AnimationProperty::ParticleColor
                | w::AnimationProperty::ParticleCustomColor => "animation.particle",
                w::AnimationProperty::RendererEnabled | w::AnimationProperty::ComponentEnabled => {
                    "animation.component-enabled"
                }
                w::AnimationProperty::SpriteColor => "animation.sprite",
                _ => "animation.renderer-material",
            };
            features.insert(capability.into(), 1);
        }
    }
    features.insert("camera.host".into(), 1);
    if !draft.host_layout_nodes.is_empty() {
        features.insert("layout.rect".into(), 1);
    }
    for c in &components {
        match &c.data {
            w::ComponentDataT::MeshFilter(_) | w::ComponentDataT::MeshRenderer(_) => {
                features.insert("scene.mesh-renderer".into(), 1);
            }
            w::ComponentDataT::SpriteRenderer(r) => {
                features.insert("scene.sprite-renderer".into(), 1);
                if r.mask_interaction != w::MaskInteraction::None {
                    features.insert("render.sprite-mask".into(), 1);
                }
            }
            w::ComponentDataT::SpriteMask(_) => {
                features.insert("render.sprite-mask".into(), 1);
            }
            w::ComponentDataT::Canvas(_)
            | w::ComponentDataT::CanvasRenderer(_)
            | w::ComponentDataT::UIImage(_)
            | w::ComponentDataT::UIMask(_)
            | w::ComponentDataT::RectMask2D(_)
            | w::ComponentDataT::CanvasGroup(_) => {
                features.insert("ui.canvas".into(), 1);
            }
            w::ComponentDataT::CustomComponent(_) => {
                features.insert("script.bridge".into(), 1);
            }
            w::ComponentDataT::Animator(_) => {
                features.insert("animator.single-layer".into(), 1);
            }
            w::ComponentDataT::ParticleRenderer(r) => {
                features.insert("particle.renderer".into(), 1);
                if r.mode == w::RenderMode::None {
                    features.insert("particle.renderer-none".into(), 1);
                }
                if r.mask_interaction != w::MaskInteraction::None {
                    features.insert("render.sprite-mask".into(), 1);
                }
                if r.mode == w::RenderMode::Mesh
                    && r.mesh_distribution == w::MeshDistribution::NonUniformRandom
                {
                    features.insert("particle.mesh-distribution".into(), 1);
                }
            }
            w::ComponentDataT::ParticleSystem(p) => {
                features.insert("particle.main".into(), 1);
                features.insert("color.authored".into(), 1);
                for m in p.modules.iter().flatten() {
                    let name = match &m.data {
                        w::ParticleModuleDataT::EmissionModule(_) => "emission",
                        w::ParticleModuleDataT::ShapeModule(s) => {
                            let feature = noa_format::document::shape_feature(&s.geometry)
                                .ok_or_else(|| err("ShapeModule", "missing geometry"))?;
                            features.insert(feature.into(), 1);
                            continue;
                        }
                        w::ParticleModuleDataT::ColorModule(_) => "color",
                        w::ParticleModuleDataT::SizeModule(_) => "size",
                        w::ParticleModuleDataT::SizeBySpeedModule(_) => "size-by-speed",
                        w::ParticleModuleDataT::RotationBySpeedModule(_) => "rotation-by-speed",
                        w::ParticleModuleDataT::RotationModule(_) => "rotation",
                        w::ParticleModuleDataT::VelocityModule(_) => "velocity",
                        w::ParticleModuleDataT::LimitVelocityModule(_) => "limit-velocity",
                        w::ParticleModuleDataT::NoiseModule(_) => "noise.unity",
                        w::ParticleModuleDataT::TextureSheetModule(t) => {
                            if t.mode == w::SheetMode::Sprites {
                                features.insert("particle.texture-sheet-sprites".into(), 1);
                            }
                            "texture-sheet"
                        }
                        w::ParticleModuleDataT::CustomDataModule(_) => "custom-data",
                        _ => return Err(err("module", "unknown union")),
                    };
                    features.insert(format!("particle.{name}"), 1);
                }
            }
            _ => {}
        }
    }
    if components.iter().any(|c| {
        let e = match &c.data {
            w::ComponentDataT::MeshRenderer(r) => r.environment.as_deref(),
            w::ComponentDataT::ParticleRenderer(r) => r.environment.as_deref(),
            _ => None,
        };
        e.is_some_and(|e| {
            e.cast_shadows != 0
                || e.receive_shadows
                || e.light_probes != 0
                || e.reflection_probes != 0
        })
    }) {
        features.insert("render.environment".into(), 1);
        diagnostics.push(Diagnostic {
            severity: Severity::Warning, code: "HOST_RENDER_ENVIRONMENT_REQUIRED", asset: entry.clone(), file_id: None,
            property: Some("Renderer.environment".into()), message: "source requests shadows or probes; Natsume/host must provide a render environment; flags do not contain baked light/probe data".into()
        });
    }
    for m in resources.materials.values() {
        features.insert(
            match m.model {
                w::MaterialModel::ParticleAddAlpha => "material.particle-add-alpha",
                w::MaterialModel::LegacyParticleAdditive => {
                    "material.unity-legacy-particle-additive"
                }
                w::MaterialModel::LegacyParticleAlphaBlend => {
                    "material.unity-legacy-particle-alpha-blend"
                }
                w::MaterialModel::SpriteDefault => "material.unity-sprite-default",
                w::MaterialModel::UIDefault => "material.unity-ui-default",
                w::MaterialModel::TextureVertexColor => "material.texture-vertex-color",
                _ => "material.particle-unlit",
            }
            .into(),
            1,
        );
    }
    if resources
        .assets
        .values()
        .any(|a| a.kind == w::AssetKind::Sprite)
    {
        features.insert("asset.sprite".into(), 1);
    }
    if resources
        .assets
        .values()
        .any(|a| a.kind == w::AssetKind::Mesh)
    {
        features.insert("mesh.noms".into(), 2);
    }
    if resources.materials.values().any(|m| {
        matches!(
            m.model,
            w::MaterialModel::LegacyParticleAdditive | w::MaterialModel::LegacyParticleAlphaBlend
        )
    }) {
        features.insert("host.legacy-particle-environment".into(), 1);
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "HOST_PARTICLE_ENVIRONMENT_REQUIRED",
            asset: entry.clone(),
            file_id: None,
            property: Some("Material.shader_environment".into()),
            message: concat!(
                "source fog and soft-particle settings are unavailable; Natsume must resolve ",
                "'source-scene-fog' and 'source-soft-particles' host slots in the builtin shader ",
                "recipe; depth is required when soft particles are enabled"
            )
            .into(),
        });
    }
    if resources
        .files
        .iter()
        .filter(|(p, _)| p.ends_with(".shader.json"))
        .any(|(_, bytes)| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .is_ok_and(|r| r["global_mip_bias"].as_bool() == Some(true))
        })
    {
        diagnostics.push(Diagnostic { severity: Severity::Warning, code:"HOST_SHADER_MIP_BIAS_REQUIRED", asset:entry.clone(), file_id:None,
            property:Some("Material.shader_environment".into()), message:"verified shader reads source-global-mip-bias; Natsume must obtain the source scene value from the host".into() });
    }
    let mut records = Vec::new();
    for o in file.objects.values() {
        if targets
            .get(&(entry.clone(), o.file_id))
            .is_some_and(|t| t.id.get() == ids.get(&entry, o.file_id).unwrap().get())
        {
            records.push(n!(SourceRecordT {
                entity_id: ids.get(&entry, o.file_id)?.get(),
                asset_path: Some(entry.clone()),
                object_identity: Some(o.file_id.to_string())
            }));
        }
    }
    let mut document = n!(DocumentT {
        format_major: 2,
        nodes,
        components: Some(components),
        assets: Some(resources.assets.into_values().collect()),
        materials: Some(resources.materials.into_values().collect()),
        animations: Some(
            draft
                .animations
                .into_iter()
                .map(convert::document::animation)
                .collect()
        ),
        entries: vec![ep],
        default_entry_id: entry_id,
        features: Some(
            features
                .into_iter()
                .map(|(name, revision)| n!(FeatureRequirementT {
                    name,
                    revision,
                    required: true
                }))
                .collect()
        ),
        sources: Some(records),
        color_context: draft.color_context.map(|c| Box::new(n!(ColorContextT {
            project_color_space: color_space(c.project_color_space),
            host_project_color_space_slot: c.host_project_color_space_slot
        })))
    });
    if noa_format::document::requires_unity_infinity(&document) {
        let features = document.features.get_or_insert_with(Vec::new);
        features.push(n!(FeatureRequirementT {
            name: "curve.unity-infinity".into(),
            revision: 1,
            required: true
        }));
        features.sort_by(|a, b| a.name.cmp(&b.name));
    }
    let bytes = noa_format::document::encode(&document).map_err(|e| err("NOIR", e))?;
    let mut files = resources.files;
    files.insert("scene.noir".into(), bytes);
    Ok(Compilation {
        document,
        files,
        diagnostics,
    })
}
pub fn compile_to_path(
    prefab: &Path,
    output: &Path,
    options: &InspectOptions,
) -> Result<CompileReport, Error> {
    if output.extension().and_then(|v| v.to_str()) != Some("noap") {
        return Err(err("output", "expected .noap extension"));
    }
    let compilation = compile(prefab, options)?;
    compilation.write(output)?;
    Ok(CompileReport {
        output: output.display().to_string(),
        nodes: compilation.document.nodes.len(),
        components: compilation.document.components.as_ref().map_or(0, Vec::len),
        assets: compilation.document.assets.as_ref().map_or(0, Vec::len),
        materials: compilation.document.materials.as_ref().map_or(0, Vec::len),
        animations: compilation.document.animations.as_ref().map_or(0, Vec::len),
        diagnostics: compilation.diagnostics,
    })
}
