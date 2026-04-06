//! Tests for the `launch` command.
//!
//! We cannot actually launch Firefox in CI, so these tests focus on:
//! - CLI argument parsing (--help, flag combinations)
//! - `build_command` argument construction (white-box unit tests via `pub(crate)`)
//! - Graceful failure when given a non-existent binary path
//!
//! A live-Firefox integration test is left for local developer use and is
//! gated behind the `live_firefox` env-var pattern to avoid CI noise.

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

// ---------------------------------------------------------------------------
// CLI argument-parsing smoke tests (no Firefox needed)
// ---------------------------------------------------------------------------

#[test]
fn launch_help_exits_zero() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        output.status.success(),
        "expected zero exit for --help, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("headless") || stdout.contains("Launch"),
        "help output should mention launch flags: {stdout}"
    );
}
