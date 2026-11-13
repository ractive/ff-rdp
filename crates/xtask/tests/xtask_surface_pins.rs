//! Pins on the xtask surface.
//!
//! - The subcommand list is pinned to `live-sweep`, so adding a subcommand has
//!   to be a deliberate edit here (DEC-059: checks are tests).
//! - `ac-fidelity-check.sh` and `claims-vs-code.sh`, deleted in iter-162b, must
//!   not be invoked from anywhere that could still run them.

use std::path::{Path, PathBuf};

const DELETED: &[&str] = &["ac-fidelity-check.sh", "claims-vs-code.sh"];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/xtask.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/xtask")
        .to_path_buf()
}

/// Every in-repo tree that could invoke the deleted scripts.
const INVOCATION_SCAN_ROOTS: &[&str] =
    &["tools", "crates", ".github", "CLAUDE.md", "CONTRIBUTING.md"];

/// Global skill directories, relative to `$HOME`. `create-pr` is where the
/// surviving invocation was found when iter-162b first scanned only the repo.
const SKILL_SCAN_ROOTS: &[&str] = &[".claude/skills/create-pr"];

/// A mention inside prose or a comment is fine — this repo documents what it
/// deleted. An *invocation* is not. Match the shapes that actually run a
/// script: `bash <name>`, `sh <name>`, `./<name>`, `<dir>/<name>`.
fn invocations_in(body: &str, name: &str) -> bool {
    body.lines().any(|line| {
        let Some(pos) = line.find(name) else {
            return false;
        };
        let before = &line[..pos];
        before.ends_with('/')
            || before.trim_end().ends_with("bash")
            || before.trim_end().ends_with("sh")
    })
}

fn scan_file(path: &Path, scanned: &mut usize, offenders: &mut Vec<String>) {
    let Ok(body) = std::fs::read_to_string(path) else {
        return;
    };
    *scanned += 1;
    for name in DELETED {
        if invocations_in(&body, name) {
            offenders.push(format!("{} invokes {name}", path.display()));
        }
    }
}

fn scan_tree(root: &Path, scanned: &mut usize, offenders: &mut Vec<String>) {
    if root.is_file() {
        scan_file(root, scanned, offenders);
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == ".git")
            {
                continue;
            }
            scan_tree(&path, scanned, offenders);
        } else {
            scan_file(&path, scanned, offenders);
        }
    }
}

#[test]
fn deleted_discipline_scripts_are_not_invoked_anywhere() {
    let root = workspace_root();
    let mut scanned = 0usize;
    let mut offenders = Vec::new();

    for rel in INVOCATION_SCAN_ROOTS {
        scan_tree(&root.join(rel), &mut scanned, &mut offenders);
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in SKILL_SCAN_ROOTS {
            let dir = home.join(rel);
            if dir.exists() {
                scan_tree(&dir, &mut scanned, &mut offenders);
            }
        }
    }

    assert!(
        scanned > 50,
        "scanned only {scanned} files — the scan roots are wrong and this test proves nothing"
    );
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// Since DEC-059 every repo check is a `cargo test` test and `xtask` holds only
/// the live sweep. Pinned so a new subcommand has to be a deliberate edit here:
/// a check belongs in a test, not back in xtask.
const EXPECTED_SUBCOMMANDS: &[&str] = &["live-sweep"];

#[test]
fn xtask_help_matches_the_pinned_subcommand_list() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--help")
        .output()
        .expect("running xtask --help");
    assert!(out.status.success(), "xtask --help exited non-zero");
    let help = String::from_utf8_lossy(&out.stdout);

    // Parse the `Commands:` block rather than substring-matching, so an
    // unlisted subcommand — a re-added gate under a new name — fails here
    // instead of slipping through a set of `contains` checks.
    let listed = parse_subcommands(&help);
    let mut expected: Vec<&str> = EXPECTED_SUBCOMMANDS.to_vec();
    expected.sort_unstable();
    let mut actual: Vec<&str> = listed.iter().map(String::as_str).collect();
    actual.sort_unstable();
    assert_eq!(
        actual, expected,
        "xtask subcommands changed; update EXPECTED_SUBCOMMANDS deliberately if that is intended"
    );
}

/// Collect subcommand names from clap's `Commands:` block: indented lines whose
/// first token is the name. Stops at the next unindented section (`Options:`).
fn parse_subcommands(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_block = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            in_block = true;
            continue;
        }
        if in_block {
            if line.trim().is_empty() {
                continue;
            }
            if !line.starts_with(char::is_whitespace) {
                break;
            }
            if let Some(name) = line.split_whitespace().next()
                && name != "help"
            {
                names.push(name.to_owned());
            }
        }
    }
    names
}
