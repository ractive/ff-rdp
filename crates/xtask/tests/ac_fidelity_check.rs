//! Integration test for the iter-66 strengthening of `ac-fidelity-check.sh`.
//!
//! Verifies that a ticked Acceptance Criteria checkbox naming a test slug
//! which does NOT exist anywhere in the workspace is rejected by the script
//! with a non-zero exit code — exactly the failure mode iter-61w slipped past.
//!
//! The script is bash-only, so the whole module is skipped on Windows.
#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

fn script_path() -> PathBuf {
    repo_root().join("tools/ralph-loop/scripts/ac-fidelity-check.sh")
}

/// Make a sandbox git repo so the script's `git diff <range>` invocation
/// doesn't traverse out into the real repo and find unrelated symbols.
fn make_git_sandbox(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let s = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|_| panic!("git {args:?}"));
        assert!(s.status.success(), "git {args:?} failed: {s:?}");
    };
    run(&["init", "--quiet", "-b", "main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    // One initial commit so HEAD resolves and HEAD..HEAD is a valid empty range.
    fs::write(dir.join("README"), "seed\n").unwrap();
    run(&["add", "README"]);
    run(&["commit", "--quiet", "-m", "seed"]);
}

#[test]
fn ac_fidelity_check_validates_test_existence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    make_git_sandbox(dir);

    // A plan whose sole ticked AC names a test that does not exist in the
    // workspace (the sandbox `crates/` directory is empty, and we point the
    // script at this sandbox via cwd).
    let plan_text = "\
---
title: synthetic
---

## Acceptance Criteria

- [x] `nonexistent_test_xyzzy_iter66`: this test was never written.
";
    let plan_path = dir.join("plan.md");
    fs::write(&plan_path, plan_text).unwrap();

    let script = script_path();
    assert!(script.exists(), "script missing: {}", script.display());

    let out = Command::new("bash")
        .arg(&script)
        .arg("--plan")
        .arg(&plan_path)
        .arg("--range")
        .arg("HEAD..HEAD")
        .current_dir(dir)
        .output()
        .expect("run ac-fidelity-check.sh");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "script should have failed for a non-existent test slug.\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("nonexistent_test_xyzzy_iter66")
            || stderr.contains("nonexistent_test_xyzzy_iter66"),
        "expected the missing-slug name in the failure output.\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

#[test]
fn ac_fidelity_check_accepts_existing_workspace_test() {
    // Counter-test: when the named test exists somewhere under crates/, the
    // strengthened check accepts the AC even if it isn't in the branch diff.
    // We run from the real repo root and reference a test we just shipped
    // (`test_token_comparison_constant_time` exists in crates/ff-rdp-cli).
    let root = repo_root();
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.md");
    fs::write(
        &plan_path,
        "\
---
title: synthetic
---

## Acceptance Criteria

- [x] `test_token_comparison_constant_time`: structural CT-equality check.
",
    )
    .unwrap();

    let out = Command::new("bash")
        .arg(script_path())
        .arg("--plan")
        .arg(&plan_path)
        .arg("--range")
        .arg("HEAD..HEAD")
        .current_dir(&root)
        .output()
        .expect("run ac-fidelity-check.sh");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "script should have passed for an existing workspace test.\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}
