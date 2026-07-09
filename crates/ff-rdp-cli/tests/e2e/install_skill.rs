//! E2E tests for `ff-rdp install-skill`.
//!
//! These tests exercise the CLI binary directly via `std::process::Command`.
//! They do NOT rely on embedded skill content — all tests that install files
//! use `--from-dir` to read skill source from a temporary directory.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Create a minimal skill source directory with one markdown file.
fn make_skill_src(tmp: &TempDir, skill_name: &str) -> PathBuf {
    let dir = tmp.path().join("src").join(skill_name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), "# test skill\nsome content\n").unwrap();
    dir
}

/// Run `ff-rdp install-skill` with the given extra args and an overridden HOME.
///
/// Both `HOME` (Unix) and `USERPROFILE` (Windows) are pointed at the isolated
/// temp dir so the install location is fully redirected on every platform. See
/// iter-108: `dirs::home_dir()` reads the Windows known-folder API and ignores
/// `HOME`, so without the `USERPROFILE` override these tests leaked installs
/// into the real profile (`C:\Users\runneradmin\...`) and shared state across
/// tests running in the same binary.
fn run_install_skill(extra_args: &[&str], home_dir: &std::path::Path) -> std::process::Output {
    std::process::Command::new(ff_rdp_bin())
        .args(["install-skill", "--claude"])
        .args(extra_args)
        .env("HOME", home_dir)
        .env("USERPROFILE", home_dir)
        // Unset RUST_LOG so verbose mode doesn't pollute stderr.
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn ff-rdp")
}

// ---------------------------------------------------------------------------
// --dry-run: should list files without touching disk
// ---------------------------------------------------------------------------

#[test]
fn dry_run_user_lists_files_no_writes() {
    let tmp = TempDir::new().unwrap();
    let home_tmp = TempDir::new().unwrap();
    let skill_src = make_skill_src(&tmp, "my-skill");

    let out = run_install_skill(
        &[
            "--dry-run",
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );

    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout must be valid JSON");
    assert!(json["results"].is_array(), "results must be an array");
    let results = json["results"].as_array().unwrap();
    // At least one file should show action=would-write
    assert!(
        results
            .iter()
            .any(|r| r["action"].as_str() == Some("would-write")),
        "dry-run should show would-write entries; got: {json}"
    );

    // No files should have been written to the fake HOME
    let skills_dir = home_tmp.path().join(".claude").join("skills");
    assert!(
        !skills_dir.exists(),
        "dry-run must not create any directories on disk"
    );
}

// ---------------------------------------------------------------------------
// Install to a temp HOME, then re-install is a no-op
// ---------------------------------------------------------------------------

#[test]
fn install_writes_files_and_reinst_is_noop() {
    let tmp = TempDir::new().unwrap();
    let home_tmp = TempDir::new().unwrap();
    let skill_src = make_skill_src(&tmp, "my-skill");

    // First install.
    let out = run_install_skill(
        &[
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );
    assert!(
        out.status.success(),
        "first install failed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout must be valid JSON");
    let results = json["results"].as_array().unwrap();
    assert!(
        results
            .iter()
            .any(|r| r["action"].as_str() == Some("written")),
        "first install should write files; got: {json}"
    );

    // Installed file should exist.
    let installed = home_tmp
        .path()
        .join(".claude")
        .join("skills")
        .join("ff-rdp-debug")
        .join("SKILL.md");
    assert!(
        installed.exists(),
        "SKILL.md should be on disk after install"
    );

    // File should contain the managed-by header.
    let content = fs::read_to_string(&installed).unwrap();
    assert!(
        content.contains("managed-by: ff-rdp"),
        "installed file should contain managed-by header"
    );

    // Second install → should be no-op (skipped).
    let out2 = run_install_skill(
        &[
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );
    assert!(
        out2.status.success(),
        "second install failed, stderr: {}",
        String::from_utf8_lossy(&out2.stderr)
    );

    let json2: serde_json::Value =
        serde_json::from_slice(&out2.stdout).expect("stdout must be valid JSON");
    let results2 = json2["results"].as_array().unwrap();
    assert!(
        results2
            .iter()
            .all(|r| r["action"].as_str() == Some("skipped")),
        "re-install should skip all files; got: {json2}"
    );
}

// ---------------------------------------------------------------------------
// --force overwrites a user-modified (unmanaged) file
// ---------------------------------------------------------------------------

#[test]
fn force_overwrites_unmanaged_file() {
    let tmp = TempDir::new().unwrap();
    let home_tmp = TempDir::new().unwrap();
    let skill_src = make_skill_src(&tmp, "my-skill");

    // Plant an unmanaged file at the expected install location.
    let skill_dir = home_tmp
        .path()
        .join(".claude")
        .join("skills")
        .join("ff-rdp-debug");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "user-edited content, no header\n",
    )
    .unwrap();

    // Without --force: should error.
    let out_no_force = run_install_skill(
        &[
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );
    assert!(
        !out_no_force.status.success(),
        "expected failure when overwriting unmanaged file without --force"
    );
    // The error is emitted as the JSON error envelope on stdout (iter-98 Theme
    // D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&out_no_force.stderr);
    let stdout = String::from_utf8_lossy(&out_no_force.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("not managed by ff-rdp") || combined.contains("--force"),
        "error should mention managed-by or --force; stderr={stderr:?} stdout={stdout:?}"
    );

    // With --force: should succeed and overwrite.
    let out_force = run_install_skill(
        &[
            "--user",
            "--force",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );
    assert!(
        out_force.status.success(),
        "forced install should succeed, stderr: {}",
        String::from_utf8_lossy(&out_force.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&out_force.stdout).expect("stdout must be valid JSON");
    let results = json["results"].as_array().unwrap();
    assert!(
        results
            .iter()
            .any(|r| r["action"].as_str() == Some("written")),
        "--force should write the file; got: {json}"
    );
}

// ---------------------------------------------------------------------------
// --list shows skill with installed=true after install, installed=false before
// ---------------------------------------------------------------------------

#[test]
fn list_shows_installed_status() {
    let home_tmp = TempDir::new().unwrap();

    // List before install: should show installed=false.
    let out_before = run_install_skill(&["--list"], home_tmp.path());
    assert!(
        out_before.status.success(),
        "list before install failed, stderr: {}",
        String::from_utf8_lossy(&out_before.stderr)
    );
    let json_before: serde_json::Value =
        serde_json::from_slice(&out_before.stdout).expect("stdout must be valid JSON");
    let before_items = json_before["results"].as_array().unwrap();
    assert!(
        !before_items.is_empty(),
        "list should show registered skills"
    );
    let debug_entry_before = before_items
        .iter()
        .find(|r| r["name"].as_str() == Some("ff-rdp-debug"))
        .expect("ff-rdp-debug must appear in list");
    assert_eq!(
        debug_entry_before["installed"], false,
        "should not be installed before install"
    );

    // Install.
    let tmp = TempDir::new().unwrap();
    let skill_src = make_skill_src(&tmp, "my-skill");
    let out_install = run_install_skill(
        &[
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );
    assert!(out_install.status.success(), "install failed");

    // List after install: should show installed=true.
    let out_after = run_install_skill(&["--list"], home_tmp.path());
    assert!(out_after.status.success(), "list after install failed");
    let json_after: serde_json::Value =
        serde_json::from_slice(&out_after.stdout).expect("stdout must be valid JSON");
    let after_items = json_after["results"].as_array().unwrap();
    let debug_entry_after = after_items
        .iter()
        .find(|r| r["name"].as_str() == Some("ff-rdp-debug"))
        .expect("ff-rdp-debug must appear in list after install");
    assert_eq!(
        debug_entry_after["installed"], true,
        "should be installed after install"
    );
    assert!(
        !debug_entry_after["installed_path"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "installed_path must be non-empty after install"
    );

    // Uninstall.
    let out_uninstall = run_install_skill(&["--uninstall", "ff-rdp-debug"], home_tmp.path());
    assert!(out_uninstall.status.success(), "uninstall failed");

    // List after uninstall: should show installed=false.
    let out_final = run_install_skill(&["--list"], home_tmp.path());
    assert!(out_final.status.success(), "list after uninstall failed");
    let json_final: serde_json::Value =
        serde_json::from_slice(&out_final.stdout).expect("stdout must be valid JSON");
    let final_items = json_final["results"].as_array().unwrap();
    let debug_entry_final = final_items
        .iter()
        .find(|r| r["name"].as_str() == Some("ff-rdp-debug"))
        .expect("ff-rdp-debug must appear in list after uninstall");
    assert_eq!(
        debug_entry_final["installed"], false,
        "should not be installed after uninstall"
    );
}

// ---------------------------------------------------------------------------
// --project outside git repo → error mentioning git repo or --force
// ---------------------------------------------------------------------------

#[test]
fn project_outside_git_repo_errors() {
    let home_tmp = TempDir::new().unwrap();
    // Create a temp dir with no .git
    let no_git_dir = TempDir::new().unwrap();

    let out = std::process::Command::new(ff_rdp_bin())
        .args(["install-skill", "--claude", "--project", "--list"])
        .env("HOME", home_tmp.path())
        .env("USERPROFILE", home_tmp.path())
        .env_remove("RUST_LOG")
        .current_dir(no_git_dir.path())
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !out.status.success(),
        "should fail outside git repo; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    // The error is emitted as the JSON error envelope on stdout (iter-98 Theme
    // D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("git") || combined.contains("--force"),
        "error should mention git or --force; stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// --project inside a git repo installs to <git-root>/.claude/skills/
// ---------------------------------------------------------------------------

#[test]
fn project_inside_git_repo_installs_correctly() {
    let home_tmp = TempDir::new().unwrap();
    let git_dir = TempDir::new().unwrap();
    // Create a .git directory to make it look like a git repo.
    fs::create_dir_all(git_dir.path().join(".git")).unwrap();

    let skill_src_tmp = TempDir::new().unwrap();
    let skill_src = make_skill_src(&skill_src_tmp, "my-skill");

    let out = std::process::Command::new(ff_rdp_bin())
        .args([
            "install-skill",
            "--claude",
            "--project",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ])
        .env("HOME", home_tmp.path())
        .env("USERPROFILE", home_tmp.path())
        .env_remove("RUST_LOG")
        .current_dir(git_dir.path())
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        out.status.success(),
        "project install should succeed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = git_dir
        .path()
        .join(".claude")
        .join("skills")
        .join("ff-rdp-debug")
        .join("SKILL.md");
    assert!(
        expected.exists(),
        "SKILL.md should be installed in <git-root>/.claude/skills/"
    );

    // Verify meta.scope is "project"
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout must be valid JSON");
    assert_eq!(
        json["meta"]["scope"].as_str(),
        Some("project"),
        "meta.scope should be 'project'"
    );
}

// ---------------------------------------------------------------------------
// --from-dir reads files from disk and installs them
// ---------------------------------------------------------------------------

#[test]
fn from_dir_installs_custom_content() {
    let tmp = TempDir::new().unwrap();
    let home_tmp = TempDir::new().unwrap();

    // Create a skill source with specific content.
    let skill_src = tmp.path().join("custom-skill");
    fs::create_dir_all(&skill_src).unwrap();
    fs::write(skill_src.join("guide.md"), "# custom guide\nhello world\n").unwrap();

    let out = run_install_skill(
        &[
            "--user",
            "--from-dir",
            skill_src.to_str().unwrap(),
            "ff-rdp-debug",
        ],
        home_tmp.path(),
    );

    assert!(
        out.status.success(),
        "install with --from-dir failed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let installed = home_tmp
        .path()
        .join(".claude")
        .join("skills")
        .join("ff-rdp-debug")
        .join("guide.md");
    assert!(installed.exists(), "guide.md should be installed");

    let content = fs::read_to_string(&installed).unwrap();
    assert!(
        content.contains("hello world"),
        "installed file should contain source content"
    );
    assert!(
        content.contains("managed-by: ff-rdp"),
        "installed file should have managed-by header"
    );
}

// ---------------------------------------------------------------------------
// Missing --claude flag → user-friendly error
// ---------------------------------------------------------------------------

#[test]
fn missing_claude_flag_errors() {
    let home_tmp = TempDir::new().unwrap();

    let out = std::process::Command::new(ff_rdp_bin())
        .args(["install-skill", "--list"])
        .env("HOME", home_tmp.path())
        .env("USERPROFILE", home_tmp.path())
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(!out.status.success(), "should fail without --claude flag");
}
