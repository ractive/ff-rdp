mod check_actor_kb_sync;
mod check_dogfood_script;
mod check_firefox_refs;
mod check_iteration_plan;
mod check_live_test_layout;
mod check_source_invariants;
mod find_iteration_plan;
mod live_sweep;
mod skill_drift;
mod stderr_scan;

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
    /// Validate an iteration plan's frontmatter and required sections.
    CheckIterationPlan(check_iteration_plan::Args),
    /// Scan product source for three defect shapes: `.lock().unwrap()` in the daemon,
    /// `eprintln!` + `AppError::Exit(N)` that bypasses the JSON envelope, and any
    /// `eprintln!` under commands/ without a `// stderr-ok: <reason>` justification.
    CheckSourceInvariants(check_source_invariants::Args),
    /// Validate firefox_refs line ranges in an iteration plan against the local Firefox checkout.
    CheckFirefoxRefs(check_firefox_refs::Args),
    /// Fail if an actor source file was changed without a corresponding kb/rdp/actors/*.md update.
    CheckActorKbSync(check_actor_kb_sync::Args),
    /// Fail if any top-level crates/ff-rdp-cli/tests/live_*.rs binary exists
    /// (live suites must be modules of the consolidated tests/live/ target).
    CheckLiveTestLayout(check_live_test_layout::Args),
    /// Lint and execute the iteration plan's dogfood_script, verifying the sentinel is written.
    /// Skips gracefully if FF_RDP_LIVE_TESTS != "1" or no dogfood_script field is set.
    CheckDogfoodScript(check_dogfood_script::Args),
    /// Resolve a branch name (iter-N/slug) to the absolute path of its iteration plan.
    FindIterationPlan(find_iteration_plan::Args),
    /// Run the live-Firefox test suite so an unmet env gate reports `ignored`
    /// (libtest's own vocabulary) instead of a fake `ok` (iter-155). Prints a
    /// machine-readable `LIVE_SWEEP_SUMMARY executed=N skipped=M preexisting=K
    /// vanished=V launch_timeout=L timed_out=X total=T` line. `vanished` and
    /// `launch_timeout` are unmet preconditions (a port-6000 browser that went
    /// away mid-sweep, and a Firefox that never opened its debug port in time)
    /// rather than product failures (iter-173); `timed_out` counts tests whose
    /// phase the watchdog killed before libtest reported a verdict, and is
    /// always a red (iter-197).
    LiveSweep(live_sweep::Args),
    /// Regenerate the marked region of the bundled skill's SKILL.md from the
    /// CLI's own command tables (`ff_rdp_cli::commands::skill_doc`).
    GenSkill(skill_drift::GenArgs),
    /// Fail when the committed SKILL.md's generated region no longer matches
    /// what the CLI would generate. Wired into CI so the agent-facing skill
    /// cannot silently drift from the command surface it documents.
    CheckSkillDrift(skill_drift::CheckArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::CheckIterationPlan(args) => check_iteration_plan::run(args),
        Commands::CheckSourceInvariants(args) => check_source_invariants::run(args),
        Commands::CheckFirefoxRefs(args) => check_firefox_refs::run(args),
        Commands::CheckActorKbSync(args) => check_actor_kb_sync::run(args),
        Commands::CheckLiveTestLayout(args) => check_live_test_layout::run(args),
        Commands::FindIterationPlan(args) => find_iteration_plan::run(args),
        Commands::CheckDogfoodScript(args) => check_dogfood_script::run(args),
        Commands::LiveSweep(args) => live_sweep::run(args),
        Commands::GenSkill(args) => skill_drift::run_gen(args),
        Commands::CheckSkillDrift(args) => skill_drift::run_check(args),
    }
}
