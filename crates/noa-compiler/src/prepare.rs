//! Partial IR preparation for development. This is not package compilation.

use crate::{
    assets::{MeshAsset, MeshBinding, plan_builtin_meshes},
    convert::{
        EntityIds, PendingComponent, SourceIdentity, UnitySources, convert_custom_components,
        convert_particle_colors, convert_scene, plan_animators,
    },
    diagnostics::{Diagnostic, Error},
    inspect::{InspectOptions, Inspection, inspect_indexed},
    resolver::{AssetIndex, MetadataCatalog, ScriptResolver, resolve_color_context},
};
use noa_format::ir::{
    Animation, Animator, ColorContext, CustomComponent, EntityId, Node, ParticleColors, RectLayout,
    ReferenceKind, RendererColors, TypedReference,
};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

/// An inspectable compiler intermediate, not the NOIR wire representation.
#[derive(Debug, Serialize)]
pub struct DraftIr {
    pub source_inspection: Inspection,
    pub nodes: Vec<Node>,
    pub layouts: Vec<RectLayout>,
    pub animators: Vec<Animator>,
    pub animations: Vec<Animation>,
    pub custom_components: Vec<CustomComponent>,
    pub meshes: Vec<MeshAsset>,
    pub mesh_bindings: Vec<MeshBinding>,
    pub color_context: Option<ColorContext>,
    pub particle_colors: Vec<ParticleColors>,
    pub renderer_colors: Vec<RendererColors>,
    pub host_layout_nodes: Vec<EntityId>,
    pub pending_components: Vec<PendingComponent>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn prepare_ir(prefab: &Path, options: &InspectOptions) -> Result<DraftIr, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    let metadata = MetadataCatalog::discover(&index.roots)?;
    prepare_indexed(prefab, &index, &metadata).map(|v| v.0)
}

/// Use a native metadata provider for both inspection and IR conversion.
pub fn prepare_ir_with_script_resolver(
    prefab: &Path,
    options: &InspectOptions,
    scripts: &dyn ScriptResolver,
) -> Result<DraftIr, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    prepare_indexed(prefab, &index, scripts).map(|v| v.0)
}

pub(crate) fn prepare_indexed(
    prefab: &Path,
    index: &AssetIndex,
    metadata: &dyn ScriptResolver,
) -> Result<(DraftIr, UnitySources, EntityIds), Error> {
    let (inspection, parsed) = inspect_indexed(prefab, index, metadata)?;
    if inspection.diagnostics.iter().any(|d| {
        d.severity == crate::diagnostics::Severity::Error && d.code != "SCRIPT_METADATA_CONFLICT"
    }) {
        return Err(Error::new(
            "SOURCE_INSPECTION_FAILED",
            "source contains errors; use inspect --json for original references and diagnostics",
        ));
    }
    let canonical = fs::canonicalize(prefab).map_err(|e| Error::io(prefab, e))?;
    let entry = index.label(&canonical);
    let mut sources = UnitySources::default();
    for (path, file) in parsed {
        sources.files.insert(index.label(&path), file);
    }
    for (guid, asset) in &index.assets {
        sources
            .guid_sources
            .insert(guid.clone(), index.label(&asset.path));
    }
    let plans = plan_animators(&entry, &sources)?;
    let mesh_plan = plan_builtin_meshes(&entry, &sources.files[&entry])?;
    let source_ids = sources
        .files
        .iter()
        .flat_map(|(name, file)| {
            file.objects
                .keys()
                .map(move |id| SourceIdentity::new(name, *id))
        })
        .chain(plans.iter().flat_map(|p| p.id_sources()))
        .chain(mesh_plan.id_sources());
    let ids = EntityIds::allocate(source_ids)?;
    let file = &sources.files[&entry];
    let mut scene = convert_scene(&entry, file, &ids)?;
    let (meshes, mesh_bindings) = mesh_plan.lower(&ids)?;
    let colors = convert_particle_colors(&entry, file, &ids)?;
    let color_context = if colors.systems.is_empty() && colors.renderers.is_empty() {
        None
    } else {
        Some(resolve_color_context(index)?)
    };
    let mut animators = Vec::new();
    let mut animations = Vec::new();
    for plan in plans {
        let (animator, mut clips) = plan.lower(&ids)?;
        animators.push(animator);
        animations.append(&mut clips);
    }
    // Register only actual converted entities. Source IDs alone do not imply support.
    let mut targets = BTreeMap::new();
    for object in file.objects.values() {
        let kind = match object.type_name.as_str() {
            "GameObject" => Some(ReferenceKind::Node),
            "MonoBehaviour" | "Animator" => Some(ReferenceKind::Component),
            _ => None,
        };
        if let Some(kind) = kind {
            targets.insert(
                (entry.clone(), object.file_id),
                TypedReference {
                    kind,
                    id: ids.get(&entry, object.file_id)?,
                },
            );
        }
    }
    for mesh in &meshes {
        targets.insert(
            (
                format!("builtin/{}", mesh.source.guid.as_deref().unwrap()),
                mesh.source.file_id,
            ),
            TypedReference {
                kind: ReferenceKind::Asset,
                id: mesh.id,
            },
        );
    }
    let mut scripts = convert_custom_components(&entry, file, &ids, metadata, &|reference| {
        let source = match &reference.guid {
            None => &entry,
            Some(guid) => {
                let guid: crate::unity::Guid = guid.parse().ok()?;
                if guid.is_zero() {
                    &entry
                } else if guid.is_builtin() {
                    return targets
                        .get(&(format!("builtin/{guid}"), reference.file_id))
                        .copied();
                } else {
                    sources.guid_sources.get(&guid)?
                }
            }
        };
        targets.get(&(source.clone(), reference.file_id)).copied()
    })?;
    if let Some(slot) = color_context
        .as_ref()
        .and_then(|c| c.host_project_color_space_slot.as_ref())
    {
        scripts.diagnostics.push(Diagnostic {
            severity: crate::diagnostics::Severity::Warning,
            code: "HOST_PROJECT_COLOR_SPACE_REQUIRED",
            asset: entry.clone(),
            file_id: None,
            property: Some("ColorContext.project_color_space".into()),
            message: format!("source project color space is unavailable; Natsume must receive explicit source-project context through '{slot}'"),
        });
    }
    scene
        .pending_components
        .retain(|c| !matches!(c.type_name.as_str(), "Animator" | "MonoBehaviour"));
    animators.sort_by_key(|c| c.id);
    animations.sort_by_key(|c| c.id);
    Ok((
        DraftIr {
            nodes: scene.nodes,
            layouts: scene.layouts,
            animators,
            animations,
            custom_components: scripts.components,
            meshes,
            mesh_bindings,
            color_context,
            particle_colors: colors.systems,
            renderer_colors: colors.renderers,
            host_layout_nodes: scene.host_layout_nodes,
            pending_components: scene.pending_components,
            diagnostics: scripts.diagnostics,
            source_inspection: inspection,
        },
        sources,
        ids,
    ))
}
