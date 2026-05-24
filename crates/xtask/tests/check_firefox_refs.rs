//! Tests for `check-firefox-refs` xtask.
//!
//! All tests use a temp directory as a synthetic Firefox root so no real
//! Firefox checkout is required (except the last gated test).

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

/// Write a minimal plan with zero firefox_refs.
fn plan_no_refs() -> String {
    r#"---
title: "Test Plan"
status: planned
type: iteration
dogfood_path: "ff-rdp --help"
---

# Body
"#
    .to_owned()
}

/// Write a plan with a firefox_ref pointing to a given path/lines/why.
fn plan_with_ref(path: &str, lines: &str, why: &str) -> String {
    format!(
        r#"---
title: "Test Plan"
status: planned
type: iteration
dogfood_path: "ff-rdp --help"
firefox_refs:
  - path: {path}
    lines: "{lines}"
    why: "{why}"
---

# Body
"#
    )
}

/// Run the xtask binary with the given env overrides and plan content.
fn run_xtask(firefox_root: &str, plan_content: &str, tmp: &Path) -> std::process::Output {
    let plan_path = tmp.join("plan.md");
    fs::write(&plan_path, plan_content).unwrap();

    let root = repo_root();
    Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "-p",
            "xtask",
            "--",
            "check-firefox-refs",
            plan_path.to_str().unwrap(),
        ])
        .env("FF_RDP_FIREFOX_PATH", firefox_root)
        .current_dir(&root)
        .output()
        .expect("cargo run xtask")
}

#[test]
fn plan_with_no_firefox_refs_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run_xtask("/nonexistent-firefox-root", &plan_no_refs(), tmp.path());
    assert!(
        out.status.success(),
        "expected success for plan with no firefox_refs; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no firefox_refs"),
        "expected 'no firefox_refs' message; got: {stdout}"
    );
}

#[test]
fn valid_in_range_ref_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let ff_root = tmp.path().join("firefox");
    let devtools_dir = ff_root.join("devtools/shared/specs");
    fs::create_dir_all(&devtools_dir).unwrap();
    // Write a 10-line file.
    fs::write(
        devtools_dir.join("watcher.js"),
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n",
    )
    .unwrap();

    let plan = plan_with_ref("devtools/shared/specs/watcher.js", "1-5", "test ref");
    let out = run_xtask(ff_root.to_str().unwrap(), &plan, tmp.path());
    assert!(
        out.status.success(),
        "expected success for in-range ref; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn out_of_range_ref_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ff_root = tmp.path().join("firefox");
    let devtools_dir = ff_root.join("devtools/shared/specs");
    fs::create_dir_all(&devtools_dir).unwrap();
    // Write a 5-line file, ask for lines 1-100.
    fs::write(
        devtools_dir.join("watcher.js"),
        "line1\nline2\nline3\nline4\nline5\n",
    )
    .unwrap();

    let plan = plan_with_ref("devtools/shared/specs/watcher.js", "1-100", "too far");
    let out = run_xtask(ff_root.to_str().unwrap(), &plan, tmp.path());
    assert!(
        !out.status.success(),
        "expected failure for out-of-range ref"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("out of range") || stderr.contains("check-firefox-refs"),
        "expected error message; got: {stderr}"
    );
}

#[test]
fn missing_file_ref_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ff_root = tmp.path().join("firefox");
    fs::create_dir_all(&ff_root).unwrap(); // root exists but file doesn't

    let plan = plan_with_ref("devtools/shared/specs/does-not-exist.js", "1-5", "missing");
    let out = run_xtask(ff_root.to_str().unwrap(), &plan, tmp.path());
    assert!(!out.status.success(), "expected failure for missing file");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("check-firefox-refs"),
        "expected 'not found' error; got: {stderr}"
    );
}

#[test]
fn malformed_lines_field_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ff_root = tmp.path().join("firefox");
    let devtools_dir = ff_root.join("devtools/shared/specs");
    fs::create_dir_all(&devtools_dir).unwrap();
    fs::write(devtools_dir.join("watcher.js"), "line1\nline2\n").unwrap();

    let plan = plan_with_ref("devtools/shared/specs/watcher.js", "notanumber", "bad");
    let out = run_xtask(ff_root.to_str().unwrap(), &plan, tmp.path());
    assert!(
        !out.status.success(),
        "expected failure for malformed lines"
    );
}

#[test]
fn missing_firefox_root_fails_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    // Point to a path that definitely doesn't exist.
    let plan = plan_with_ref("devtools/shared/specs/watcher.js", "1-5", "test");
    let out = run_xtask("/tmp/this-does-not-exist-ff-rdp-test", &plan, tmp.path());
    assert!(
        !out.status.success(),
        "expected failure when firefox root is missing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Firefox source root") || stderr.contains("FF_RDP_FIREFOX_PATH"),
        "expected clear error about missing firefox root; got: {stderr}"
    );
}

/// Live gate: runs only when the local Firefox checkout is present and
/// `FF_RDP_LIVE_TESTS=1`. Marked `#[ignore]` so it's visible in test output
/// rather than silently passing, matching the project convention for live
/// gates documented in CLAUDE.md.
#[test]
#[ignore]
fn real_firefox_path_iter73_plan_no_refs_passes() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("skipping: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let firefox_devtools = Path::new("/Users/james/devel/firefox/devtools");
    if !firefox_devtools.exists() {
        eprintln!("skipping: /Users/james/devel/firefox/devtools not present");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = run_xtask("/Users/james/devel/firefox", &plan_no_refs(), tmp.path());
    assert!(
        out.status.success(),
        "expected success with real firefox path; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
