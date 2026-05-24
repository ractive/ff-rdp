//! Integration tests for `find-iteration-plan` resolver.
//!
//! Tests that use the REAL `kb/iterations/` directory verify against plans
//! that actually exist on this branch. Tests for error cases use a synthetic
//! temporary directory.

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

fn xtask_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_xtask") {
        return PathBuf::from(p);
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .ancestors()
        .find(|p| p.join("target").is_dir())
        .map(|p| p.join("target"))
        .unwrap_or_else(|| manifest_dir.join("../../target"));
    let debug = target_dir.join("debug").join("xtask");
    let release = target_dir.join("release").join("xtask");
    if debug.exists() { debug } else { release }
}

fn run_find_plan(branch: &str, repo_root_override: Option<&Path>) -> std::process::Output {
    let mut cmd = Command::new(xtask_bin());
    cmd.arg("find-iteration-plan").arg("--branch").arg(branch);
    if let Some(root) = repo_root_override {
        cmd.arg("--repo-root").arg(root);
    }
    cmd.current_dir(repo_root())
        .output()
        .expect("run xtask find-iteration-plan")
}

// ---------------------------------------------------------------------------
// Tests using the REAL kb/iterations/ directory
// ---------------------------------------------------------------------------

/// `iter-61b/recorder-cli-wiring` resolves to `iteration-61b-recorder-cli-wiring.md`.
/// Uses iter-61b because it has exactly one plan file (unlike iter-75b which has two).
#[test]
fn find_iteration_plan_letter_suffix() {
    let root = repo_root();
    let expected = root.join("kb/iterations/iteration-61b-recorder-cli-wiring.md");

    // Only run if the plan file actually exists.
    if !expected.exists() {
        eprintln!("SKIP: plan file not found at {}", expected.display());
        return;
    }

    let out = run_find_plan("iter-61b/recorder-cli-wiring", None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "should resolve iter-61b plan.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    let resolved = stdout.trim();
    let resolved_path = Path::new(resolved);
    assert!(
        resolved_path
            .file_name()
            .map(|n| n == "iteration-61b-recorder-cli-wiring.md")
            .unwrap_or(false),
        "expected filename iteration-61b-recorder-cli-wiring.md, got: {resolved}"
    );
}

/// `iter-77/spec-drift-and-windows-reparse-points` resolves to
/// `iteration-77-spec-drift-and-windows-reparse-points.md`.
#[test]
fn find_iteration_plan_pure_integer() {
    let root = repo_root();
    let expected = root.join("kb/iterations/iteration-77-spec-drift-and-windows-reparse-points.md");

    if !expected.exists() {
        eprintln!("SKIP: plan file not found at {}", expected.display());
        return;
    }

    let out = run_find_plan("iter-77/spec-drift-and-windows-reparse-points", None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "should resolve iter-77 plan.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    let resolved = stdout.trim();
    assert!(
        resolved.ends_with("iteration-77-spec-drift-and-windows-reparse-points.md"),
        "expected the iter-77 plan path, got: {resolved}"
    );
}

// ---------------------------------------------------------------------------
// Tests using synthetic temporary directories (error cases)
// ---------------------------------------------------------------------------

/// A branch for a non-existent iteration produces a clear error.
#[test]
fn find_iteration_plan_no_match_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kb_dir = tmp.path().join("kb").join("iterations");
    fs::create_dir_all(&kb_dir).unwrap();

    let out = run_find_plan("iter-99/nonexistent-plan", Some(tmp.path()));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "should exit non-zero for a branch with no matching plan.\n--- combined ---\n{combined}"
    );
    assert!(
        combined.contains("no plan found") || combined.contains("iteration-99"),
        "expected an actionable error message, got: {combined:?}"
    );
}

/// A non-iter-* branch produces a clear error.
#[test]
fn find_iteration_plan_non_iter_branch() {
    let out = run_find_plan("main", None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "should exit non-zero for non-iter-* branch.\n--- combined ---\n{combined}"
    );
    assert!(
        combined.contains("not an iter-* branch"),
        "expected 'not an iter-* branch' error, got: {combined:?}"
    );
}

/// Multiple plans for the same iteration ID produces a clear error.
#[test]
fn find_iteration_plan_multiple_matches_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kb_dir = tmp.path().join("kb").join("iterations");
    fs::create_dir_all(&kb_dir).unwrap();

    // Create two plans with the same iteration ID.
    fs::write(
        kb_dir.join("iteration-88-first-slug.md"),
        "---\ntitle: first\n---\n",
    )
    .unwrap();
    fs::write(
        kb_dir.join("iteration-88-second-slug.md"),
        "---\ntitle: second\n---\n",
    )
    .unwrap();

    let out = run_find_plan("iter-88/some-branch", Some(tmp.path()));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "should exit non-zero when multiple plans match.\n--- combined ---\n{combined}"
    );
    assert!(
        combined.contains("multiple plans") || combined.contains("iteration-88"),
        "expected a 'multiple plans' error, got: {combined:?}"
    );
}

// ---------------------------------------------------------------------------
// Unit tests for parse_iter_id (via the xtask library, tested inline)
// ---------------------------------------------------------------------------

// We can't import xtask as a library from an integration test, but we can
// test the resolver logic by exercising it through the binary. The branch
// format tests above cover this implicitly. For completeness, the unit tests
// live alongside the source in `find_iteration_plan.rs` itself.
