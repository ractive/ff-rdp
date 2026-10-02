//! `unit_162a_source_invariants_covers_three` (now two: the lock-unwrap scan
//! was retired with the code it scanned).
//!
//! iter-162a merged `check-error-envelope-paths` and `check-stderr-annotations`
//! into one subcommand and one CI step. This test drives the real binary
//! against synthetic trees containing one defect shape each, and asserts the
//! failure is attributed to the right invariant by name.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Create `<root>/commands/` with the given file contents.
fn synthetic_tree(commands_src: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("commands");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("fixture.rs"), commands_src).unwrap();
    dir
}

/// Run `check-source-invariants` against a synthetic tree, returning
/// `(success, combined_output)`.
fn run_against(root: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "check-source-invariants",
            "--commands-dir",
            root.join("commands").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

const CLEAN_COMMANDS: &str = "fn f() { let _ = 1; }\n";

#[test]
fn eprintln_then_exit_bypass_fails_named_invariant() {
    let tree = synthetic_tree(
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
}

#[test]
fn unannotated_eprintln_fails_named_invariant() {
    // No AppError::Exit nearby, so only the annotation invariant should fire.
    let tree = synthetic_tree("fn f() {\n    eprintln!(\"warning: something went wrong\");\n}\n");
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
fn clean_tree_passes_both() {
    let tree = synthetic_tree(CLEAN_COMMANDS);
    let (ok, out) = run_against(tree.path());

    assert!(ok, "expected exit 0 on a clean tree:\n{out}");
    for invariant in ["error-envelope-paths", "stderr-annotations"] {
        assert!(
            out.contains(&format!("{invariant} OK")),
            "missing OK line for {invariant}:\n{out}"
        );
    }
}

#[test]
fn real_tree_passes() {
    // The default points at crates/ff-rdp-cli/src/commands; the subcommand
    // must exit 0 against the checked-in source.
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
