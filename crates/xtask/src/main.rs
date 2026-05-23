mod check_dead_primitives;
mod check_iteration_plan;
mod check_todo_annotations;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Iteration discipline tooling for ff-rdp")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::enum_variant_names)]
enum Commands {
    /// Check for new public items that have no non-test consumers in the workspace.
    CheckDeadPrimitives(check_dead_primitives::Args),
    /// Check that new TODO/FIXME/XXX annotations have issue links or explicit allow comments.
    CheckTodoAnnotations(check_todo_annotations::Args),
    /// Validate an iteration plan's frontmatter and required sections.
    CheckIterationPlan(check_iteration_plan::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::CheckDeadPrimitives(args) => check_dead_primitives::run(args),
        Commands::CheckTodoAnnotations(args) => check_todo_annotations::run(args),
        Commands::CheckIterationPlan(args) => check_iteration_plan::run(args),
    }
}
