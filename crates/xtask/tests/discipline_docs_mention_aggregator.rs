//! Doc test: verify that `CLAUDE.md`, `CONTRIBUTING.md`, and the create-pr
//! skill each mention `check-iteration-ready`.
//!
//! The skill file check silently skips on CI runners that don't have the
//! skill installed (using `if let Ok(content) = fs::read_to_string(...)`).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

const NEEDLE: &str = "check-iteration-ready";

#[test]
fn discipline_docs_mention_aggregator() {
    let root = repo_root();

    // --- CLAUDE.md ---
    let claude_md = root.join("CLAUDE.md");
    let content = fs::read_to_string(&claude_md)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", claude_md.display()));
    assert!(
        content.contains(NEEDLE),
        "CLAUDE.md does not mention '{}'\nPath: {}",
        NEEDLE,
        claude_md.display()
    );

    // --- CONTRIBUTING.md ---
    let contributing = root.join("CONTRIBUTING.md");
    let content = fs::read_to_string(&contributing)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", contributing.display()));
    assert!(
        content.contains(NEEDLE),
        "CONTRIBUTING.md does not mention '{}'\nPath: {}",
        NEEDLE,
        contributing.display()
    );

    // --- ~/.claude/skills/create-pr/SKILL.md (skip if absent — CI runners) ---
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);

    if let Some(home) = home {
        let skill_md = home.join(".claude/skills/create-pr/SKILL.md");
        if let Ok(content) = fs::read_to_string(&skill_md) {
            assert!(
                content.contains(NEEDLE),
                "~/.claude/skills/create-pr/SKILL.md does not mention '{}'\nPath: {}",
                NEEDLE,
                skill_md.display()
            );
        } else {
            // Not installed — skip silently (expected on CI).
            eprintln!(
                "discipline_docs_mention_aggregator: {} not found — skipping skill check",
                skill_md.display()
            );
        }
    }
}
