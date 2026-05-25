//! iter-80 Theme A AC: `cli_help_groups_commands_by_role`.
//!
//! Asserts that `ff-rdp --help` prints the four command-group section headers
//! (`Inspect`, `Navigate`, `Trace`, `Lifecycle`) so the grouping survives a
//! clap refactor or `about`/`long_about` edit. Case-insensitive match.

use std::process::Command;

#[test]
fn cli_help_groups_commands_by_role() {
    let bin = env!("CARGO_BIN_EXE_ff-rdp");
    let output = Command::new(bin)
        .arg("--help")
        .output()
        .expect("failed to spawn ff-rdp --help");

    assert!(
        output.status.success(),
        "ff-rdp --help must exit 0; status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lower = stdout.to_lowercase();
    for heading in ["inspect", "navigate", "trace", "lifecycle"] {
        assert!(
            lower.contains(heading),
            "ff-rdp --help must include section header {heading:?}; got:\n{stdout}"
        );
    }
}
