//! One subcommand, one CI step, three source-scanning invariants.
//!
//! iter-162a merged `check-daemon-locks` (iter-63), `check-error-envelope-paths`
//! (iter-145 Theme C) and `check-stderr-annotations` (iter-148) into this file.
//! All three did the same thing — regex-scan product source for one specific
//! defect shape — and two of them already shared [`crate::stderr_scan`]'s walk.
//! Each invariant keeps its own named result line, so a failure still says which
//! one fired and how to fix it.

use crate::stderr_scan::{locate_repo_root, scan_rs_files, strip_test_module};
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(ClapArgs)]
pub struct Args {
    /// Directory scanned by the `daemon-locks` invariant (relative to repo root).
    #[arg(long, default_value = "crates/ff-rdp-cli/src/daemon")]
    daemon_dir: PathBuf,

    /// Directory scanned by the `error-envelope-paths` and `stderr-annotations`
    /// invariants (relative to repo root).
    #[arg(long, default_value = "crates/ff-rdp-cli/src/commands")]
    commands_dir: PathBuf,
}

/// One violation of one invariant.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub content: String,
}

const LOOKAHEAD_LINES: usize = 6;
const LOOKBACK_JUSTIFICATION_LINES: usize = 2;
const JUSTIFICATION_MARKER: &str = "// stderr-ok:";

// --- invariant: daemon-locks (iter-63) ---------------------------------------

/// Matches `.lock().unwrap()`, including rustfmt-split chains like
/// `firefox_writer\n    .lock()\n    .unwrap()` — those were bypassing the
/// original check until iter-63's post-review hardening added the `\s*`.
///
/// `.lock().expect(...)` is intentionally NOT matched: `#[cfg(test)]` modules
/// still use that form against a `buffer` mutex where panic-on-poison is the
/// desired test behaviour.
fn lock_unwrap_regex() -> Result<Regex> {
    Regex::new(r"\.lock\(\)\s*\.unwrap\(\)").context("compiling lock-unwrap regex")
}

/// Scan one file for `.lock().unwrap()`. The daemon must use `lock_or_recover!`
/// so a poisoned mutex doesn't take the whole process down.
pub fn check_daemon_locks_source(file_label: &str, src: &str, pattern: &Regex) -> Vec<Finding> {
    let mut findings = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0usize;

    while i < lines.len() {
        // Single-line match.
        if pattern.is_match(lines[i]) {
            findings.push(Finding {
                file: file_label.to_owned(),
                line: i + 1,
                content: lines[i].trim().to_owned(),
            });
            i += 1;
            continue;
        }
        // Two- then three-line windows, whitespace-normalised, for split chains.
        let mut matched = false;
        for window_len in 2..=3 {
            if i + window_len > lines.len() {
                break;
            }
            let window = lines[i..i + window_len]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join(" ");
            if pattern.is_match(&window) {
                findings.push(Finding {
                    file: file_label.to_owned(),
                    line: i + 1,
                    content: lines[i].trim().to_owned(),
                });
                i += window_len;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }

    findings
}

// --- invariant: error-envelope-paths (iter-145 Theme C) ----------------------

/// Scan one file for the print-then-bypass idiom: an `eprintln!` that is
/// followed within a bounded window by a `AppError::Exit(N)` — a variant `main`
/// treats as "already printed, just exit", so a scripted consumer parsing
/// stdout for `--jq '.error_type'` gets an empty string and a bare exit code
/// instead of a parseable envelope.
///
/// A `// stderr-ok:` comment on the `eprintln!` line or the two lines above it
/// exempts the site.
pub fn check_error_envelope_source(file_label: &str, src: &str) -> Vec<Finding> {
    let scoped = strip_test_module(src);
    let lines: Vec<&str> = scoped.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("eprintln!") || is_justified(&lines, idx) {
            continue;
        }

        // Raw text, deliberately not a Rust parse: `AppError::Exit(` inside a
        // `return Err(...)` (or a bare trailing `Err(...)` in a match arm) a few
        // lines after the eprintln! is the whole idiom.
        let window_end = (idx + 1 + LOOKAHEAD_LINES).min(lines.len());
        if lines[idx + 1..window_end]
            .iter()
            .any(|l| l.contains("AppError::Exit("))
        {
            findings.push(Finding {
                file: file_label.to_owned(),
                line: idx + 1,
                content: line.trim().to_owned(),
            });
        }
    }

    findings
}

// --- invariant: stderr-annotations (iter-148) --------------------------------

/// Scan one file for `eprintln!` sites missing their `// stderr-ok: <reason>`
/// justification comment. iter-148 annotated the whole legitimate-stderr tail
/// (progress lines, `debug:` diagnostics, warn-and-continue cleanup, `hint:`
/// suggestions) so a future sweep can trust the comments; this keeps it true.
pub fn check_stderr_annotations_source(file_label: &str, src: &str) -> Vec<Finding> {
    let scoped = strip_test_module(src);
    let lines: Vec<&str> = scoped.lines().collect();

    lines
        .iter()
        .enumerate()
        .filter(|(idx, line)| line.contains("eprintln!") && !is_justified(&lines, *idx))
        .map(|(idx, line)| Finding {
            file: file_label.to_owned(),
            line: idx + 1,
            content: line.trim().to_owned(),
        })
        .collect()
}

/// True if a `// stderr-ok:` comment sits on line `idx` or the two above it.
fn is_justified(lines: &[&str], idx: usize) -> bool {
    (idx.saturating_sub(LOOKBACK_JUSTIFICATION_LINES)..=idx)
        .filter_map(|i| lines.get(i))
        .any(|l| l.contains(JUSTIFICATION_MARKER))
}

// --- driver ------------------------------------------------------------------

/// Remediation text printed under a failing invariant.
fn remedy(invariant: &str) -> &'static str {
    match invariant {
        "daemon-locks" => {
            "  Use `lock_or_recover!` instead of `.lock().unwrap()` so a poisoned mutex\n  \
             doesn't take the whole daemon process down (iter-63)."
        }
        "error-envelope-paths" => {
            "  Route the error through the standard AppError envelope (e.g. AppError::User /\n  \
             AppError::Timeout) instead of `eprintln!` + `AppError::Exit(N)`, or add a\n  \
             `// stderr-ok: <reason>` comment on/above the eprintln! if this one is deliberate."
        }
        _ => {
            "  Add a `// stderr-ok: <reason>` comment on the eprintln! line itself or within\n  \
             the two lines above it, classifying why this site legitimately writes to\n  \
             stderr (see kb/iterations/iteration-148-stderr-path-annotations.md)."
        }
    }
}

/// Print one named result line for an invariant; returns true if it passed.
fn report(invariant: &str, findings: &[Finding]) -> bool {
    if findings.is_empty() {
        println!("check-source-invariants: {invariant} OK");
        return true;
    }
    println!(
        "check-source-invariants: {invariant} FAIL ({} site(s))",
        findings.len()
    );
    println!("{}", remedy(invariant));
    for f in findings {
        println!("  {}:{}: {}", f.file, f.line, f.content);
    }
    false
}

/// Resolve a possibly-relative directory against the repo root and require it
/// to exist.
fn resolve_dir(repo_root: &Path, dir: &Path) -> Result<PathBuf> {
    let resolved = repo_root.join(dir);
    if !resolved.exists() {
        anyhow::bail!("directory does not exist: {}", resolved.display());
    }
    Ok(resolved)
}

pub fn run(args: Args) -> Result<()> {
    let repo_root = locate_repo_root()?;
    let daemon_dir = resolve_dir(&repo_root, &args.daemon_dir)?;
    let commands_dir = resolve_dir(&repo_root, &args.commands_dir)?;

    let pattern = lock_unwrap_regex()?;
    let daemon_findings = scan_rs_files(&daemon_dir, &repo_root, &mut |label, src| {
        check_daemon_locks_source(label, src, &pattern)
    })?;
    let envelope_findings =
        scan_rs_files(&commands_dir, &repo_root, &mut check_error_envelope_source)?;
    let annotation_findings = scan_rs_files(
        &commands_dir,
        &repo_root,
        &mut check_stderr_annotations_source,
    )?;

    let mut ok = true;
    ok &= report("daemon-locks", &daemon_findings);
    ok &= report("error-envelope-paths", &envelope_findings);
    ok &= report("stderr-annotations", &annotation_findings);

    if ok {
        Ok(())
    } else {
        anyhow::bail!("check-source-invariants: one or more invariants failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon_findings(src: &str) -> Vec<Finding> {
        let pattern = lock_unwrap_regex().unwrap();
        check_daemon_locks_source("fake.rs", src, &pattern)
    }

    #[test]
    fn daemon_locks_pass_when_no_unwraps() {
        assert!(daemon_findings("fn f() { let _ = lock_or_recover!(state.x); }\n").is_empty());
    }

    #[test]
    fn daemon_locks_catch_single_line() {
        let findings = daemon_findings("fn f() { let _ = state.mu.lock().unwrap(); }\n");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 1);
    }

    #[test]
    fn daemon_locks_catch_rustfmt_split_chain() {
        // The post-review gap that motivated the `\s*` combination in iter-63.
        let src = "fn f() {\n    firefox_writer\n        .lock()\n        .unwrap()\n        .send(&msg);\n}\n";
        let findings = daemon_findings(src);
        assert_eq!(findings.len(), 1, "multiline split must still be caught");
    }

    #[test]
    fn daemon_locks_allow_lock_expect() {
        // `.lock().expect(...)` is deliberately out of scope — test modules use it.
        assert!(daemon_findings("let g = m.lock().expect(\"poisoned\");\n").is_empty());
    }

    #[test]
    fn envelope_detects_eprintln_immediately_before_exit_bypass() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    if bad {
        eprintln!("error: {}", msg);
        return Err(AppError::Exit(1));
    }
    Ok(())
}
"#;
        let findings = check_error_envelope_source("crates/ff-rdp-cli/src/commands/fake.rs", src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 4);
    }

    #[test]
    fn envelope_allows_warn_and_continue_eprintln() {
        let src = r#"
fn do_thing() {
    if let Err(e) = fallible() {
        eprintln!("warning: could not do the thing: {e}");
    }
}
"#;
        assert!(check_error_envelope_source("fake.rs", src).is_empty());
    }

    #[test]
    fn envelope_allows_exit_bypass_that_prints_far_away() {
        // AppError::Exit appears beyond LOOKAHEAD_LINES — treated as unrelated.
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    eprintln!("debug: something");
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
    return Err(AppError::Exit(1));
}
"#;
        assert!(check_error_envelope_source("fake.rs", src).is_empty());
    }

    #[test]
    fn envelope_stderr_ok_annotation_on_same_line_exempts() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    eprintln!("error: {}", msg); // stderr-ok: pre-existing, tracked separately
    return Err(AppError::Exit(1));
}
"#;
        assert!(check_error_envelope_source("fake.rs", src).is_empty());
    }

    #[test]
    fn envelope_stderr_ok_annotation_above_exempts() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    // stderr-ok: this one intentionally bypasses the envelope, see iter-NN
    eprintln!("error: {}", msg);
    return Err(AppError::Exit(1));
}
"#;
        assert!(check_error_envelope_source("fake.rs", src).is_empty());
    }

    #[test]
    fn envelope_rejects_marker_text_outside_a_comment() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    let msg = "stderr-ok: not actually a justification comment";
    eprintln!("{msg}");
    return Err(AppError::Exit(1));
}
"#;
        assert_eq!(check_error_envelope_source("fake.rs", src).len(), 1);
    }

    #[test]
    fn envelope_test_modules_are_excluded() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn some_test() {
        eprintln!("error: test output");
        return Err(AppError::Exit(1));
    }
}
"#;
        assert!(check_error_envelope_source("fake.rs", src).is_empty());
    }

    #[test]
    fn envelope_click_rs_bug_shape_is_caught_pre_fix() {
        // The exact iter-145 click.rs pattern before the fix, in miniature.
        let src = r#"
fn do_click() -> Result<(Value, Option<String>), AppError> {
    let msg = exc.message.unwrap_or_else(|| "click failed".to_owned());
    if !msg.contains(ELEMENT_NOT_FOUND_MARKER) {
        eprintln!("error: {}", sanitize_for_terminal(&msg));
        return Err(AppError::Exit(1));
    }
    Ok((Value::Null, None))
}
"#;
        assert_eq!(
            check_error_envelope_source("crates/ff-rdp-cli/src/commands/click.rs", src).len(),
            1
        );
    }

    #[test]
    fn annotations_flag_unannotated_eprintln() {
        let src = r#"
fn do_thing() {
    eprintln!("warning: something went wrong: {e}");
}
"#;
        let findings = check_stderr_annotations_source("fake.rs", src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 3);
    }

    #[test]
    fn annotations_allow_same_line_comment() {
        let src = r#"
fn do_thing() {
    eprintln!("warning: something went wrong: {e}"); // stderr-ok: (b) warn-and-continue
}
"#;
        assert!(check_stderr_annotations_source("fake.rs", src).is_empty());
    }

    #[test]
    fn annotations_allow_comment_within_two_lines_above() {
        let src = r#"
fn do_thing() {
    // stderr-ok: (b) debug/diagnostic, gated on --verbose.
    eprintln!("debug: something");
}
"#;
        assert!(check_stderr_annotations_source("fake.rs", src).is_empty());
    }

    #[test]
    fn annotations_reject_comment_more_than_two_lines_above() {
        let src = r#"
fn do_thing() {
    // stderr-ok: (b) debug/diagnostic, gated on --verbose.
    let unrelated = 1;
    let _ = unrelated;
    eprintln!("debug: something");
}
"#;
        assert_eq!(check_stderr_annotations_source("fake.rs", src).len(), 1);
    }

    #[test]
    fn annotations_reject_marker_text_outside_a_comment() {
        let src = r#"
fn do_thing() {
    let msg = "stderr-ok: not actually a justification comment";
    eprintln!("{msg}");
}
"#;
        assert_eq!(check_stderr_annotations_source("fake.rs", src).len(), 1);
    }

    #[test]
    fn annotations_test_modules_are_excluded() {
        let src = r#"
fn do_thing() {
}

#[cfg(test)]
mod tests {
    #[test]
    fn some_test() {
        eprintln!("error: test output");
    }
}
"#;
        assert!(check_stderr_annotations_source("fake.rs", src).is_empty());
    }
}
