//! NOAP v1 ZIP manifest. Paths are package-relative, using forward slashes.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub noir_version: [u16; 2],
    pub document: String,
    pub files: Vec<PackageFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageFile {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<u32>,
    pub byte_length: u64,
    pub sha256: String,
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != "noa-package"
            || self.version != 1
            || self.noir_version != [2, 0]
            || self.document != "scene.noir"
        {
            return Err("unsupported package version or document path".into());
        }
        let mut paths = std::collections::BTreeSet::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut documents = 0;
        for f in &self.files {
            if !paths.insert(&f.path) {
                return Err("duplicate package path".into());
            }
            if f.sha256.len() != 64
                || !f
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("invalid SHA-256".into());
            }
            if f.path == self.document {
                if f.kind != "noir" || f.asset_id.is_some() {
                    return Err("invalid document record".into());
                }
                documents += 1;
            } else {
                let id = f.asset_id.filter(|id| *id != 0).ok_or("missing asset ID")?;
                if !ids.insert(id) {
                    return Err("duplicate asset ID".into());
                }
                let extension = match f.kind.as_str() {
                    "image" => "png",
                    "mesh" => "nomesh",
                    "shader_data" => "shader.json",
                    _ => return Err("unknown asset kind".into()),
                };
                if f.path != format!("assets/{id}.{extension}") {
                    return Err("invalid asset path/kind".into());
                }
            }
        }
        if documents != 1 {
            return Err("package must contain one document".into());
        }
        Ok(())
    }
}
