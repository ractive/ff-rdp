//! AC `e2e_flag_subcommand_hints` (iter-132 Theme D): `scroll --bottom` and
//! `dom --stats` are natural-but-wrong guesses for what are actually
//! subcommands (`scroll bottom`, `dom stats`). clap rejects the flag before
//! any connection is attempted, so these run the compiled binary directly —
//! no mock server needed.

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

#[test]
fn scroll_bottom_flag_hints_at_scroll_bottom_subcommand() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["scroll", "--bottom"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(output.status.code(), Some(2), "clap usage-error exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ff-rdp scroll bottom"),
        "stderr must suggest `ff-rdp scroll bottom`, got: {stderr}"
    );
}

#[test]
fn scroll_top_flag_hints_at_scroll_top_subcommand() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["scroll", "--top"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(output.status.code(), Some(2), "clap usage-error exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ff-rdp scroll top"),
        "stderr must suggest `ff-rdp scroll top`, got: {stderr}"
    );
}

/// The specific regression this AC targets: the default clap "did you
/// mean" tip for `dom --stats` points at `--attrs`, which is a real flag
/// but not what the user meant — following it silently produces a
/// different, wrong command instead of an error. That tip must be gone;
/// only the corrected `dom stats` hint may appear.
#[test]
fn dom_stats_flag_hints_at_dom_stats_subcommand_no_attrs_tip() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["dom", "--stats"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(output.status.code(), Some(2), "clap usage-error exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ff-rdp dom stats"),
        "stderr must suggest `ff-rdp dom stats`, got: {stderr}"
    );
    assert!(
        !stderr.contains("--attrs"),
        "the misleading '--attrs' tip must not appear, got: {stderr}"
    );
}

/// Same trap with a selector present (`dom sel --stats`) — the selector
/// positional must not change whether the hint fires.
#[test]
fn dom_stats_flag_with_selector_hints_at_dom_stats_subcommand() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["dom", "h1", "--stats"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(output.status.code(), Some(2), "clap usage-error exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ff-rdp dom stats"),
        "stderr must suggest `ff-rdp dom stats`, got: {stderr}"
    );
    assert!(
        !stderr.contains("--attrs"),
        "the misleading '--attrs' tip must not appear, got: {stderr}"
    );
}

/// An unrelated unknown flag on a subcommand with no trap-table entry must
/// keep clap's normal behavior — no false-positive hint text.
#[test]
fn unrelated_unknown_flag_keeps_default_clap_error() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["eval", "--bogus"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert_eq!(output.status.code(), Some(2), "clap usage-error exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("is a subcommand, not a flag"),
        "unrelated unknown flags must not get the subcommand-trap hint, got: {stderr}"
    );
}
