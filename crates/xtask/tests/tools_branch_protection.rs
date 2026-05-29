//! Integration tests for `tools/branch-protection.sh`.
//!
//! These tests use a fake `gh` shim (placed on PATH via a temporary directory)
//! to simulate the GitHub API response without network access.
//!
//! Tests:
//!   - `tools_branch_protection_asserts_required_live_tests`: script exits 0
//!     when `live-tests` is in required_status_checks.contexts; exits 1 otherwise.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success());
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

/// Write a fake `gh` shim that outputs a given JSON fixture and exits 0.
/// The shim ignores all arguments — it just cats the fixture file.
fn write_gh_shim(dir: &Path, fixture_json: &str) -> PathBuf {
    let shim_path = dir.join("gh");
    let fixture_path = dir.join("fixture.json");
    fs::write(&fixture_path, fixture_json).unwrap();

    let shim_content = format!(
        "#!/usr/bin/env bash\n\
         # Fake gh that handles both 'api' and 'repo view' sub-commands.\n\
         if [[ \"$*\" == *\"nameWithOwner\"* ]]; then\n\
             echo '{{\"nameWithOwner\":\"ractive/ff-rdp\"}}'\n\
             exit 0\n\
         fi\n\
         cat '{fixture}'\n\
         exit 0\n",
        fixture = fixture_path.display(),
    );
    fs::write(&shim_path, &shim_content).unwrap();
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755)).unwrap();
    shim_path
}

/// Run branch-protection.sh with a fake `gh` that returns the given fixture JSON.
fn run_branch_protection(fixture_json: &str) -> std::process::Output {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_gh_shim(tmp.path(), fixture_json);

    let script = repo_root().join("tools/branch-protection.sh");
    assert!(
        script.exists(),
        "branch-protection.sh must exist at {script:?}"
    );

    // Prepend tmp dir to PATH so our fake `gh` is found first.
    let path_var = format!(
        "{}:{}",
        tmp.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    Command::new("bash")
        .arg(&script)
        .arg("ractive/ff-rdp")
        .env("PATH", &path_var)
        .env("GH_BIN", tmp.path().join("gh"))
        .output()
        .expect("run branch-protection.sh")
}

/// Load a fixture JSON file from the repo.
fn load_fixture(name: &str) -> String {
    let path = repo_root().join("tools/tests/branch-protection").join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

/// `tools_branch_protection_asserts_required_live_tests`:
/// - When `live-tests` is in contexts: exit 0.
/// - When `live-tests` is absent: exit 1 with remediation message.
#[test]
fn tools_branch_protection_asserts_required_live_tests() {
    // --- Pass case: fixture contains live-tests ---
    let has_live = load_fixture("has-live-tests.json");
    let out = run_branch_protection(&has_live);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "expected exit 0 when live-tests is present.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK"),
        "expected 'OK' in output.\nstdout: {stdout}"
    );

    // --- Fail case: fixture missing live-tests ---
    let missing_live = load_fixture("missing-live-tests.json");
    let out = run_branch_protection(&missing_live);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected exit 1 when live-tests is absent.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("live-tests"),
        "expected 'live-tests' mentioned in failure output.\ncombined: {combined}"
    );
    assert!(
        combined.contains("Remediation"),
        "expected 'Remediation' section in failure output.\ncombined: {combined}"
    );
}
