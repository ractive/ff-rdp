//! `unit_162a_source_invariants_covers_three`.
//!
//! iter-162a merged `check-daemon-locks`, `check-error-envelope-paths` and
//! `check-stderr-annotations` into one subcommand and one CI step. This test is
//! the proof that the merge kept all three: it drives the real binary against
//! synthetic trees containing one defect shape each, and asserts the failure is
//! attributed to the right invariant by name — a merged gate that reported
//! "something failed" would be a regression on the three it replaced.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Create `<root>/daemon/` and `<root>/commands/` with the given file contents
/// (empty string → the directory exists but holds no `.rs` file).
fn synthetic_tree(daemon_src: &str, commands_src: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (sub, src) in [("daemon", daemon_src), ("commands", commands_src)] {
        let path = dir.path().join(sub);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("fixture.rs"), src).unwrap();
    }
    dir
}

/// Run `check-source-invariants` against a synthetic tree, returning
/// `(success, combined_output)`.
fn run_against(root: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "check-source-invariants",
            "--daemon-dir",
            root.join("daemon").to_str().unwrap(),
            "--commands-dir",
            root.join("commands").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

const CLEAN_DAEMON: &str = "fn f() { let _ = lock_or_recover!(state.x); }\n";
const CLEAN_COMMANDS: &str = "fn f() { let _ = 1; }\n";

#[test]
fn daemon_lock_unwrap_fails_named_invariant() {
    let tree = synthetic_tree(
        "fn f() { let _ = state.mu.lock().unwrap(); }\n",
        CLEAN_COMMANDS,
    );
    let (ok, out) = run_against(tree.path());

    assert!(!ok, "expected non-zero exit:\n{out}");
    assert!(
        out.contains("daemon-locks FAIL"),
        "failure must name the daemon-locks invariant:\n{out}"
    );
    assert!(
        out.contains("error-envelope-paths OK") && out.contains("stderr-annotations OK"),
        "the other two invariants must still report their own result:\n{out}"
    );
}

#[test]
fn eprintln_then_exit_bypass_fails_named_invariant() {
    let tree = synthetic_tree(
        CLEAN_DAEMON,
        "fn f() -> Result<(), AppError> {\n    \
         eprintln!(\"error: {}\", msg);\n    \
         return Err(AppError::Exit(1));\n}\n",
    );
    let (ok, out) = run_against(tree.path());

    assert!(!ok, "expected non-zero exit:\n{out}");
    assert!(
        out.contains("error-envelope-paths FAIL"),
        "failure must name the error-envelope-paths invariant:\n{out}"
    );
    assert!(
        out.contains("daemon-locks OK"),
        "daemon-locks must still report its own result:\n{out}"
    );
}

#[test]
fn unannotated_eprintln_fails_named_invariant() {
    // No AppError::Exit nearby, so only the annotation invariant should fire.
    let tree = synthetic_tree(
        CLEAN_DAEMON,
        "fn f() {\n    eprintln!(\"warning: something went wrong\");\n}\n",
    );
    let (ok, out) = run_against(tree.path());

    assert!(!ok, "expected non-zero exit:\n{out}");
    assert!(
        out.contains("stderr-annotations FAIL"),
        "failure must name the stderr-annotations invariant:\n{out}"
    );
    assert!(
        out.contains("error-envelope-paths OK"),
        "error-envelope-paths must not fire on a warn-and-continue eprintln!:\n{out}"
    );
}

#[test]
fn clean_tree_passes_all_three() {
    let tree = synthetic_tree(CLEAN_DAEMON, CLEAN_COMMANDS);
    let (ok, out) = run_against(tree.path());

    assert!(ok, "expected exit 0 on a clean tree:\n{out}");
    for invariant in ["daemon-locks", "error-envelope-paths", "stderr-annotations"] {
        assert!(
            out.contains(&format!("{invariant} OK")),
            "missing OK line for {invariant}:\n{out}"
        );
    }
}

#[test]
fn real_tree_passes() {
    // The defaults point at crates/ff-rdp-cli/src/{daemon,commands}; the
    // subcommand must exit 0 against the checked-in source.
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-source-invariants")
        .output()
        .unwrap();
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "check-source-invariants must pass against the real tree:\n{combined}"
    );
}
