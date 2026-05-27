mod check_actor_kb_sync;
mod check_daemon_locks;
mod check_dead_primitives;
mod check_discipline_regression;
mod check_dogfood_script;
mod check_firefox_refs;
mod check_iteration_plan;
mod check_iteration_ready;
mod check_oneway_conformance;
mod check_todo_annotations;
mod find_iteration_plan;

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
    /// Fail if `.lock().unwrap()` remains in the daemon (must use `lock_or_recover!`).
    CheckDaemonLocks(check_daemon_locks::Args),
    /// Validate firefox_refs line ranges in an iteration plan against the local Firefox checkout.
    CheckFirefoxRefs(check_firefox_refs::Args),
    /// Fail if an actor source file was changed without a corresponding kb/rdp/actors/*.md update.
    CheckActorKbSync(check_actor_kb_sync::Args),
    /// Fail if any actor_request call targets a method declared oneway: true in the Firefox spec.
    CheckOnewayConformance(check_oneway_conformance::Args),
    /// Run all iteration discipline gates and aggregate results.
    /// Fails if any sub-check fails; reports all failures before exiting.
    CheckIterationReady(check_iteration_ready::Args),
    /// Execute the iteration plan's dogfood_script and verify the sentinel is written.
    /// Skips gracefully if FF_RDP_LIVE_TESTS != "1" or no dogfood_script field is set.
    CheckDogfoodScript(check_dogfood_script::Args),
    /// Resolve a branch name (iter-N/slug) to the absolute path of its iteration plan.
    FindIterationPlan(find_iteration_plan::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::CheckDeadPrimitives(args) => check_dead_primitives::run(args),
        Commands::CheckTodoAnnotations(args) => check_todo_annotations::run(args),
        Commands::CheckIterationPlan(args) => check_iteration_plan::run(args),
        Commands::CheckDisciplineRegression(args) => check_discipline_regression::run(args),
        Commands::CheckDaemonLocks(args) => check_daemon_locks::run(args),
        Commands::CheckFirefoxRefs(args) => check_firefox_refs::run(args),
        Commands::CheckActorKbSync(args) => check_actor_kb_sync::run(args),
        Commands::CheckOnewayConformance(args) => check_oneway_conformance::run(args),
        Commands::CheckIterationReady(args) => check_iteration_ready::run(args),
        Commands::FindIterationPlan(args) => find_iteration_plan::run(args),
        Commands::CheckDogfoodScript(args) => check_dogfood_script::run(args),
    }
}
