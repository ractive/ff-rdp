//! Integration tests for `check-iteration-ready` aggregator.
//!
//! Each test builds a minimal synthetic git repository and invokes the
//! `xtask check-iteration-ready` binary. Because the sub-checks shell out
//! to `cargo run -q -p xtask`, these tests are slower but exercise the real
//! aggregation path including ac-fidelity-check.sh.
//!
//! Skipped on Windows — the ac-fidelity check is bash-only.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

/// Path to the compiled xtask binary.
fn xtask_bin() -> PathBuf {
    // CARGO_BIN_EXE_xtask is set by cargo test for integration tests.
    // If not set (running via `cargo run`), fall back to looking it up.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_xtask") {
        return PathBuf::from(p);
    }
    // Reconstruct the path from CARGO_TARGET_DIR or the default.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .ancestors()
        .find(|p| p.join("target").is_dir())
        .map(|p| p.join("target"))
        .unwrap_or_else(|| manifest_dir.join("../../target"));
    // Try debug first, then release.
    let debug = target_dir.join("debug").join("xtask");
    let release = target_dir.join("release").join("xtask");
    if debug.exists() { debug } else { release }
}

/// A minimal plan with no ticked ACs and no firefox_refs.
fn clean_plan() -> &'static str {
    "\
---
title: \"Synthetic clean plan\"
type: iteration
date: 2026-05-24
status: in_progress
branch: iter-99/synthetic
tags:
- iteration
first_call_sites: []
dogfood_path: |
  cargo run -p xtask -- check-iteration-ready --plan this-plan.md
---

## Tasks

- [ ] nothing

## Acceptance Criteria [0/0]

## Design notes

No ACs, no firefox_refs — all sub-checks should pass.
"
}

/// Run `xtask check-iteration-ready` in the real repo root (not a sandbox),
/// pointing at the given plan path, with `--base HEAD` so the diff is empty.
fn run_aggregator_real_repo(plan_path: &Path, base: &str) -> std::process::Output {
    run_aggregator_real_repo_with_skips(plan_path, base, &[])
}

/// Same as `run_aggregator_real_repo` but with a list of sub-checks to skip.
fn run_aggregator_real_repo_with_skips(
    plan_path: &Path,
    base: &str,
    skip: &[&str],
) -> std::process::Output {
    let mut cmd = Command::new(xtask_bin());
    cmd.arg("check-iteration-ready")
        .arg("--plan")
        .arg(plan_path)
        .arg("--base")
        .arg(base);
    for s in skip {
        cmd.arg("--skip").arg(s);
    }
    cmd.current_dir(repo_root())
        .output()
        .expect("run xtask check-iteration-ready")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Happy path: synthetic plan with no ACs + clean diff → all 10 sub-checks PASS.
///
/// We run against the real repo root with `--base HEAD` so the code diff is
/// empty. That means dead-primitives, todo-annotations, and actor-kb-sync all
/// see nothing to complain about.  check-firefox-refs accepts the plan because
/// it has no `firefox_refs:` key. check-discipline-regression runs the mirror
/// check and replay baselines normally. ac-fidelity passes because there are no
/// ticked ACs.
#[test]
fn check_iteration_ready_happy_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.md");
    fs::write(&plan_path, clean_plan()).unwrap();

    // Skip check-discipline-regression because its replay baselines require
    // the full main history (iter-61t merge commit) which CI's shallow
    // checkout does not include. The standalone `discipline` CI job exercises
    // it separately and proves it works in isolation.
    //
    // Skip check-dogfood-script because the plan uses dogfood_path (not
    // dogfood_script) and the new fail-by-default logic in iter-87 causes it
    // to FAIL rather than SKIP on iter-* branches when FF_RDP_LIVE_TESTS is
    // unset. The live-tests CI job exercises the dogfood gate end-to-end.
    //
    // Skip check-pre-fix-repro and lint-dogfood-script because the synthetic
    // plan has no dogfood_script field and no pre_fix_repro_test annotations —
    // both would SKIP anyway, but skipping them here keeps the test fast.
    let out = run_aggregator_real_repo_with_skips(
        &plan_path,
        "HEAD",
        &[
            "check-discipline-regression",
            "check-dogfood-script",
            "check-pre-fix-repro",
            "lint-dogfood-script",
        ],
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        out.status.success(),
        "aggregator should exit 0 for a clean plan.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    // Final line must be "10/10 PASS" (10 sub-checks as of iter-100b, which
    // added check-live-test-layout).
    let last_meaningful = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    assert!(
        last_meaningful.contains("10/10 PASS"),
        "expected '10/10 PASS' as final summary line, got: {last_meaningful:?}\n--- combined ---\n{combined}"
    );
}

/// Regression test for the iter-74 failure mode: a plan with a ticked AC that
/// names a non-existent test function must cause the aggregator to exit 1 with
/// the ac-fidelity failure surfaced in its output.
///
/// Previously this failure only fired in Phase 2 *after* PR creation.
#[test]
fn regression_iter74_failure_mode() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.md");

    // Plan with one ticked AC naming a non-existent test.
    fs::write(
        &plan_path,
        "\
---
title: \"Synthetic iter-74 regression plan\"
type: iteration
date: 2026-05-24
status: in_progress
branch: iter-74/regression-fixture
tags:
- iteration
first_call_sites: []
dogfood_path: |
  cargo run -p xtask -- check-iteration-ready --plan this-plan.md
---

## Acceptance Criteria [1/1]

- [x] `live_nonexistent_xyzzy_iter74`: this test was never written.
",
    )
    .unwrap();

    let out = run_aggregator_real_repo(&plan_path, "HEAD");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "aggregator should exit 1 for a ticked AC naming a non-existent test.\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    // The ac-fidelity failure must be surfaced.
    assert!(
        combined.contains("live_nonexistent_xyzzy_iter74"),
        "expected the missing test slug in the output.\n--- combined ---\n{combined}"
    );
    // Confirm the ac-fidelity sub-check is the one that reported it.
    assert!(
        combined.contains("ac-fidelity"),
        "expected 'ac-fidelity' in the failure output.\n--- combined ---\n{combined}"
    );
    // Non-short-circuit: every sub-check header `[N/10]` must appear, proving
    // the aggregator continued past the failing sub-check rather than bailing
    // on the first one. This is the iter-74 regression's "see every issue at
    // once" requirement.
    for i in 1..=10 {
        let header = format!("[{i}/10]");
        assert!(
            combined.contains(&header),
            "expected sub-check header {header} in output (non-short-circuit).\n\
             --- combined ---\n{combined}"
        );
    }
}

/// Aggregation test: when a plan has a ticked AC naming a non-existent test
/// (causing ac-fidelity to fail), the aggregator exits 1, reports both the
/// sub-check name and the failure detail, and does NOT short-circuit — all
/// other sub-checks also run.
///
/// This exercises the "continue past first failure, collect all errors" requirement.
#[test]
fn check_iteration_ready_aggregates_failures() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.md");

    // A plan with a ticked AC naming a test that doesn't exist — this will
    // cause ac-fidelity-check to fail. The other 9 sub-checks should still
    // run (we can verify by seeing all 10 [N/10] lines in the output).
    fs::write(
        &plan_path,
        "\
---
title: \"Aggregates-failures synthetic plan\"
type: iteration
date: 2026-05-24
status: in_progress
branch: iter-99/synthetic-failure
tags:
- iteration
first_call_sites: []
dogfood_path: |
  cargo run -p xtask -- check-iteration-ready --plan this-plan.md
---

## Acceptance Criteria [1/1]

- [x] `live_totally_missing_function_xyzzy_aggfail`: this test was never written.
",
    )
    .unwrap();

    let out = run_aggregator_real_repo(&plan_path, "HEAD");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "aggregator should exit 1.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    // All 10 sub-check lines must appear — aggregator does NOT short-circuit.
    for i in 1..=10 {
        assert!(
            combined.contains(&format!("[{i}/10]")),
            "expected '[{i}/10]' in output (aggregator must not short-circuit).\n--- combined ---\n{combined}"
        );
    }

    // The ac-fidelity failure must be surfaced.
    assert!(
        combined.contains("ac-fidelity"),
        "expected 'ac-fidelity' sub-check in output.\n--- combined ---\n{combined}"
    );
    assert!(
        combined.contains("FAIL"),
        "expected FAIL in output.\n--- combined ---\n{combined}"
    );
}

/// Missing --plan argument produces a clear error and non-zero exit.
#[test]
fn missing_plan_arg() {
    let out = Command::new(xtask_bin())
        .arg("check-iteration-ready")
        .arg("--base")
        .arg("origin/main")
        .current_dir(repo_root())
        .output()
        .expect("run xtask");

    assert!(!out.status.success(), "should fail when --plan is missing");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // clap should emit an error about the missing required argument.
    assert!(
        combined.contains("plan") || combined.contains("required"),
        "expected a clear error about missing --plan, got: {combined:?}"
    );
}
