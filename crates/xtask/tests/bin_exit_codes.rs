//! End-to-end exit-code tests for the `xtask` binary.
//!
//! These tests invoke the prebuilt `xtask` binary directly via
//! `CARGO_BIN_EXE_xtask` (an env var Cargo sets only for integration tests
//! and benches, not unit tests). That is why these live here rather than in
//! `check_dogfood_script`'s `#[cfg(test)]`
//! module: a unit test has no `current_exe` pointing at `xtask` (it points
//! at the test runner itself), so the only way to observe the binary's real
//! exit code/stdout from a unit test is to spawn `cargo run -p xtask -- ...`
//! as a child process — and that nested `cargo run` contends with the outer
//! `cargo test --workspace` for Cargo's build-directory lock, which stalled
//! a full workspace test run for 20+ minutes on a cold build (see iter-124
//! follow-up).

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Write a minimal plan file with the given extra frontmatter into `dir`.
fn write_plan(dir: &TempDir, name: &str, extra_fm: &str) -> PathBuf {
    let path = dir.path().join(name);
    let content = format!(
        "---\ntitle: \"Test Plan\"\nstatus: planned\ntype: iteration\n{extra_fm}---\n\n# Body\n"
    );
    std::fs::write(&path, content).unwrap();
    path
}

/// Write an executable shell script into `dir`.
fn write_script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "#!/usr/bin/env bash").unwrap();
    writeln!(f, "{body}").unwrap();
    // Mark executable on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// Write a dogfood script that satisfies every `tools/lint-dogfood-script.sh`
/// rule and writes the per-run sentinel the gate names in
/// `FF_RDP_DOGFOOD_SENTINEL` (iter-184; before that the path was hardcoded).
///
/// Since iter-162a `check-dogfood-script` lints the script before running it,
/// so any fixture that is meant to reach the execution stage has to lint clean.
/// `keep_sentinel = false` writes the sentinel and then deletes it again: that
/// is how a fixture stays lint-clean while still leaving the gate's sentinel
/// absent at the point the gate looks for it.
fn write_clean_dogfood_script(dir: &TempDir, name: &str, keep_sentinel: bool) -> PathBuf {
    let tail = if keep_sentinel {
        ""
    } else {
        "rm -f \"$SENTINEL\""
    };
    write_script(
        dir,
        name,
        &format!(
            "set -euo pipefail\n\
             SENTINEL=\"${{FF_RDP_DOGFOOD_SENTINEL:?not set}}\"\n\
             rm -f \"$SENTINEL\"\n\
             date -u > \"$SENTINEL\"\n\
             {tail}\n"
        ),
    )
}

#[test]
#[cfg(unix)]
fn xtask_check_dogfood_script_missing_sentinel() {
    // Script exits 0 but does NOT write the sentinel → run_script returns
    // an error via anyhow::bail!, which the xtask binary propagates as a
    // non-zero exit code. We invoke the prebuilt binary to observe the exit
    // code end-to-end.
    let dir = TempDir::new().unwrap();
    let plan_path = write_plan(
        &dir,
        "iteration-98-no-sentinel.md",
        "dogfood_script: no-sentinel.dogfood.sh\n",
    );
    // Lints clean, but removes the sentinel again before exiting 0, so the
    // path the gate handed it is empty when the gate checks.
    write_clean_dogfood_script(&dir, "no-sentinel.dogfood.sh", false);

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .env("FF_RDP_LIVE_TESTS", "1")
        .args(["check-dogfood-script", plan_path.to_str().unwrap()])
        .output()
        .unwrap();

    // Should have exited non-zero (missing sentinel).
    assert!(
        !output.status.success(),
        "expected failure when sentinel is missing"
    );
}

/// `live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch`:
/// When the branch is an iter-* branch and FF_RDP_LIVE_TESTS is unset,
/// check-dogfood-script must FAIL (not SKIP).
///
/// Uses FF_RDP_CURRENT_BRANCH override so this test does not depend on the
/// actual checked-out branch.
#[test]
fn live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch() {
    let dir = TempDir::new().unwrap();
    // Needs a dogfood_script field (and a lint-clean script) so we reach the
    // gate logic rather than tripping the lint sub-check first.
    write_clean_dogfood_script(&dir, "fake.dogfood.sh", true);
    let plan_path = write_plan(
        &dir,
        "iteration-95-branch-test.md",
        "dogfood_script: fake.dogfood.sh\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-dogfood-script", plan_path.to_str().unwrap()])
        .env("FF_RDP_CURRENT_BRANCH", "iter-99/test")
        .env_remove("FF_RDP_LIVE_TESTS")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected FAIL on iter-* branch w/o FF_RDP_LIVE_TESTS"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("FF_RDP_LIVE_TESTS"),
        "expected FF_RDP_LIVE_TESTS hint in output:\n{combined}"
    );
}

/// `live_check_dogfood_script_skips_on_main_without_ff_rdp_live_tests`:
/// On a non-iter-* branch (e.g. "main"), check-dogfood-script must SKIP
/// (exit 0) when FF_RDP_LIVE_TESTS is unset.
#[test]
fn live_check_dogfood_script_skips_on_main_without_ff_rdp_live_tests() {
    let dir = TempDir::new().unwrap();
    write_clean_dogfood_script(&dir, "fake.dogfood.sh", true);
    let plan_path = write_plan(
        &dir,
        "iteration-94-main-test.md",
        "dogfood_script: fake.dogfood.sh\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-dogfood-script", plan_path.to_str().unwrap()])
        .env("FF_RDP_CURRENT_BRANCH", "main")
        .env_remove("FF_RDP_LIVE_TESTS")
        .output()
        .unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "expected SKIP (exit 0) on non-iter-* branch w/o FF_RDP_LIVE_TESTS: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SKIP"),
        "expected SKIP message in stdout:\n{stdout}"
    );
}

/// `unit_162a_lint_dogfood_rehosted`: `check-dogfood-script` runs the
/// `lint-dogfood-script` sub-check itself.
///
/// Until iter-162a this lived in the `check-iteration-ready` aggregator, which
/// was the linter's only non-test caller. This test is what proves the linter
/// kept a caller after that deletion — it drives the binary end-to-end and
/// asserts the named result line appears.
///
/// Also covers the `--plan` spelling of the plan argument (the positional form
/// is covered by the tests above).
#[test]
fn xtask_check_dogfood_script_runs_lint() {
    let dir = TempDir::new().unwrap();
    // No dogfood_script field → the lint sub-check SKIPs (still a result line)
    // and the dogfood gate itself skips on a non-iter branch.
    let plan_path = write_plan(
        &dir,
        "iteration-96-test.md",
        "dogfood_path: \"ff-rdp --help\"\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "check-dogfood-script",
            "--plan",
            plan_path.to_str().unwrap(),
        ])
        .env("FF_RDP_CURRENT_BRANCH", "main")
        .env_remove("FF_RDP_LIVE_TESTS")
        .output()
        .unwrap();

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };

    assert!(
        output.status.success(),
        "expected exit 0 for a plan with no dogfood_script:\n{combined}"
    );
    assert!(
        combined.contains("lint-dogfood-script:"),
        "lint-dogfood-script result line missing from output:\n{combined}"
    );
}

/// A plan whose dogfood script violates a lint rule must fail
/// `check-dogfood-script` — the linter's verdict is load-bearing, not advisory.
#[test]
#[cfg(unix)]
fn xtask_check_dogfood_script_fails_on_lint_error() {
    let dir = TempDir::new().unwrap();
    write_script(&dir, "dirty.dogfood.sh", "echo no set -euo pipefail here");
    let plan_path = write_plan(
        &dir,
        "iteration-89-dirty.md",
        "dogfood_script: dirty.dogfood.sh\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-dogfood-script", plan_path.to_str().unwrap()])
        .env("FF_RDP_CURRENT_BRANCH", "main")
        .env_remove("FF_RDP_LIVE_TESTS")
        .output()
        .unwrap();

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };

    assert!(
        !output.status.success(),
        "expected non-zero exit when the dogfood script fails lint:\n{combined}"
    );
    assert!(
        combined.contains("lint-dogfood-script: FAIL"),
        "expected a FAIL result line:\n{combined}"
    );
}
