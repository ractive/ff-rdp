mod check_dead_primitives;
mod check_discipline_regression;
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
    /// Verify the ralph-loop skill scripts mirror is in sync and replay
    /// baselines (iter-61v fails, iter-61t passes) still hold.
    CheckDisciplineRegression(check_discipline_regression::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::CheckDeadPrimitives(args) => check_dead_primitives::run(args),
        Commands::CheckTodoAnnotations(args) => check_todo_annotations::run(args),
        Commands::CheckIterationPlan(args) => check_iteration_plan::run(args),
        Commands::CheckDisciplineRegression(args) => check_discipline_regression::run(args),
    }
}
