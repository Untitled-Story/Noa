//! Asset conversion independent of Unity's filesystem export layout.

mod builtin;
pub use builtin::{BuiltinMeshPlan, MeshAsset, MeshBinding, builtin_mesh, plan_builtin_meshes};
