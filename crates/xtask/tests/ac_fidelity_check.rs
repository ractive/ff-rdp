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

- [x] `test_nonexistent_xyzzy_iter66`: this test was never written.
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
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("test_nonexistent_xyzzy_iter66"),
        "expected the missing-slug name in the failure output.\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    // Assert the strengthened iter-66 diagnostic specifically, so a future
    // regression that swaps the slug-check for the generic backtick heuristic
    // would be caught.
    assert!(
        combined.contains("no matching `fn` in the workspace"),
        "expected the strengthened iter-66 'no matching `fn`' diagnostic.\n\
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

// --- iter-154: the gate never established that a named test *ran*. ------------

/// Run the gate from the real repo root against a checked-in fixture plan.
fn run_gate(fixture: &str, range: &str) -> (bool, String) {
    let root = repo_root();
    let out = Command::new("bash")
        .arg(script_path())
        .arg("--plan")
        .arg(root.join("tools/tests/ac-fidelity-check").join(fixture))
        .arg("--range")
        .arg(range)
        .current_dir(&root)
        .output()
        .expect("run ac-fidelity-check.sh");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

#[test]
fn shell_154_unrun_ac_fails() {
    // The fixture's AC names a test that really exists under crates/, so every
    // pre-154 heuristic is satisfied; the only defect is that the AC's own
    // continuation text admits the test was never exercised.
    let (passed, output) = run_gate("unrun-live-ac.md", "HEAD..HEAD");
    assert!(
        !passed,
        "gate must reject a ticked AC that declares its own non-execution.\n{output}"
    );
    assert!(
        output.contains("declares its own non-execution"),
        "expected the Theme A diagnostic, not an unrelated failure.\n{output}"
    );
    assert!(
        output.contains("live_110_replace_never_kills_foreign_firefox"),
        "failure output must name the offending AC.\n{output}"
    );
    assert!(
        output.contains("Untick it") && output.contains("[deferred — new plan:"),
        "failure output must suggest untick-or-defer.\n{output}"
    );
}

#[test]
fn shell_154_evidenced_ac_passes() {
    // Same AC plus a `[verified: <date>, <measurement>]` annotation.
    let (passed, output) = run_gate("evidenced-live-ac.md", "HEAD..HEAD");
    assert!(
        passed,
        "a live AC carrying run evidence must pass.\n{output}"
    );

    // A legitimate `[deferred — new plan: …]` necessarily carries the same
    // wording Theme A denies. The deferral must win.
    let (passed, output) = run_gate("deferred-ac.md", "HEAD..HEAD");
    assert!(
        passed,
        "Theme A's denial list must not swallow a legitimate deferral.\n{output}"
    );
}

#[test]
fn shell_154_iter151_prefix_would_have_failed() {
    // Replay the real plan text that motivated iter-154, rather than a case
    // invented to be catchable. Before this iteration the gate exited 0 here.
    let (passed, output) = run_gate("iter151-prefix-ac.md", "6d07c8c^..6d07c8c");
    assert!(
        !passed,
        "iteration-151's pre-fix AC block must not pass the gate.\n{output}"
    );
    assert!(
        output.contains("declares its own non-execution")
            && output.contains("live_151_chunk_a_leaves_no_orphans"),
        "the chunk-A AC must fail on its 'not exercised' text specifically.\n{output}"
    );

    // Guard the fixture against drift or invention: it must be the Acceptance
    // Criteria block of iteration-151 exactly as it stood at 6d07c8c.
    let show = Command::new("git")
        .args([
            "show",
            "6d07c8c:kb/iterations/iteration-151-residual-live-firefox-leak.md",
        ])
        .current_dir(repo_root())
        .output()
        .expect("git show");
    if !show.status.success() {
        eprintln!("6d07c8c unreachable (shallow clone?) — skipping fixture provenance check");
        return;
    }
    let plan = String::from_utf8_lossy(&show.stdout);
    let expected: String = plan
        .lines()
        .skip_while(|l| !l.starts_with("## Acceptance Criteria"))
        .take_while(|l| !l.starts_with("## ") || l.starts_with("## Acceptance Criteria"))
        .collect::<Vec<_>>()
        .join("\n");
    let fixture =
        fs::read_to_string(repo_root().join("tools/tests/ac-fidelity-check/iter151-prefix-ac.md"))
            .expect("read fixture");
    assert!(
        fixture.contains(expected.trim()),
        "fixture no longer matches iteration-151's AC block at 6d07c8c — do not edit it"
    );
}
