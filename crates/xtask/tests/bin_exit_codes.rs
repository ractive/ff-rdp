//! End-to-end exit-code tests for the `xtask` binary.
//!
//! These tests invoke the prebuilt `xtask` binary directly via
//! `CARGO_BIN_EXE_xtask` (an env var Cargo sets only for integration tests
//! and benches, not unit tests). That is why these live here rather than in
//! `check_dogfood_script`'s `#[cfg(test)]` module: a unit test has no
//! `current_exe` pointing at `xtask` (it points at the test runner itself),
//! so the only way to observe the binary's real exit code from a unit test
//! is to spawn `cargo run -p xtask -- ...` as a child process — and that
//! nested `cargo run` contends with the outer `cargo test --workspace` for
//! Cargo's build-directory lock, which stalled a full workspace test run for
//! 20+ minutes on a cold build (see iter-124 follow-up).

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
    write_script(
        &dir,
        "no-sentinel.dogfood.sh",
        "# intentionally no sentinel",
    );

    // Pre-clean sentinel.
    let _ = std::fs::remove_file("/tmp/ff-rdp-iter-98-dogfood-ok");

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
    // Needs a dogfood_script field so we reach the gate logic.
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
