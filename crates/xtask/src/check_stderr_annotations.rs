use crate::stderr_scan::{locate_repo_root, scan_rs_files, strip_test_module};
use anyhow::Result;
use clap::Args as ClapArgs;

/// iter-148 companion check: annotation coverage guard.
///
/// `check-error-envelope-paths` (iter-145 Theme C) only flags the narrow
/// print-then-bypass bug shape; it deliberately leaves the much larger
/// "legitimate stderr" long tail (progress lines, `debug:`-gated
/// diagnostics, warn-and-continue best-effort cleanup, `hint:`
/// suggestions) unannotated, per its own scope note. iter-148 annotated
/// that whole tail with `// stderr-ok: <reason>` comments so a future
/// sweep can trust the comments instead of re-deriving the classification
/// from scratch — this check is the regression guard that keeps it true:
/// it fails if any `eprintln!` under `crates/ff-rdp-cli/src/commands/`
/// (excluding `#[cfg(test)]` modules) lacks a `// stderr-ok:` comment on
/// its own line or within the two lines above it.
#[derive(ClapArgs)]
pub struct Args {
    /// Directory to scan (default: crates/ff-rdp-cli/src/commands relative to repo root).
    #[arg(long)]
    dir: Option<String>,
}

/// An `eprintln!` site missing its `// stderr-ok:` justification comment.
#[derive(Debug, PartialEq, Eq)]
pub struct UnannotatedFinding {
    pub file: String,
    pub line: usize,
    pub content: String,
}

const LOOKBACK_JUSTIFICATION_LINES: usize = 2;
const JUSTIFICATION_MARKER: &str = "stderr-ok:";

/// Scan one file's source text for `eprintln!` sites missing the
/// `// stderr-ok:` justification comment.
pub fn check_source(file_label: &str, src: &str) -> Vec<UnannotatedFinding> {
    let scoped = strip_test_module(src);
    let lines: Vec<&str> = scoped.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("eprintln!") {
            continue;
        }

        let justified = (idx.saturating_sub(LOOKBACK_JUSTIFICATION_LINES)..=idx)
            .filter_map(|i| lines.get(i))
            .any(|l| l.contains(JUSTIFICATION_MARKER));
        if justified {
            continue;
        }

        findings.push(UnannotatedFinding {
            file: file_label.to_owned(),
            line: idx + 1,
            content: line.trim().to_owned(),
        });
    }

    findings
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

    let findings = scan_rs_files(&dir, &repo_root, &mut check_source)?;

    if findings.is_empty() {
        println!("check-stderr-annotations: PASS (every eprintln! carries a stderr-ok comment)");
        return Ok(());
    }

    eprintln!("check-stderr-annotations: unannotated eprintln! sites found:");
    eprintln!(
        "  Add a `// stderr-ok: <reason>` comment on the eprintln! line itself or within\n  \
         the two lines above it, classifying why this site legitimately writes to\n  \
         stderr (see kb/iterations/iteration-148-stderr-path-annotations.md)."
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
    fn flags_unannotated_eprintln() {
        let src = r#"
fn do_thing() {
    eprintln!("warning: something went wrong: {e}");
}
"#;
        let findings = check_source("fake.rs", src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 3);
    }

    #[test]
    fn allows_annotation_on_same_line() {
        let src = r#"
fn do_thing() {
    eprintln!("warning: something went wrong: {e}"); // stderr-ok: (b) warn-and-continue
}
"#;
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn allows_annotation_within_two_lines_above() {
        let src = r#"
fn do_thing() {
    // stderr-ok: (b) debug/diagnostic, gated on --verbose.
    eprintln!("debug: something");
}
"#;
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }

    #[test]
    fn rejects_annotation_more_than_two_lines_above() {
        let src = r#"
fn do_thing() {
    // stderr-ok: (b) debug/diagnostic, gated on --verbose.
    let unrelated = 1;
    let _ = unrelated;
    eprintln!("debug: something");
}
"#;
        let findings = check_source("fake.rs", src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_modules_are_excluded() {
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
        let findings = check_source("fake.rs", src);
        assert!(findings.is_empty());
    }
}
