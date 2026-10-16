use crate::{
    diagnostics::Error,
    unity::{Guid, parse_meta_guid},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

mod colors;
mod scripts;

pub use colors::resolve_color_context;
pub use scripts::{MetadataCatalog, ScriptReference, ScriptResolution, ScriptResolver};

#[derive(Debug, Clone)]
pub struct IndexedAsset {
    pub guid: Guid,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct AssetIndex {
    pub roots: Vec<PathBuf>,
    pub assets: BTreeMap<Guid, IndexedAsset>,
}

impl AssetIndex {
    pub fn discover(prefab: &Path, additional: &[PathBuf]) -> Result<Self, Error> {
        let prefab = fs::canonicalize(prefab).map_err(|e| Error::io(prefab, e))?;
        if prefab.extension().and_then(|v| v.to_str()) != Some("prefab") || !prefab.is_file() {
            return Err(Error::new("INVALID_ENTRY", "entry must be a .prefab file"));
        }
        let parent = prefab
            .parent()
            .ok_or_else(|| Error::new("INVALID_ENTRY", "prefab has no parent directory"))?;
        let default = parent
            .ancestors()
            .find_map(|p| {
                (p.join("Assets").is_dir() && p.join("ProjectSettings").is_dir())
                    .then(|| p.join("Assets"))
            })
            .unwrap_or_else(|| parent.to_path_buf());
        let mut roots = vec![fs::canonicalize(&default).map_err(|e| Error::io(&default, e))?];
        for root in additional {
            let root = fs::canonicalize(root).map_err(|e| Error::io(root, e))?;
            if !root.is_dir() {
                return Err(Error::new(
                    "INVALID_SEARCH_ROOT",
                    format!("{} is not a directory", root.display()),
                ));
            }
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        let mut index = Self {
            roots,
            assets: BTreeMap::new(),
        };
        for root in &index.roots {
            for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
                let entry = entry.map_err(|e| Error::new("SCAN_ERROR", e.to_string()))?;
                if !entry.file_type().is_file()
                    || entry.path().extension().and_then(|s| s.to_str()) != Some("meta")
                {
                    continue;
                }
                let meta = entry.path();
                let asset = meta.with_extension("");
                if !asset.is_file() {
                    continue;
                } // folder metadata is not a loadable asset
                let text = fs::read_to_string(meta).map_err(|e| Error::io(meta, e))?;
                let guid = parse_meta_guid(&text)
                    .map_err(|e| Error::new(e.code, format!("{}: {e}", meta.display())))?;
                if guid.is_zero() || guid.is_builtin() {
                    return Err(Error::new(
                        "RESERVED_GUID",
                        format!("{} uses reserved GUID {guid}", meta.display()),
                    ));
                }
                let asset = fs::canonicalize(&asset).map_err(|e| Error::io(&asset, e))?;
                if let Some(previous) = index.assets.get(&guid) {
                    if previous.path != asset {
                        return Err(Error::new(
                            "DUPLICATE_GUID",
                            format!(
                                "GUID {guid}: {} and {}",
                                previous.path.display(),
                                asset.display()
                            ),
                        ));
                    }
                } else {
                    index
                        .assets
                        .insert(guid.clone(), IndexedAsset { guid, path: asset });
                }
            }
        }
        Ok(index)
    }

    pub fn label(&self, path: &Path) -> String {
        for (i, root) in self.roots.iter().enumerate() {
            if let Ok(relative) = path.strip_prefix(root) {
                return format!("root{i}/{}", relative.to_string_lossy().replace('\\', "/"));
            }
        }
        path.to_string_lossy().into_owned()
    }
}
