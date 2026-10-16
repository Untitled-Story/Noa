use crate::{
    diagnostics::{Diagnostic, Error, Severity},
    resolver::{AssetIndex, MetadataCatalog, ScriptReference, ScriptResolution, ScriptResolver},
    unity::{ObjectReference, ReferenceKind, UnityFile},
};
use serde::Serialize;
use serde_yaml_ng::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default)]
pub struct InspectOptions {
    pub search_roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct AssetSummary {
    pub path: String,
    pub guid: Option<String>,
    pub object_counts: BTreeMap<String, usize>,
    pub references: Vec<ObjectReference>,
}

#[derive(Debug, Serialize)]
pub struct ParticleSummary {
    pub asset: String,
    pub file_id: i64,
    pub node_name: Option<String>,
    pub node_file_id: Option<i64>,
    pub enabled_modules: Vec<String>,
    pub looped: Option<bool>,
    pub play_on_awake: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ScriptSummary {
    pub asset: String,
    pub file_id: i64,
    pub script_asset: Option<String>,
    pub script_reference: ScriptReference,
    pub script_identity: Option<noa_format::ir::ScriptIdentity>,
    pub node_file_id: Option<i64>,
    pub enabled: Option<bool>,
    pub fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SceneSummary {
    pub asset: String,
    pub nodes: Vec<crate::unity::SceneNode>,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub entry: String,
    pub indexed_asset_count: usize,
    pub assets: Vec<AssetSummary>,
    pub particles: Vec<ParticleSummary>,
    pub scenes: Vec<SceneSummary>,
    pub scripts: Vec<ScriptSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Inspection {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Inspect source assets only. Successful resolution does not imply IR/playback support.
pub fn inspect(prefab: &Path, options: &InspectOptions) -> Result<Inspection, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    let metadata = MetadataCatalog::discover(&index.roots)?;
    inspect_indexed(prefab, &index, &metadata).map(|(report, _)| report)
}

/// Inject a provider for native Unity metadata without requiring JSON sidecars.
pub fn inspect_with_script_resolver(
    prefab: &Path,
    options: &InspectOptions,
    scripts: &dyn ScriptResolver,
) -> Result<Inspection, Error> {
    let index = AssetIndex::discover(prefab, &options.search_roots)?;
    inspect_indexed(prefab, &index, scripts).map(|(report, _)| report)
}

pub(crate) fn inspect_indexed(
    prefab: &Path,
    index: &AssetIndex,
    scripts: &dyn ScriptResolver,
) -> Result<(Inspection, BTreeMap<PathBuf, UnityFile>), Error> {
    let prefab = fs::canonicalize(prefab).map_err(|e| Error::io(prefab, e))?;
    let mut report = Inspection {
        entry: index.label(&prefab),
        indexed_asset_count: index.assets.len(),
        assets: Vec::new(),
        particles: Vec::new(),
        scenes: Vec::new(),
        scripts: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut pending = VecDeque::from([prefab]);
    let mut visited = BTreeSet::new();
    let mut parsed = BTreeMap::<PathBuf, UnityFile>::new();
    // Resolve the finite file graph. Cycles in source references are expected.
    while let Some(path) = pending.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let label = index.label(&path);
        let guid = index
            .assets
            .values()
            .find(|a| a.path == path)
            .map(|a| a.guid.to_string());
        let mut summary = AssetSummary {
            path: label.clone(),
            guid,
            object_counts: BTreeMap::new(),
            references: Vec::new(),
        };
        if is_unity_yaml(&path) {
            let text = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            let file =
                UnityFile::parse(&text).map_err(|e| Error::new(e.code, format!("{label}: {e}")))?;
            for obj in file.objects.values() {
                *summary
                    .object_counts
                    .entry(obj.type_name.clone())
                    .or_default() += 1;
            }
            summary.references = file
                .references()
                .map_err(|e| Error::new(e.code, format!("{label}: {e}")))?;
            for reference in &summary.references {
                match &reference.kind {
                    ReferenceKind::External { guid } => match index.assets.get(guid) {
                        Some(asset) => pending.push_back(asset.path.clone()),
                        // Script metadata is a valid identity source even when
                        // the export has no placeholder .cs file at all.
                        None if reference.property == "MonoBehaviour.m_Script"
                            && file.objects[&reference.source_file_id].type_name == "MonoBehaviour" => {},
                        None => report.diagnostics.push(Diagnostic {
                            severity: Severity::Error, code: "MISSING_ASSET", asset: label.clone(),
                            file_id: Some(reference.source_file_id), property: Some(reference.property.clone()),
                            message: format!("cannot resolve GUID {guid}; add the dependency directory with --search"),
                        }),
                    },
                    ReferenceKind::Local => {
                        if !file.objects.contains_key(&reference.target_file_id) {
                            report.diagnostics.push(reference_error("MISSING_OBJECT", &label, reference, format!("local fileID {} does not exist", reference.target_file_id)));
                        }
                    }
                    ReferenceKind::Builtin { guid } => report.diagnostics.push(Diagnostic {
                        severity: Severity::Warning, code: "BUILTIN_RESOURCE", asset: label.clone(),
                        file_id: Some(reference.source_file_id), property: Some(reference.property.clone()),
                        message: format!("Unity builtin resource {guid}/{}; conversion support is checked separately", reference.target_file_id),
                    }),
                }
            }
            if file
                .objects
                .values()
                .any(|o| matches!(o.type_name.as_str(), "Transform" | "RectTransform"))
            {
                let nodes = crate::unity::scene_nodes(&file)
                    .map_err(|e| Error::new(e.code, format!("{label}: {e}")))?;
                report.scenes.push(SceneSummary {
                    asset: label.clone(),
                    nodes,
                });
            }
            summarize_components(&label, &file, index, scripts, &mut report)?;
            parsed.insert(path, file);
        }
        report.assets.push(summary);
    }
    // External YAML object IDs can only be checked after their files are loaded.
    for (path, file) in &parsed {
        for reference in file.references()? {
            if let ReferenceKind::External { guid } = &reference.kind
                && let Some(asset) = index.assets.get(guid)
                && let Some(target) = parsed.get(&asset.path)
                && !target.objects.contains_key(&reference.target_file_id)
            {
                report.diagnostics.push(reference_error(
                    "MISSING_EXTERNAL_OBJECT",
                    &index.label(path),
                    &reference,
                    format!(
                        "GUID {guid} resolves to {}, but fileID {} does not exist",
                        index.label(&asset.path),
                        reference.target_file_id
                    ),
                ));
            }
        }
    }
    report.assets.sort_by(|a, b| a.path.cmp(&b.path));
    report
        .particles
        .sort_by(|a, b| (&a.asset, a.file_id).cmp(&(&b.asset, b.file_id)));
    report
        .scripts
        .sort_by(|a, b| (&a.asset, a.file_id).cmp(&(&b.asset, b.file_id)));
    report.diagnostics.sort_by(|a, b| {
        (&a.asset, a.file_id, &a.property, a.code).cmp(&(&b.asset, b.file_id, &b.property, b.code))
    });
    Ok((report, parsed))
}

fn is_unity_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("prefab" | "anim" | "controller" | "mat" | "asset" | "shadervariants" | "unity")
    )
}

fn reference_error(
    code: &'static str,
    asset: &str,
    reference: &ObjectReference,
    message: String,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code,
        asset: asset.into(),
        file_id: Some(reference.source_file_id),
        property: Some(reference.property.clone()),
        message,
    }
}

fn bool_value(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value.as_i64().and_then(|n| match n {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        })
    })
}

fn summarize_components(
    label: &str,
    file: &UnityFile,
    index: &AssetIndex,
    scripts: &dyn ScriptResolver,
    report: &mut Inspection,
) -> Result<(), Error> {
    for obj in file.objects.values() {
        if obj.type_name == "ParticleSystem" {
            let node_file_id = obj
                .data
                .get("m_GameObject")
                .and_then(|v| v.get("fileID"))
                .and_then(Value::as_i64);
            let node_name = node_file_id
                .and_then(|id| file.objects.get(&id))
                .and_then(|o| o.data.get("m_Name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let enabled_modules = obj
                .data
                .as_mapping()
                .into_iter()
                .flatten()
                .filter_map(|(key, value)| {
                    let key = key.as_str()?;
                    (key.ends_with("Module")
                        && value.get("enabled").and_then(bool_value) == Some(true))
                    .then(|| key.to_owned())
                })
                .collect();
            report.particles.push(ParticleSummary {
                asset: label.into(),
                file_id: obj.file_id,
                node_name,
                node_file_id,
                enabled_modules,
                looped: obj.data.get("looping").and_then(bool_value),
                play_on_awake: obj.data.get("playOnAwake").and_then(bool_value),
            });
        } else if obj.type_name == "MonoBehaviour" {
            let reference = obj.data.get("m_Script").ok_or_else(|| {
                Error::new(
                    "INVALID_REFERENCE",
                    format!("{label}, fileID {}: missing m_Script", obj.file_id),
                )
            })?;
            let script_reference = ScriptReference {
                guid: reference
                    .get("guid")
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| {
                                Error::new("INVALID_REFERENCE", "script GUID must be string")
                            })?
                            .parse()
                    })
                    .transpose()?,
                file_id: reference
                    .get("fileID")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        Error::new("INVALID_REFERENCE", "m_Script.fileID must be int64")
                    })?,
            };
            let script_identity = match scripts.resolve(label, &script_reference) {
                ScriptResolution::Resolved(identity) => Some(identity),
                resolution => {
                    let conflict = matches!(resolution, ScriptResolution::Conflict(_));
                    report.diagnostics.push(Diagnostic {
                        severity: if conflict { Severity::Error } else { Severity::Warning },
                        code: if conflict { "SCRIPT_METADATA_CONFLICT" } else { "SCRIPT_METADATA_MISSING" },
                        asset: label.into(), file_id: Some(obj.file_id), property: Some("MonoBehaviour.m_Script".into()),
                        message: format!("script {:?}: {resolution:?}; original reference retained, identity not inferred", script_reference),
                    });
                    None
                }
            };
            let script_asset = obj
                .data
                .get("m_Script")
                .and_then(|v| v.get("guid"))
                .and_then(Value::as_str)
                .map(str::parse)
                .transpose()?
                .and_then(|guid| index.assets.get(&guid))
                .map(|a| index.label(&a.path));
            let mut fields = BTreeMap::new();
            for (key, value) in obj.data.as_mapping().into_iter().flatten() {
                if let Some(key) = key.as_str() {
                    if crate::unity::is_script_engine_field(key) {
                        continue;
                    }
                    let value = serde_json::to_value(value).map_err(|e| {
                        Error::new(
                            "REPORT_VALUE_ERROR",
                            format!("{label}, {}.{key}: {e}", obj.file_id),
                        )
                    })?;
                    fields.insert(key.into(), value);
                }
            }
            report.scripts.push(ScriptSummary {
                asset: label.into(),
                file_id: obj.file_id,
                script_asset,
                script_reference,
                script_identity,
                node_file_id: obj
                    .data
                    .get("m_GameObject")
                    .and_then(|v| v.get("fileID"))
                    .and_then(Value::as_i64),
                enabled: obj.data.get("m_Enabled").and_then(bool_value),
                fields,
            });
        }
    }
    Ok(())
}
