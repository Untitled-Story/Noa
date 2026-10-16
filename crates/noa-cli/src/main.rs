use clap::{Parser, Subcommand};
use noa_compiler::{InspectOptions, inspect, prepare_ir};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(name = "noa", version, about = "Unity effect resource compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a self-contained Noa package.
    Compile {
        prefab: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long = "search")]
        search_roots: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Resolve a Prefab's dependencies and inspect its source components.
    Inspect {
        prefab: PathBuf,
        /// Additional dependency directory (may be repeated).
        #[arg(long = "search")]
        search_roots: Vec<PathBuf>,
        /// Emit a structured source inspection report to stdout.
        #[arg(long)]
        json: bool,
        /// Include partial compiler IR (not a complete .noir or playable package).
        #[arg(long)]
        ir: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}: {error}", error.code);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, noa_compiler::diagnostics::Error> {
    match cli.command {
        Command::Compile {
            prefab,
            output,
            search_roots,
            json,
        } => {
            let report =
                noa_compiler::compile_to_path(&prefab, &output, &InspectOptions { search_roots })?;
            if json {
                serde_json::to_writer_pretty(std::io::stdout().lock(), &report).map_err(|e| {
                    noa_compiler::diagnostics::Error::new("REPORT_WRITE_ERROR", e.to_string())
                })?;
            } else {
                println!(
                    "Written {}: {} nodes, {} components, {} assets, {} materials, {} animations",
                    report.output,
                    report.nodes,
                    report.components,
                    report.assets,
                    report.materials,
                    report.animations
                );
                for d in &report.diagnostics {
                    eprintln!("{:?} {}: {}", d.severity, d.code, d.message);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Inspect {
            prefab,
            search_roots,
            json,
            ir,
        } => {
            if ir {
                let draft = prepare_ir(&prefab, &InspectOptions { search_roots })?;
                let failed = draft
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == noa_compiler::diagnostics::Severity::Error);
                if json {
                    serde_json::to_writer_pretty(std::io::stdout().lock(), &draft).map_err(
                        |e| {
                            noa_compiler::diagnostics::Error::new(
                                "REPORT_WRITE_ERROR",
                                e.to_string(),
                            )
                        },
                    )?;
                } else {
                    println!(
                        "Nodes: {}, layouts: {}, animators: {}, animations: {}, custom components: {}",
                        draft.nodes.len(),
                        draft.layouts.len(),
                        draft.animators.len(),
                        draft.animations.len(),
                        draft.custom_components.len()
                    );
                    println!("Pending components: {}", draft.pending_components.len());
                    println!(
                        "Mesh assets: {}, bindings: {}",
                        draft.meshes.len(),
                        draft.mesh_bindings.len()
                    );
                    println!(
                        "Particle color fragments: {}, renderer color flags: {}",
                        draft.particle_colors.len(),
                        draft.renderer_colors.len()
                    );
                    for diagnostic in draft
                        .source_inspection
                        .diagnostics
                        .iter()
                        .chain(&draft.diagnostics)
                    {
                        println!(
                            "{:?} {}: {}",
                            diagnostic.severity, diagnostic.code, diagnostic.message
                        );
                    }
                    println!(
                        "Partial compiler IR only; use `noa compile` for the complete NOIR and package."
                    );
                }
                return Ok(if failed {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                });
            }
            let report = inspect(&prefab, &InspectOptions { search_roots })?;
            if json {
                // Use a writer to avoid panicking when a downstream pipe closes.
                serde_json::to_writer_pretty(std::io::stdout().lock(), &report).map_err(|e| {
                    noa_compiler::diagnostics::Error::new("REPORT_WRITE_ERROR", e.to_string())
                })?;
            } else {
                println!("Entry: {}", report.entry);
                println!(
                    "Resolved assets: {} ({} indexed)",
                    report.assets.len(),
                    report.indexed_asset_count
                );
                println!("Particle systems: {}", report.particles.len());
                println!("Script components: {}", report.scripts.len());
                for d in &report.diagnostics {
                    println!(
                        "{:?} {}: {} fileID={:?} {}: {}",
                        d.severity,
                        d.code,
                        d.asset,
                        d.file_id,
                        d.property.as_deref().unwrap_or(""),
                        d.message
                    );
                }
                println!(
                    "Source inspection only; use --ir for partial compiler IR. Use `noa compile` for .noap output."
                );
            }
            Ok(if report.has_errors() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}
