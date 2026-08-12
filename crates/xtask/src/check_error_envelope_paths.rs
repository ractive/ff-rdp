use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::Path;

/// iter-145 Theme C regression guard.
///
/// The defect class this iteration exists to fix (see
/// `kb/iterations/iteration-145-error-envelope-completeness.md`): a command
/// prints a genuine error to stderr with `eprintln!` and then bypasses the
/// JSON error envelope entirely by returning `AppError::Exit(N)` — a variant
/// `main` treats as "already printed, just exit" (see `error.rs`). A scripted
/// consumer parsing stdout for `--jq '.error_type'` gets an empty string and
/// a bare exit code instead of a parseable envelope.
///
/// This check enumerates every `eprintln!` call site under
/// `crates/ff-rdp-cli/src/commands/` (excluding `#[cfg(test)]` modules) and
/// fails if any of them is immediately followed by a `return`/`Err` that
/// constructs `AppError::Exit(` — i.e. the exact "print then bypass" idiom —
/// unless a `// stderr-ok:` justification comment appears on the `eprintln!`
/// line itself or within the two lines above it.
///
/// Scope note: this intentionally does not require every stderr-emitting
/// `eprintln!` in `commands/` to carry a justification comment — the sweep in
/// iter-145 found dozens of legitimate warn-and-continue / debug / progress
/// lines that are correctly stderr and not part of this defect class (see the
/// plan's Theme B). Annotating that long tail is deferred to a sibling plan
/// per the plan's own Notes section. This check's job is narrower and
/// permanent: never again let a bare-stderr-then-bypass path land.
#[derive(ClapArgs)]
pub struct Args {
    /// Directory to scan (default: crates/ff-rdp-cli/src/commands relative to repo root).
    #[arg(long)]
    dir: Option<String>,
}

/// A rejected "print then bypass the envelope" finding.
#[derive(Debug, PartialEq, Eq)]
pub struct EnvelopeFinding {
    pub file: String,
    pub line: usize,
    pub content: String,
}

const LOOKAHEAD_LINES: usize = 6;
const LOOKBACK_JUSTIFICATION_LINES: usize = 2;
const JUSTIFICATION_MARKER: &str = "stderr-ok:";

/// Strip everything from the first `#[cfg(test)]` module onward — a cheap
/// heuristic (not a real parser) that is good enough because every source
/// file in `commands/` puts its `#[cfg(test)] mod tests { ... }` block last.
fn strip_test_module(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(idx) => &src[..idx],
        None => src,
    }
}

/// Scan one file's source text for the print-then-bypass idiom.
pub fn check_source(file_label: &str, src: &str) -> Vec<EnvelopeFinding> {
    let scoped = strip_test_module(src);
    let lines: Vec<&str> = scoped.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("eprintln!") {
            continue;
        }

        // Justification: a `// stderr-ok:` comment on this line or the two
        // lines immediately above it exempts this site.
        let justified = (idx.saturating_sub(LOOKBACK_JUSTIFICATION_LINES)..=idx)
            .filter_map(|i| lines.get(i))
            .any(|l| l.contains(JUSTIFICATION_MARKER));
        if justified {
            continue;
        }

        // Look ahead a bounded window for the bypass pattern. We deliberately
        // scan raw text rather than parsing Rust — `AppError::Exit(` inside a
        // `return Err(...)` (or a bare trailing `Err(...)` in a match arm) a
        // few lines after the eprintln! is the whole idiom this class of bug
        // takes.
        let window_end = (idx + 1 + LOOKAHEAD_LINES).min(lines.len());
        let window = &lines[idx + 1..window_end];
        if window.iter().any(|l| l.contains("AppError::Exit(")) {
            findings.push(EnvelopeFinding {
                file: file_label.to_owned(),
                line: idx + 1,
                content: line.trim().to_owned(),
            });
        }
    }

    findings
}

fn scan_dir(dir: &Path, repo_root: &Path) -> Result<Vec<EnvelopeFinding>> {
    let mut findings = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            findings.extend(scan_dir(&path, repo_root)?);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let label = path
            .strip_prefix(repo_root)
            .unwrap_or(&path)
            .display()
            .to_string();
        findings.extend(check_source(&label, &src));
    }

    Ok(findings)
}

fn locate_repo_root() -> Result<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("running git rev-parse --show-toplevel")?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse --show-toplevel failed");
    }
    let s = String::from_utf8(output.stdout).context("non-utf8 git output")?;
    Ok(std::path::PathBuf::from(s.trim()))
}

pub fn run(args: Args) -> Result<()> {
    let repo_root = locate_repo_root()?;
    let dir = match args.dir {
        Some(d) => repo_root.join(d),
        None => repo_root.join("crates/ff-rdp-cli/src/commands"),
    };

    if !dir.exists() {
        anyhow::bail!("directory does not exist: {}", dir.display());
    }

    let findings = scan_dir(&dir, &repo_root)?;

    if findings.is_empty() {
        println!("check-error-envelope-paths: PASS (no bare-stderr-then-bypass error paths)");
        return Ok(());
    }

    eprintln!("check-error-envelope-paths: bare-stderr-then-bypass error paths found:");
    eprintln!(
        "  Route the error through the standard AppError envelope (e.g. AppError::User /\n  \
         AppError::Timeout) instead of `eprintln!` + `AppError::Exit(N)`, or add a\n  \
         `// stderr-ok: <reason>` comment on/above the eprintln! if this one is deliberate."
    );
    eprintln!();
    for f in &findings {
        eprintln!("{}:{}: {}", f.file, f.line, f.content);
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_eprintln_immediately_before_exit_bypass() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    if bad {
        eprintln!("error: {}", msg);
        return Err(AppError::Exit(1));
    }
    Ok(())
}
"#;
        let findings = check_source("crates/ff-rdp-cli/src/commands/fake.rs", src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 4);
    }

    #[test]
    fn allows_warn_and_continue_eprintln() {
        let src = r#"
fn do_thing() {
    if let Err(e) = fallible() {
        eprintln!("warning: could not do the thing: {e}");
    }
}
"#;
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn allows_exit_bypass_that_prints_far_away() {
        // AppError::Exit appears more than LOOKAHEAD_LINES away — outside the
        // detection window, treated as unrelated.
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
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn stderr_ok_annotation_on_same_line_exempts() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    eprintln!("error: {}", msg); // stderr-ok: pre-existing, tracked separately
    return Err(AppError::Exit(1));
}
"#;
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn stderr_ok_annotation_above_exempts() {
        let src = r#"
fn do_thing() -> Result<(), AppError> {
    // stderr-ok: this one intentionally bypasses the envelope, see iter-NN
    eprintln!("error: {}", msg);
    return Err(AppError::Exit(1));
}
"#;
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_modules_are_excluded() {
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
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn click_rs_bug_shape_is_caught_pre_fix() {
        // Regression shape check: the exact iter-145 click.rs pattern before
        // the fix, reproduced verbatim in miniature, must be caught.
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
        let findings = check_source("crates/ff-rdp-cli/src/commands/click.rs", src);
        assert_eq!(findings.len(), 1);
    }
}
