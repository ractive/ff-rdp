//! The generated region of the bundled skill matches the CLI's own tables.
//!
//! Formerly `cargo run -p xtask -- gen-skill` and `check-skill-drift`
//! (iter-212); a plain `cargo test` since DEC-059.
//!
//! `crates/ff-rdp-cli/skills/ff-rdp-debug/SKILL.md` is hand-written prose
//! except for the region between the two marker comments, which restates the
//! CLI (description, quick start, command groups, idioms). That region is
//! produced by `commands::skill_doc` and printed by the hidden `ff-rdp
//! skill-doc` subcommand; this test runs the binary built from the tree under
//! test and compares.
//!
//! Regenerate with `FF_RDP_BLESS=1 cargo test -p ff-rdp-cli --test skill_drift`.

use std::path::{Path, PathBuf};
use std::process::Command;

const BLOCK_BEGIN: &str = "<!-- ff-rdp:generated:begin -->";
const BLOCK_END: &str = "<!-- ff-rdp:generated:end -->";
const BLESS: &str = "FF_RDP_BLESS=1 cargo test -p ff-rdp-cli --test skill_drift";

fn skill_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills")
        .join("ff-rdp-debug")
        .join("SKILL.md")
}

fn generated_block() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .arg("skill-doc")
        .output()
        .expect("spawn ff-rdp skill-doc");
    assert!(
        out.status.success(),
        "ff-rdp skill-doc failed: status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("skill-doc emits UTF-8")
}

/// Byte range of the generated region in `existing`, markers included.
fn region(existing: &str) -> Option<std::ops::Range<usize>> {
    let start = existing.find(BLOCK_BEGIN)?;
    let end = existing.find(BLOCK_END)?;
    (start < end).then_some(start..end + BLOCK_END.len())
}

/// `existing` with its generated region replaced by `generated`. Trims the
/// generator's trailing newline so splicing is idempotent.
fn splice(existing: &str, range: std::ops::Range<usize>, generated: &str) -> String {
    format!(
        "{}{}{}",
        &existing[..range.start],
        generated.trim_end_matches('\n'),
        &existing[range.end..]
    )
}

/// First line where `committed` and `generated` disagree, as a report.
fn first_difference(committed: &str, generated: &str) -> String {
    let (c, g): (Vec<&str>, Vec<&str>) = (committed.lines().collect(), generated.lines().collect());
    let i = (0..c.len().max(g.len()))
        .find(|&i| c.get(i) != g.get(i))
        .unwrap_or(0);
    format!(
        "first difference at line {} of the generated region:\n  committed: {}\n  generated: {}",
        i + 1,
        c.get(i).unwrap_or(&"<end of region>"),
        g.get(i).unwrap_or(&"<end of region>")
    )
}

#[test]
fn skill_md_generated_region_is_current() {
    let path = skill_path();
    let existing = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let range = region(&existing).unwrap_or_else(|| {
        panic!(
            "{} has no generated region: expected one delimited by\n  {BLOCK_BEGIN}\n  {BLOCK_END}",
            path.display()
        )
    });
    let generated = generated_block();
    assert!(
        generated.starts_with(BLOCK_BEGIN) && generated.trim_end().ends_with(BLOCK_END),
        "ff-rdp skill-doc output is not a marked region:\n{generated}"
    );
    let committed = &existing[range.clone()];
    if committed == generated.trim_end_matches('\n') {
        return;
    }
    if std::env::var_os("FF_RDP_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, splice(&existing, range, &generated))
            .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
        eprintln!(
            "skill_drift: rewrote the generated region of {}",
            path.display()
        );
        return;
    }
    panic!(
        "{} is stale: its generated region no longer matches \
         crates/ff-rdp-cli/src/commands/skill_doc.rs.\n{}\nfix: {BLESS}",
        path.display(),
        first_difference(committed, generated.trim_end_matches('\n'))
    );
}

#[test]
fn splice_replaces_only_the_region_and_is_idempotent() {
    let existing = format!("# Skill\n\n{BLOCK_BEGIN}\nold\n{BLOCK_END}\n\ntail\n");
    let generated = format!("{BLOCK_BEGIN}\nnew\n{BLOCK_END}\n");
    let once = splice(&existing, region(&existing).expect("region"), &generated);
    assert_eq!(
        once,
        format!("# Skill\n\n{BLOCK_BEGIN}\nnew\n{BLOCK_END}\n\ntail\n")
    );
    let twice = splice(&once, region(&once).expect("region"), &generated);
    assert_eq!(once, twice);
    assert!(region("no markers\n").is_none());
    assert!(region(&format!("{BLOCK_END}\n{BLOCK_BEGIN}\n")).is_none());
}

#[test]
fn first_difference_names_the_edited_and_the_dropped_line() {
    let report = first_difference("a\nb\nend", "a\nc\nend");
    assert!(
        report.contains("line 2") && report.contains("committed: b"),
        "{report}"
    );
    let report = first_difference("a\nend", "a\nb\nend");
    assert!(
        report.contains("line 2") && report.contains("generated: b"),
        "{report}"
    );
}
