//! iter-79 Theme B AC: `ff-rdp dom --help` must mention `styles` and `computed`.
//!
//! Dogfooding (2026-05-25) found users reaching for `ff-rdp dom --include-style`
//! (does not exist) when `ff-rdp styles` and `ff-rdp computed` already do
//! exactly what was wanted. The cross-reference under `dom --help` is the
//! signpost; this test prevents the line from rotting silently.

use std::process::Command;

#[test]
fn dom_help_mentions_styles_and_computed() {
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"));
    let output = Command::new(&bin)
        .args(["dom", "--help"])
        .output()
        .expect("failed to run ff-rdp dom --help");
    assert!(
        output.status.success(),
        "ff-rdp dom --help exited non-zero: status={:?}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        stdout.contains("styles"),
        "ff-rdp dom --help must mention `styles` so users discover the sibling command.\nstdout was:\n{stdout}",
    );
    assert!(
        stdout.contains("computed"),
        "ff-rdp dom --help must mention `computed` so users discover the sibling command.\nstdout was:\n{stdout}",
    );
}
