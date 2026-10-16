use crate::{diagnostics::Error, unity::Guid};
use noa_format::ir::ScriptIdentity;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};
use walkdir::WalkDir;

/// The original Unity identity remains available even if metadata is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ScriptReference {
    pub guid: Option<Guid>,
    pub file_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptResolution {
    Resolved(ScriptIdentity),
    Missing,
    Conflict(Vec<ScriptIdentity>),
}

/// Other providers may resolve MonoScript directly from original Unity data.
/// Consumers do not depend on JSON or on C# source code parsing.
pub trait ScriptResolver {
    fn resolve(&self, source_asset: &str, reference: &ScriptReference) -> ScriptResolution;
}

#[derive(Debug, Default)]
pub struct MetadataCatalog {
    entries: BTreeMap<ScriptReference, BTreeSet<ScriptIdentity>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataFile {
    format: String,
    version: u32,
    scripts: Vec<MetadataEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataEntry {
    guid: String,
    // Decimal string in JSON: Unity fileIDs can exceed JS safe integers.
    file_id: String,
    assembly: String,
    namespace: String,
    class_name: String,
    source: Option<MetadataSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataSource {
    asset: String,
    file_id: String,
}

fn decimal_id(value: &str) -> Result<i64, Error> {
    let id = value.parse::<i64>().map_err(|_| {
        Error::new(
            "INVALID_SCRIPT_METADATA",
            format!("invalid int64 file_id: {value}"),
        )
    })?;
    if id == 0 || id.to_string() != value {
        return Err(Error::new(
            "INVALID_SCRIPT_METADATA",
            "file_id must be a nonzero canonical int64 decimal string",
        ));
    }
    Ok(id)
}

impl MetadataCatalog {
    /// Search only the dependency roots; never walk above the entry's scope.
    pub fn discover(roots: &[PathBuf]) -> Result<Self, Error> {
        let mut catalog = Self::default();
        let mut visited = BTreeSet::new();
        for root in roots {
            for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
                let entry = entry.map_err(|e| Error::new("SCAN_ERROR", e.to_string()))?;
                if entry.file_type().is_file()
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|n| n.ends_with(".monoscripts.json"))
                {
                    let path =
                        fs::canonicalize(entry.path()).map_err(|e| Error::io(entry.path(), e))?;
                    if visited.insert(path.clone()) {
                        let text = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
                        catalog
                            .add_json(&text)
                            .map_err(|e| Error::new(e.code, format!("{}: {e}", path.display())))?;
                    }
                }
            }
        }
        Ok(catalog)
    }

    pub fn add_json(&mut self, text: &str) -> Result<(), Error> {
        let metadata: MetadataFile = serde_json::from_str(text)
            .map_err(|e| Error::new("INVALID_SCRIPT_METADATA", e.to_string()))?;
        if metadata.format != "unity-monoscript-metadata" || metadata.version != 1 {
            return Err(Error::new(
                "UNSUPPORTED_SCRIPT_METADATA",
                "expected unity-monoscript-metadata version 1",
            ));
        }
        // Validate the entire file before changing the catalog.
        let mut entries = Vec::new();
        for entry in metadata.scripts {
            let guid: Guid = entry.guid.parse()?;
            if guid.is_zero()
                || guid.is_builtin()
                || entry.assembly.trim().is_empty()
                || entry.class_name.trim().is_empty()
            {
                return Err(Error::new(
                    "INVALID_SCRIPT_METADATA",
                    "non-reserved GUID, assembly and class_name are required",
                ));
            }
            if let Some(source) = entry.source {
                if source.asset.trim().is_empty() {
                    return Err(Error::new(
                        "INVALID_SCRIPT_METADATA",
                        "source.asset must not be empty",
                    ));
                }
                decimal_id(&source.file_id)?;
            }
            entries.push((
                ScriptReference {
                    guid: Some(guid),
                    file_id: decimal_id(&entry.file_id)?,
                },
                ScriptIdentity {
                    assembly: entry.assembly,
                    namespace: entry.namespace,
                    class_name: entry.class_name,
                },
            ));
        }
        for (reference, identity) in entries {
            self.entries.entry(reference).or_default().insert(identity);
        }
        Ok(())
    }
}

impl ScriptResolver for MetadataCatalog {
    fn resolve(&self, _source_asset: &str, reference: &ScriptReference) -> ScriptResolution {
        match self.entries.get(reference) {
            None => ScriptResolution::Missing,
            Some(identities) if identities.len() == 1 => {
                ScriptResolution::Resolved(identities.first().expect("one identity").clone())
            }
            Some(identities) => ScriptResolution::Conflict(identities.iter().cloned().collect()),
        }
    }
}
