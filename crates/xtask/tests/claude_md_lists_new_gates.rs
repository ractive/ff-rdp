//! Verifies that CLAUDE.md documents the two new xtask gates introduced in iter-73:
//! `check-firefox-refs` and `check-actor-kb-sync`.
//!
//! This is a contract test: if someone adds a gate and forgets to document it,
//! this test catches it.

use std::fs;

#[test]
fn claude_md_lists_check_firefox_refs() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let claude_md = fs::read_to_string(format!("{manifest_dir}/../../CLAUDE.md"))
        .expect("CLAUDE.md not found at repo root");

    assert!(
        claude_md.contains("check-firefox-refs"),
        "CLAUDE.md must document the `check-firefox-refs` xtask gate (iter-73)"
    );
}

#[test]
fn claude_md_lists_check_actor_kb_sync() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let claude_md = fs::read_to_string(format!("{manifest_dir}/../../CLAUDE.md"))
        .expect("CLAUDE.md not found at repo root");

    assert!(
        claude_md.contains("check-actor-kb-sync"),
        "CLAUDE.md must document the `check-actor-kb-sync` xtask gate (iter-73)"
    );
}
