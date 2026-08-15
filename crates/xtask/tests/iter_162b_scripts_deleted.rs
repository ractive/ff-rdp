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

/// The subcommand list after iter-162a (16 → 9) and iter-162b (9 → 8).
/// Pinned so a re-added gate has to be a deliberate edit here, not a drive-by.
const EXPECTED_SUBCOMMANDS: &[&str] = &[
    "check-iteration-plan",
    "check-source-invariants",
    "check-firefox-refs",
    "check-actor-kb-sync",
    "check-live-test-layout",
    "check-dogfood-script",
    "find-iteration-plan",
    "live-sweep",
];

#[test]
fn unit_162b_xtask_help_lists_eight() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--help")
        .output()
        .expect("running xtask --help");
    assert!(out.status.success(), "xtask --help exited non-zero");
    let help = String::from_utf8_lossy(&out.stdout);

    for name in EXPECTED_SUBCOMMANDS {
        assert!(help.contains(name), "xtask --help no longer lists {name}");
    }
    for gone in [
        "check-discipline-regression",
        "check-iteration-ready",
        "check-dead-primitives",
        "check-todo-annotations",
        "check-pre-fix-repro",
        "check-oneway-conformance",
    ] {
        assert!(
            !help.contains(gone),
            "xtask --help still lists {gone}, deleted in iter-162a/162b"
        );
    }
}

#[test]
fn ci_162b_discipline_job_two_xtask_steps() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("reading ci.yml");
    let invocations: Vec<&str> = ci
        .lines()
        .filter(|l| l.contains("cargo run -p xtask --"))
        .collect();
    assert_eq!(
        invocations.len(),
        2,
        "expected exactly 2 xtask steps in CI, found: {invocations:#?}"
    );
    for line in &invocations {
        let name = line
            .rsplit("xtask -- ")
            .next()
            .expect("subcommand after `xtask -- `")
            .trim();
        assert!(
            EXPECTED_SUBCOMMANDS.contains(&name),
            "CI invokes `{name}`, which xtask does not ship"
        );
    }
}
