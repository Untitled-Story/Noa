//! Unity resource inspection and compilation for Noa.
//!
//! Inspection resolves source references; compilation lowers the supported
//! Prefab subset into verified NOIR and a self-contained deterministic NOAP.

pub mod assets;
pub mod convert;
pub mod diagnostics;
pub mod inspect;
mod prepare;
pub mod resolver;
pub mod unity;

pub use inspect::{InspectOptions, Inspection, inspect, inspect_with_script_resolver};
pub use prepare::{DraftIr, prepare_ir, prepare_ir_with_script_resolver};

mod compile;
mod pack;
pub use compile::{CompileReport, compile, compile_to_path, compile_with_script_resolver};
pub use pack::Compilation;
