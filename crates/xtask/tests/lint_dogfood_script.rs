//! Integration tests for `tools/lint-dogfood-script.sh`.
//!
//! Each test invokes `bash tools/lint-dogfood-script.sh <fixture>` and asserts:
//!   - exit code (0 = clean, 1 = lint errors)
//!   - stderr contains the expected rule tag
//!
//! Fixtures live in `tools/tests/lint-dogfood-script/`.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success());
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

fn lint_script() -> PathBuf {
    repo_root().join("tools/lint-dogfood-script.sh")
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("tools/tests/lint-dogfood-script")
        .join(name)
}

/// Run the linter against a fixture file. Returns (exit_success, combined_output).
fn run_linter(fixture_name: &str) -> (bool, String) {
    let script = lint_script();
    let fix = fixture(fixture_name);
    assert!(
        script.exists(),
        "lint-dogfood-script.sh not found: {script:?}"
    );
    assert!(fix.exists(), "fixture not found: {fix:?}");

    let out = Command::new("bash")
        .arg(&script)
        .arg(&fix)
        .current_dir(repo_root())
        .output()
        .expect("run lint-dogfood-script.sh");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

/// `unit_lint_dogfood_script_flags_unanchored_grep`:
/// The iter-86 Theme B grep (`grep -qi 'headless'`) must trigger the
/// unanchored-grep rule — error message must mention "anchored" or "false-positive".
#[test]
fn unit_lint_dogfood_script_flags_unanchored_grep() {
    let (ok, combined) = run_linter("unanchored-grep-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for unanchored-grep-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[unanchored-grep]"),
        "expected [unanchored-grep] tag in output.\noutput: {combined}"
    );
    assert!(
        combined.contains("anchored")
            || combined.contains("false-positive")
            || combined.contains("false positive"),
        "expected 'anchored' or 'false-positive' in diagnostic.\noutput: {combined}"
    );
}

/// `unit_lint_dogfood_script_flags_boolean_flag_with_positional`:
/// The iter-86 Theme D bug (`--jq-strict '.results.does_not_exist_xyz'`) must
/// trigger the bool-flag-positional rule — error message must mention the flag.
#[test]
fn unit_lint_dogfood_script_flags_boolean_flag_with_positional() {
    let (ok, combined) = run_linter("bool-flag-positional-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for bool-flag-positional-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[bool-flag-positional]"),
        "expected [bool-flag-positional] tag in output.\noutput: {combined}"
    );
    assert!(
        combined.contains("--jq-strict"),
        "expected '--jq-strict' mentioned in diagnostic.\noutput: {combined}"
    );
}

/// `unit_lint_dogfood_script_requires_set_euo_pipefail`:
/// A script without `set -euo pipefail` must trigger missing-set-euo-pipefail.
#[test]
fn unit_lint_dogfood_script_requires_set_euo_pipefail() {
    let (ok, combined) = run_linter("missing-set-euo-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for missing-set-euo-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[missing-set-euo-pipefail]"),
        "expected [missing-set-euo-pipefail] tag in output.\noutput: {combined}"
    );
}

/// `unit_lint_dogfood_script_requires_sentinel_pattern`:
/// A script without the SENTINEL pattern must trigger missing-sentinel-pattern.
#[test]
fn unit_lint_dogfood_script_requires_sentinel_pattern() {
    let (ok, combined) = run_linter("missing-sentinel-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for missing-sentinel-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[missing-sentinel-pattern]"),
        "expected [missing-sentinel-pattern] tag in output.\noutput: {combined}"
    );
}

/// A script that passes all rules must exit 0.
#[test]
fn unit_lint_dogfood_script_good_fixture_passes() {
    let (ok, combined) = run_linter("all-rules-good.sh");
    assert!(
        ok,
        "expected lint PASS for all-rules-good.sh, got failure.\noutput: {combined}"
    );
    assert!(
        combined.contains("OK"),
        "expected 'OK' in lint output.\noutput: {combined}"
    );
}

/// `lint_flags_iter86_assertions_before_fix`:
/// The original iter-86 dogfood script (before Theme E fixes) must fail the linter.
/// This is the pre_fix_repro_test for Theme E — verifies the bug exists before fix.
///
/// Note: Since we're testing the FIXED version in this iteration, this test verifies
/// the linter correctly identifies bugs in a known-bad fixture that mirrors the
/// original iter-86 patterns.
#[test]
fn lint_flags_iter86_assertions_before_fix() {
    // Use the unanchored-grep-bad.sh and bool-flag-positional-bad.sh fixtures which
    // replicate the exact iter-86 bugs verbatim.
    let (unanchored_fails, _) = run_linter("unanchored-grep-bad.sh");
    let (boolflag_fails, _) = run_linter("bool-flag-positional-bad.sh");

    assert!(
        !unanchored_fails,
        "unanchored-grep fixture (iter-86 Theme B pattern) must FAIL linting"
    );
    assert!(
        !boolflag_fails,
        "bool-flag-positional fixture (iter-86 Theme D pattern) must FAIL linting"
    );
}
