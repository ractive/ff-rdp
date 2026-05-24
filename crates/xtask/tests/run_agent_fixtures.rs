//! Snapshot/contract test for the rdp-spec-reviewer agent fixture.
//!
//! This is a *format and contract test only* — it does not invoke the
//! `claude` CLI or any AI model. Instead it parses the fixture patch and
//! the expected drift report and asserts that the expected report mentions
//! the specific changes introduced by the patch.
//!
//! Full agent invocation (which would produce the report dynamically) is a
//! manual flow:
//!
//! ```bash
//! claude --agent rdp-spec-reviewer \
//!     --input tools/agents/fixtures/synthetic-watcher-diff.patch
//! ```
//!
//! A `cargo xtask run-agent-fixtures` subcommand to automate this is
//! // allow-todo: deferred to follow-up iteration

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success());
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

#[test]
fn rdp_spec_reviewer_fixture_snapshot() {
    let root = repo_root();

    // Read the synthetic diff patch.
    let patch_path = root.join("tools/agents/fixtures/synthetic-watcher-diff.patch");
    let patch = fs::read_to_string(&patch_path)
        .unwrap_or_else(|e| panic!("Could not read fixture patch at {patch_path:?}: {e}"));

    // Read the expected drift report.
    let report_path = root.join("tools/agents/fixtures/expected/synthetic-watcher-diff.report.md");
    let report = fs::read_to_string(&report_path)
        .unwrap_or_else(|e| panic!("Could not read expected report at {report_path:?}: {e}"));

    // Contract assertion 1: the patch renames `resource_types` → `resources`.
    // The expected report must mention this renamed field.
    assert!(
        patch.contains("resource_types") || patch.contains("resources"),
        "fixture patch should contain the renamed parameter; patch excerpt:\n{}",
        &patch[..patch.len().min(400)]
    );
    assert!(
        report.contains("resource_types") || report.contains("resources"),
        "expected report must mention the renamed parameter 'resource_types' / 'resources'"
    );

    // Contract assertion 2: the patch removes the `oneway: true` marker comment.
    assert!(
        patch.contains("oneway"),
        "fixture patch should contain a removed `oneway` marker comment"
    );
    assert!(
        report.contains("oneway"),
        "expected report must mention the removed `oneway` marker"
    );

    // Contract assertion 3: the report has the required ## Spec drift section.
    assert!(
        report.contains("## Spec drift"),
        "expected report must have a '## Spec drift' section header"
    );

    // Contract assertion 4: the report mentions the watcher actor.
    assert!(
        report.to_lowercase().contains("watcher"),
        "expected report should reference the watcher actor"
    );
}
