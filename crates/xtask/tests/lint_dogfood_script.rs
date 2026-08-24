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

/// `unit_lint_dogfood_script_flags_fixed_sentinel_path`:
/// A script still assigning the pre-iter-184 hardcoded `/tmp/ff-rdp-iter-<N>-dogfood-ok`
/// path must fail the linter — that path is shared by every concurrent gate run
/// for the same iteration, and the gate would never see the sentinel it named.
#[test]
fn unit_lint_dogfood_script_flags_fixed_sentinel_path() {
    let (ok, combined) = run_linter("fixed-sentinel-path-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for fixed-sentinel-path-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[fixed-sentinel-path]"),
        "expected [fixed-sentinel-path] tag in output.\noutput: {combined}"
    );
    assert!(
        combined.contains("FF_RDP_DOGFOOD_SENTINEL"),
        "diagnostic must name the replacement variable.\noutput: {combined}"
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
    let (unanchored_ok, _) = run_linter("unanchored-grep-bad.sh");
    let (boolflag_ok, _) = run_linter("bool-flag-positional-bad.sh");

    assert!(
        !unanchored_ok,
        "unanchored-grep fixture (iter-86 Theme B pattern) must FAIL linting"
    );
    assert!(
        !boolflag_ok,
        "bool-flag-positional fixture (iter-86 Theme D pattern) must FAIL linting"
    );
}

/// `unit_lint_dogfood_script_flags_unscoped_pkill`:
/// The opening line every checked-in dogfood script carried until iter-193 —
/// `pkill -f 'firefox.*ff-rdp-profile'` — must fail the linter, naming the rule.
/// The pattern is not scoped to the run, so on a machine where several agents
/// share one working tree it terminates a sibling's browser; that is precisely
/// why iter-184 could not execute a single migrated script to prove its own
/// migration.
#[test]
fn unit_lint_dogfood_script_flags_unscoped_pkill() {
    let (ok, combined) = run_linter("unscoped-pkill-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for unscoped-pkill-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[unscoped-pkill]"),
        "expected [unscoped-pkill] tag in output.\noutput: {combined}"
    );
    assert!(
        combined.contains("did not start"),
        "diagnostic must say why the kill is unscoped.\noutput: {combined}"
    );
}

/// The scoped-teardown form the rule points at must lint clean, so the rule is
/// actionable rather than merely prohibitive.
#[test]
fn unit_lint_dogfood_script_scoped_teardown_passes() {
    let (ok, combined) = run_linter("unscoped-pkill-good.sh");
    assert!(
        ok,
        "expected lint PASS for unscoped-pkill-good.sh, got failure.\noutput: {combined}"
    );
}

/// `unit_lint_dogfood_script_flags_path_binary`:
/// A script invoking a bare `ff-rdp` must fail the linter, naming the rule. A
/// PATH lookup can resolve a months-old install, letting the gate certify a
/// build that is not the one on the branch.
#[test]
fn unit_lint_dogfood_script_flags_path_binary() {
    let (ok, combined) = run_linter("path-binary-bad.sh");
    assert!(
        !ok,
        "expected lint FAIL for path-binary-bad.sh, got success.\noutput: {combined}"
    );
    assert!(
        combined.contains("[path-binary]"),
        "expected [path-binary] tag in output.\noutput: {combined}"
    );
    assert!(
        combined.contains("PATH"),
        "diagnostic must name $PATH as the problem.\noutput: {combined}"
    );
}

/// The `ffrdp` helper form must lint clean — and `cargo run -p ff-rdp-cli --`,
/// which contains the substring `ff-rdp`, must not be mistaken for a PATH
/// lookup (the `all-rules-good.sh` fixture uses exactly that spelling).
#[test]
fn unit_lint_dogfood_script_binary_under_test_passes() {
    let (ok, combined) = run_linter("path-binary-good.sh");
    assert!(
        ok,
        "expected lint PASS for path-binary-good.sh, got failure.\noutput: {combined}"
    );
    let (ok, combined) = run_linter("all-rules-good.sh");
    assert!(
        ok,
        "`cargo run -p ff-rdp-cli --` must not trip path-binary.\noutput: {combined}"
    );
}

/// Every checked-in `kb/iterations/*.dogfood.sh` must lint clean.
///
/// This is the regression guard for iteration 193's two headline claims — no
/// remaining unscoped `pkill`, no remaining bare `ff-rdp` — and it is
/// deliberately a sweep rather than a spot check: the defect was present in 14
/// of 16 scripts because each new script was copied from the last one.
#[test]
#[cfg(unix)]
fn unit_lint_dogfood_script_checked_in_scripts_stay_clean() {
    let dir = repo_root().join("kb/iterations");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read kb/iterations")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".dogfood.sh"))
        })
        .collect();
    scripts.sort();
    assert!(
        !scripts.is_empty(),
        "no .dogfood.sh scripts found in {dir:?}"
    );

    let out = Command::new("bash")
        .arg(lint_script())
        .args(&scripts)
        .current_dir(repo_root())
        .output()
        .expect("run lint-dogfood-script.sh");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "{} checked-in dogfood script(s) failed the linter.\noutput: {combined}",
        scripts.len()
    );
}
