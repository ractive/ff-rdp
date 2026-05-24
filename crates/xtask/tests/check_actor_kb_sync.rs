//! Tests for `check-actor-kb-sync` xtask.
//!
//! Uses a temporary git repository so git diff invocations don't traverse
//! out into the real repo.

use std::fs;
use std::path::Path;
use std::process::Command;

fn repo_root() -> std::path::PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git rev-parse failed");
    std::path::PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

/// Build the actor path inside a sandbox directory, matching the project layout.
fn actor_path(sandbox: &Path, stem: &str) -> std::path::PathBuf {
    sandbox
        .join("crates/ff-rdp-core/src/actors")
        .join(format!("{stem}.rs"))
}

/// Build the kb path inside a sandbox directory.
fn kb_path(sandbox: &Path, slug: &str) -> std::path::PathBuf {
    sandbox.join("kb/rdp/actors").join(format!("{slug}.md"))
}

/// Initialise a minimal git repo in `dir` with a seed commit.
fn make_git_sandbox(dir: &Path) {
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
    // Seed commit so HEAD exists.
    fs::write(dir.join("README"), "seed\n").unwrap();
    run(&["add", "README"]);
    run(&["commit", "--quiet", "-m", "seed"]);
}

/// Stage + commit all tracked changes.
fn git_commit(dir: &Path, msg: &str) {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|_| panic!("git {args:?}"));
    };
    run(&["add", "-A"]);
    let s = Command::new("git")
        .args(["commit", "--quiet", "-m", msg])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(s.status.success(), "git commit failed: {s:?}");
}

/// Run check-actor-kb-sync from a sandbox, wiring git env vars so the diff
/// sees only the sandbox's commits.
fn run_check_in_sandbox(sandbox: &Path, since: &str) -> std::process::Output {
    let root = repo_root();
    Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "-p",
            "xtask",
            "--",
            "check-actor-kb-sync",
            "--since",
            since,
        ])
        .current_dir(&root)
        .env("GIT_DIR", sandbox.join(".git"))
        .env("GIT_WORK_TREE", sandbox)
        .output()
        .expect("cargo run xtask")
}

#[test]
fn actor_changed_without_kb_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    make_git_sandbox(dir);

    // Create actor dir + file and commit as "base".
    let actor_dir = dir.join("crates/ff-rdp-core/src/actors");
    fs::create_dir_all(&actor_dir).unwrap();
    let kb_dir = dir.join("kb/rdp/actors");
    fs::create_dir_all(&kb_dir).unwrap();
    fs::write(actor_path(dir, "watcher"), "// initial\n").unwrap();
    fs::write(kb_path(dir, "watcher"), "# WatcherActor\n").unwrap();
    git_commit(dir, "base");

    // Record base ref.
    let base = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };

    // Now modify actor but NOT kb.
    fs::write(actor_path(dir, "watcher"), "// changed\n").unwrap();
    git_commit(dir, "change actor only");

    let out = run_check_in_sandbox(dir, &base);
    assert!(
        !out.status.success(),
        "expected failure when actor changed but kb not updated; \
         stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("watcher"),
        "error should mention the actor name; got: {stderr}"
    );
}

#[test]
fn actor_and_kb_both_changed_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    make_git_sandbox(dir);

    let actor_dir = dir.join("crates/ff-rdp-core/src/actors");
    fs::create_dir_all(&actor_dir).unwrap();
    let kb_dir = dir.join("kb/rdp/actors");
    fs::create_dir_all(&kb_dir).unwrap();
    fs::write(actor_path(dir, "watcher"), "// initial\n").unwrap();
    fs::write(kb_path(dir, "watcher"), "# WatcherActor\n").unwrap();
    git_commit(dir, "base");

    let base = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };

    // Modify both.
    fs::write(actor_path(dir, "watcher"), "// changed\n").unwrap();
    fs::write(kb_path(dir, "watcher"), "# WatcherActor v2\n").unwrap();
    git_commit(dir, "change actor and kb");

    let out = run_check_in_sandbox(dir, &base);
    assert!(
        out.status.success(),
        "expected success when both actor and kb updated; \
         stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn allow_skip_annotation_bypasses_check() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    make_git_sandbox(dir);

    let actor_dir = dir.join("crates/ff-rdp-core/src/actors");
    fs::create_dir_all(&actor_dir).unwrap();
    let kb_dir = dir.join("kb/rdp/actors");
    fs::create_dir_all(&kb_dir).unwrap();
    fs::write(actor_path(dir, "watcher"), "// initial\n").unwrap();
    fs::write(kb_path(dir, "watcher"), "# WatcherActor\n").unwrap();
    git_commit(dir, "base");

    let base = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };

    // Modify actor WITH skip annotation.
    fs::write(
        actor_path(dir, "watcher"),
        "// allow-actor-kb-skip: test only change\n// changed\n",
    )
    .unwrap();
    git_commit(dir, "change actor with skip");

    let out = run_check_in_sandbox(dir, &base);
    assert!(
        out.status.success(),
        "expected success with allow-actor-kb-skip annotation; \
         stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn empty_diff_always_passes() {
    // An empty diff (no changed files) should never trigger the check.
    // The internal unit tests already cover unmapped actor stems directly;
    // this integration test verifies the binary succeeds on a clean diff.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    make_git_sandbox(dir);
    // diff HEAD..HEAD is empty → should always pass.
    let head = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let out = run_check_in_sandbox(dir, &head);
    assert!(
        out.status.success(),
        "expected success on empty diff; stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}
