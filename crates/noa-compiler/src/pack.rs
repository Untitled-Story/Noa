//! Validated compilation output, manifests and deterministic atomic NOAP writes.
//! Scene conversion is orchestrated separately in `compile`.
use crate::diagnostics::{Diagnostic, Error};
use noa_format::{
    package::{Manifest, PackageFile},
    wire as w,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Seek, Write},
    path::Path,
};

#[derive(Debug)]
pub struct Compilation {
    pub document: w::DocumentT,
    pub files: BTreeMap<String, Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Compilation {
    pub fn manifest(&self) -> Result<Manifest, Error> {
        let mut files = Vec::new();
        for (path, bytes) in &self.files {
            let (asset_id, kind) = if path == "scene.noir" {
                (None, "noir".to_string())
            } else {
                let id = path
                    .strip_prefix("assets/")
                    .and_then(|v| v.split('.').next())
                    .and_then(|v| v.parse::<u32>().ok())
                    .ok_or_else(|| package_error("asset path", path))?;
                let asset = self
                    .document
                    .assets
                    .iter()
                    .flatten()
                    .find(|a| a.id == id)
                    .ok_or_else(|| package_error("asset", "missing declaration"))?;
                (
                    Some(id),
                    match asset.kind {
                        w::AssetKind::Image => "image",
                        w::AssetKind::Mesh => "mesh",
                        w::AssetKind::ShaderData => "shader_data",
                        _ => return Err(package_error("asset kind", "unsupported asset kind")),
                    }
                    .to_string(),
                )
            };
            files.push(PackageFile {
                path: path.clone(),
                kind,
                asset_id,
                byte_length: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            });
        }
        for asset in self.document.assets.iter().flatten() {
            if files
                .iter()
                .filter(|f| f.asset_id == Some(asset.id))
                .count()
                != 1
            {
                return Err(package_error(
                    "asset",
                    "must have exactly one packaged file",
                ));
            }
        }
        let manifest = Manifest {
            format: "noa-package".into(),
            version: 1,
            noir_version: [1, 0],
            document: "scene.noir".into(),
            files,
        };
        manifest
            .validate()
            .map_err(|e| package_error("manifest", e))?;
        Ok(manifest)
    }
    pub fn write(&self, output: &Path) -> Result<(), Error> {
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| Error::io(parent, e))?;
        self.write_archive(temp.as_file_mut())?;
        temp.as_file_mut()
            .sync_all()
            .map_err(|e| Error::io(output, e))?;
        temp.persist(output)
            .map_err(|e| Error::io(output, e.error))?;
        Ok(())
    }
    pub fn write_archive<W: Write + Seek>(&self, writer: W) -> Result<(), Error> {
        // Validate the public in-memory representation again before any output.
        let expected =
            noa_format::document::encode(&self.document).map_err(|e| package_error("NOIR", e))?;
        if self.files.get("scene.noir") != Some(&expected) {
            return Err(package_error("NOIR", "document and encoded bytes differ"));
        }
        let manifest = serde_json::to_vec_pretty(&self.manifest()?)
            .map_err(|e| package_error("manifest", e))?;
        let mut zip = zip::ZipWriter::new(writer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o644);
        zip.start_file("manifest.json", options)
            .map_err(|e| package_error("ZIP", e))?;
        zip.write_all(&manifest)
            .map_err(|e| package_error("ZIP", e))?;
        for (path, bytes) in &self.files {
            if path.contains("..") || path.starts_with('/') || path.contains('\\') {
                return Err(package_error("package path", path));
            }
            zip.start_file(path, options)
                .map_err(|e| package_error("ZIP", e))?;
            zip.write_all(bytes).map_err(|e| package_error("ZIP", e))?;
        }
        zip.finish().map_err(|e| package_error("ZIP", e))?;
        Ok(())
    }
}

fn package_error(at: &str, message: impl std::fmt::Display) -> Error {
    // Preserve the existing diagnostic contract when moving the writer.
    Error::new("UNSUPPORTED_EFFECT_DATA", format!("{at}: {message}"))
}
