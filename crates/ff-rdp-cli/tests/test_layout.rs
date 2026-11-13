//! Layout rules for this crate's live-Firefox tests.
//!
//! Formerly `cargo run -p xtask -- check-live-test-layout` (iter-100b,
//! iter-113); a plain `cargo test` since DEC-059. This file must not be named
//! `live_*.rs` — rule 1 forbids exactly that.
//!
//! 1. No top-level `tests/live_*.rs`. Every live suite is a `mod` of the
//!    single gated `tests/live/main.rs` target, so a plain `cargo test` links
//!    one live binary instead of ~45. New live tests go in
//!    `tests/live/<slug>.rs` plus a `mod` line in `tests/live/main.rs`.
//! 2. Every `#[test]` directly under `tests/live/` carries `#[ignore]` (or an
//!    `// allow-ungated-live: <reason>` marker in its attribute block), so a
//!    plain `cargo test` never launches Firefox. `xtask live-sweep` relies on
//!    this: it runs the live target with `--include-ignored`.

use std::path::{Path, PathBuf};

/// Exempts a runtime-gated fast probe from rule 2. Must appear on a comment
/// line within the attribute block directly above the `#[test]`.
const ALLOW_UNGATED_MARKER: &str = "// allow-ungated-live:";

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// Regular files directly inside `dir` (non-recursive), sorted.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry"))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Top-level `live_*.rs` files in `dir` — each would be its own test binary.
fn stray_live_binaries(files: &[PathBuf]) -> Vec<&PathBuf> {
    files
        .iter()
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "rs")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("live_"))
        })
        .collect()
}

/// 1-based line numbers of `#[test]` attributes in `src` that are not gated.
fn ungated_tests(src: &str) -> Vec<usize> {
    let src = src.replace("\r\n", "\n");
    let lines: Vec<&str> = src.lines().collect();
    (0..lines.len())
        .filter(|&idx| lines[idx].trim() == "#[test]" && !test_is_gated(&lines, idx))
        .map(|idx| idx + 1)
        .collect()
}

/// A `#[test]` is gated if an `#[ignore` attribute sits in the contiguous
/// attribute/comment block around it (above, or below before the `fn` — so
/// `#[test] #[cfg(unix)] #[ignore]` counts), or an allow marker sits above it.
fn test_is_gated(lines: &[&str], test_idx: usize) -> bool {
    let above = lines[..test_idx]
        .iter()
        .rev()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("#[") || l.starts_with("//"));
    for line in above {
        if line.starts_with("#[ignore") || line.contains(ALLOW_UNGATED_MARKER) {
            return true;
        }
    }
    lines[test_idx + 1..]
        .iter()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("#["))
        .any(|l| l.starts_with("#[ignore"))
}

#[test]
fn no_top_level_live_test_binaries() {
    let files = files_in(&tests_dir());
    assert!(
        !files.is_empty(),
        "no files found in {}",
        tests_dir().display()
    );
    let stray = stray_live_binaries(&files);
    assert!(
        stray.is_empty(),
        "found {} top-level live-test binary file(s). Live suites must be modules \
         of tests/live/main.rs, not standalone tests/live_*.rs binaries (iter-100b). \
         Move each into tests/live/<slug>.rs and add a `mod` line to \
         tests/live/main.rs:\n  {}",
        stray.len(),
        stray
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn every_live_test_is_ignore_gated() {
    let live_dir = tests_dir().join("live");
    let files: Vec<PathBuf> = files_in(&live_dir)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    assert!(
        !files.is_empty(),
        "no .rs files found in {}",
        live_dir.display()
    );
    let mut tests_seen = 0usize;
    let mut violations = Vec::new();
    for path in &files {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        tests_seen += src.lines().filter(|l| l.trim() == "#[test]").count();
        for line in ungated_tests(&src) {
            violations.push(format!("{}:{line}", path.display()));
        }
    }
    assert!(
        tests_seen > 0,
        "no #[test] found under {}",
        live_dir.display()
    );
    assert!(
        violations.is_empty(),
        "found {} ungated `#[test]` under {}. Every live test must carry `#[ignore]` \
         so a plain `cargo test` stays Firefox-free (a bare live test hangs a \
         Firefox-less CI job for the whole job budget — iter-112/113). Add \
         `#[ignore = \"…\"]`, or, for a runtime-gated fast probe that must run by \
         default, an `{ALLOW_UNGATED_MARKER} <reason>` comment in the attribute \
         block:\n  {}",
        violations.len(),
        live_dir.display(),
        violations.join("\n  ")
    );
}

#[test]
fn gating_rule_accepts_ignore_above_below_and_the_marker() {
    assert!(ungated_tests("#[test]\n#[ignore = \"live\"]\nfn t() {}\n").is_empty());
    assert!(ungated_tests("#[ignore]\n#[test]\nfn t() {}\n").is_empty());
    assert!(ungated_tests("#[test]\n#[cfg(unix)]\n#[ignore = \"live\"]\nfn t() {}\n").is_empty());
    assert!(ungated_tests("// allow-ungated-live: mock\n#[test]\nfn t() {}\n").is_empty());
    assert!(ungated_tests("#[test]\r\n#[ignore]\r\nfn t() {}\r\n").is_empty());
}

#[test]
fn gating_rule_rejects_a_bare_test() {
    assert_eq!(ungated_tests("#[test]\nfn bare() {}\n"), vec![1]);
    // An `#[ignore]` on a different item does not gate this one.
    assert_eq!(
        ungated_tests("#[ignore]\nfn other() {}\n\n#[test]\nfn bare() {}\n"),
        vec![4]
    );
}

#[test]
fn stray_rule_matches_only_top_level_live_rs() {
    let files = [
        PathBuf::from("tests").join("live_x.rs"),
        PathBuf::from("tests").join("cli_version.rs"),
        PathBuf::from("tests").join("live_notes.md"),
    ];
    assert_eq!(stray_live_binaries(&files), vec![&files[0]]);
}
