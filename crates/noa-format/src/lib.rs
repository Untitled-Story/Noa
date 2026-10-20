//! Shared definitions for Noa IR (.noir) and Noa Package (.noap).
//!
//! The protocol draft lives in `schemas/noir.fbs` at the workspace root.
//! Early scene fragments remain available for inspection. The generated wire
//! objects provide complete NOIR encoding/decoding; Mesh assets and package
//! manifests have independent validation.

pub mod ir;
pub mod mesh;

#[allow(clippy::all, unused_imports, non_snake_case)]
mod generated {
    pub mod mesh_generated;
    pub mod noir_generated;
}

/// Generated schema-native objects and verified FlatBuffers readers.
pub use generated::noir_generated::noa::ir as wire;
pub mod document;

pub mod package;

mod curves;
mod finite;

mod animation;

mod rendering;
