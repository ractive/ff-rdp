//! Integration test: `live_check_pre_fix_repro_does_not_mutate_working_tree`
//!
//! Requires `FF_RDP_LIVE_TESTS=1`.  Gated with `#[ignore]` so it only runs via
//! `cargo test-live`.
//!
//! Verifies that running `check-pre-fix-repro` against a real plan does not
//! leave any modifications in the working tree (the old stash/checkout approach
//! mutated the tree; the new worktree approach must not).

use std::fs;
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

fn git_status_porcelain(repo: &std::path::Path) -> String {
    let out = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--porcelain"])
        .output()
        .expect("git status --porcelain");
    String::from_utf8(out.stdout).unwrap()
}

/// `live_check_pre_fix_repro_does_not_mutate_working_tree`:
/// Run the check-pre-fix-repro gate against iter-89's plan and assert that
/// git status --porcelain is identical before and after.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and git worktree support (live test)"]
fn live_check_pre_fix_repro_does_not_mutate_working_tree() {
    if std::env::var("FF_RDP_LIVE_TESTS").as_deref() != Ok("1") {
        return;
    }

    let repo = repo_root();
    let before = git_status_porcelain(&repo);

    // Use iter-89's plan — it has no pre_fix_repro_test annotations, so the
    // gate will SKIP immediately. That still exercises the worktree resolution
    // path without requiring a real cargo run on main.
    let plan = repo.join("kb/iterations/iteration-89-screenshot-fifth-attempt-single-theme.md");
    assert!(plan.exists(), "iter-89 plan not found at {plan:?}");

    // Find the xtask binary.
    let xtask_bin = if let Ok(p) = std::env::var("CARGO_BIN_EXE_xtask") {
        PathBuf::from(p)
    } else {
        let target = repo.join("target");
        let debug = target.join("debug/xtask");
        let release = target.join("release/xtask");
        if debug.exists() { debug } else { release }
    };

    let tmp = tempfile::TempDir::new().unwrap();

    let status = Command::new(&xtask_bin)
        .args(["check-pre-fix-repro", "--plan", &plan.to_string_lossy()])
        .env("FF_RDP_PRE_FIX_REPRO_CACHE_DIR", tmp.path())
        .status()
        .expect("failed to run xtask check-pre-fix-repro");

    // SKIP is exit 0; we don't assert on the exit code since the plan might
    // have no annotations (SKIP) or succeed — both are fine.
    let _ = status;

    let after = git_status_porcelain(&repo);
    assert_eq!(
        before, after,
        "git working tree was mutated by check-pre-fix-repro!\nbefore:\n{before}\nafter:\n{after}"
    );

    // Also verify that the iter-91 plan itself (with annotation) does not
    // mutate the working tree.
    let plan91_glob: Vec<_> = fs::read_dir(repo.join("kb/iterations"))
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("iteration-91-")
                && e.file_name().to_string_lossy().ends_with(".md")
                && !e.file_name().to_string_lossy().ends_with(".dogfood.sh")
        })
        .map(|e| e.path())
        .collect();

    if let Some(plan91) = plan91_glob.first() {
        let before2 = git_status_porcelain(&repo);
        let _status2 = Command::new(&xtask_bin)
            .args(["check-pre-fix-repro", "--plan", &plan91.to_string_lossy()])
            .env("FF_RDP_PRE_FIX_REPRO_CACHE_DIR", tmp.path())
            .env("FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE", "live_tree_test_sha")
            .status()
            .expect("failed to run xtask check-pre-fix-repro on iter-91 plan");
        let after2 = git_status_porcelain(&repo);
        assert_eq!(
            before2, after2,
            "git working tree was mutated by iter-91 plan run!\nbefore:\n{before2}\nafter:\n{after2}"
        );
    }
}
