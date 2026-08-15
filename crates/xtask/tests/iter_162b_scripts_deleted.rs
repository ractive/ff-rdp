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

/// Every tree that could invoke the deleted scripts. The skill directories are
/// included because that is where the surviving live invocation was found in
/// review: `~/.claude/skills/create-pr/SKILL.md` still ran ac-fidelity-check.sh
/// while the first version of this test — which scanned only the two mirrors —
/// passed. Scanning too narrowly is its own kind of vacuous.
const INVOCATION_SCAN_ROOTS: &[&str] =
    &["tools", "crates", ".github", "CLAUDE.md", "CONTRIBUTING.md"];

const SKILL_SCAN_ROOTS: &[&str] = &[
    ".claude/skills/ralph-loop",
    ".claude/skills/new-ralph-loop",
    ".claude/skills/create-pr",
];

/// A mention inside prose or a comment is fine — this repo documents what it
/// deleted. An *invocation* is not. Match the shapes that actually run a
/// script: `bash foo.sh`, `./foo.sh`, `"$dir/foo.sh"`, `path/to/foo.sh …`.
fn invocations_in(body: &str, name: &str) -> bool {
    body.lines().any(|line| {
        let Some(pos) = line.find(name) else {
            return false;
        };
        let before = &line[..pos];
        // `bash <name>` / `sh <name>` / `./<name>` / `<anything>/<name>`
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
fn unit_162b_no_script_is_invoked_anywhere() {
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

    // Parse the `Commands:` block rather than substring-matching, so a NINTH
    // subcommand — a re-added gate under a new name — fails here instead of
    // slipping through a set of `contains` checks.
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

#[test]
fn ci_162b_discipline_job_two_xtask_steps() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("reading ci.yml");
    let invocations: Vec<&str> = ci
        .lines()
        .map(str::trim)
        // A YAML comment mentioning the command is not an invocation.
        .filter(|l| !l.starts_with('#'))
        .filter(|l| l.contains("cargo run -p xtask --"))
        .collect();
    assert_eq!(
        invocations.len(),
        2,
        "expected exactly 2 xtask steps in CI, found: {invocations:#?}"
    );
    for line in &invocations {
        // Take the subcommand only — trailing arguments are legitimate.
        let name = line
            .rsplit("xtask -- ")
            .next()
            .expect("subcommand after `xtask -- `")
            .split_whitespace()
            .next()
            .expect("non-empty subcommand");
        assert!(
            EXPECTED_SUBCOMMANDS.contains(&name),
            "CI invokes `{name}`, which xtask does not ship"
        );
    }
}
