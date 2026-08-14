//! iter-162b: `ac-fidelity-check.sh` and `claims-vs-code.sh` are deleted from
//! every copy — the two skill directories and their two in-repo mirrors.
//!
//! This asserts absence, which is the one shape of assertion that passes
//! vacuously when its path is wrong. iter-158's
//! `unit_158_source_scan_covers_the_live_suites` shipped exactly that bug: it
//! matched a literal `tests/live` that never occurs in a Windows path, scanned
//! zero files, and passed. So every directory here is asserted to exist and to
//! be non-empty *before* the absence check runs.

use std::path::{Path, PathBuf};

const DELETED: &[&str] = &["ac-fidelity-check.sh", "claims-vs-code.sh"];

/// The two in-repo mirrors, relative to the workspace root.
const MIRRORS: &[&str] = &["tools/ralph-loop/scripts", "tools/new-ralph-loop/scripts"];

/// The two canonical skill directories, relative to `$HOME`.
const SKILLS: &[&str] = &[
    ".claude/skills/ralph-loop/scripts",
    ".claude/skills/new-ralph-loop/scripts",
];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/xtask.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/xtask")
        .to_path_buf()
}

/// Assert the directory exists and holds at least one entry, then assert none
/// of the deleted scripts is among them.
fn assert_scripts_absent(dir: &Path) {
    assert!(
        dir.is_dir(),
        "{} is not a directory — the absence check below would pass vacuously",
        dir.display()
    );
    let entries: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        !entries.is_empty(),
        "{} is empty — an empty directory is not proof of deletion",
        dir.display()
    );
    for name in DELETED {
        assert!(
            !entries.iter().any(|e| e == name),
            "{} still contains {name}; iter-162b deletes it from all four copies",
            dir.display()
        );
    }
}

#[test]
fn unit_162b_both_scripts_absent_from_mirrors() {
    let root = workspace_root();
    for rel in MIRRORS {
        assert_scripts_absent(&root.join(rel));
    }
}

#[test]
fn unit_162b_both_scripts_absent_from_skills() {
    // The skill directories live outside the repo, so a machine that has never
    // installed the skills has nothing to check. Skip rather than fail — but
    // only when the parent is genuinely absent, never when it is merely empty.
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("HOME unset — skipping skill-directory check");
        return;
    };
    for rel in SKILLS {
        let dir = home.join(rel);
        if !dir.exists() {
            eprintln!("{} not installed — skipping", dir.display());
            continue;
        }
        assert_scripts_absent(&dir);
    }
}

#[test]
fn unit_162b_no_script_is_invoked_anywhere() {
    let root = workspace_root();
    let mut scanned = 0usize;
    let mut offenders = Vec::new();
    for rel in MIRRORS {
        for entry in std::fs::read_dir(root.join(rel)).expect("mirror dir") {
            let path = entry.expect("dir entry").path();
            if !path.is_file() {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&path) else {
                continue;
            };
            scanned += 1;
            for name in DELETED {
                // An invocation names the script with a path separator in
                // front of it; a prose mention in a comment does not.
                if body.contains(&format!("/{name}")) {
                    offenders.push(format!("{} invokes {name}", path.display()));
                }
            }
        }
    }
    assert!(
        scanned > 0,
        "scanned zero files — the mirror paths are wrong and this test proves nothing"
    );
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}
