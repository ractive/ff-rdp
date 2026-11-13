//! `xtask` holds only what cannot be a plain `cargo test`: the live-Firefox
//! sweep (watchdog, process-group kill, leftover-Firefox cleanup, stack
//! capture, preexisting-instance routing, quarantine). Every other repo check
//! is an ordinary test since DEC-058.

mod live_sweep;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Live-Firefox test sweep for ff-rdp")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::LiveSweep(args) => live_sweep::run(&args),
    }
}
