use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    plan: PathBuf,
}

/// A reference to a Firefox source file in an iteration plan.
#[derive(Debug, Deserialize)]
pub struct FirefoxRef {
    pub path: String,
    pub lines: String,
    pub why: String,
}

/// Frontmatter shape that includes firefox_refs.
#[derive(Debug, Deserialize, Default)]
struct FirefoxRefsFrontmatter {
    #[serde(default)]
    firefox_refs: Option<Vec<FirefoxRef>>,
}

/// Parse the `lines` field as `"<start>-<end>"` (inclusive, 1-based).
/// Returns `(start, end)` on success.
pub fn parse_line_range(lines: &str) -> Result<(usize, usize)> {
    let (start_str, end_str) = lines.split_once('-').with_context(|| {
        format!(
            "malformed lines range {:?}: expected \"<start>-<end>\"",
            lines
        )
    })?;
    let start: usize = start_str
        .trim()
        .parse()
        .with_context(|| format!("malformed start in lines range {:?}", lines))?;
    let end: usize = end_str
        .trim()
        .parse()
        .with_context(|| format!("malformed end in lines range {:?}", lines))?;
    if start == 0 {
        bail!("lines range {:?}: start must be >= 1 (1-based)", lines);
    }
    if start > end {
        bail!(
            "lines range {:?}: start {} > end {} (range is empty)",
            lines,
            start,
            end
        );
    }
    Ok((start, end))
}

/// Validate a single FirefoxRef against the filesystem.
fn validate_ref(firefox_root: &std::path::Path, r: &FirefoxRef) -> Result<()> {
    let file_path = firefox_root.join(&r.path);
    if !file_path.exists() {
        bail!("firefox_refs: file not found: {} (why: {})", r.path, r.why);
    }

    let (start, end) = parse_line_range(&r.lines)
        .with_context(|| format!("firefox_refs: {} (why: {})", r.path, r.why))?;

    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("failed to read {}", r.path))?;
    let total_lines = content.lines().count();

    if end > total_lines {
        bail!(
            "firefox_refs: {} lines {}-{} out of range (file has {} lines; why: {})",
            r.path,
            start,
            end,
            total_lines,
            r.why
        );
    }

    Ok(())
}

pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("failed to read {:?}", args.plan))?;

    let refs_fm = extract_firefox_refs_frontmatter(&content)?;

    let firefox_refs = match refs_fm.firefox_refs {
        None => {
            println!(
                "check-firefox-refs: OK (no firefox_refs key in {:?})",
                args.plan
            );
            return Ok(());
        }
        Some(refs) if refs.is_empty() => {
            println!(
                "check-firefox-refs: OK (no firefox_refs key in {:?})",
                args.plan
            );
            return Ok(());
        }
        Some(refs) => refs,
    };

    let firefox_root = resolve_firefox_root()?;

    let mut errors: Vec<String> = Vec::new();
    for r in &firefox_refs {
        if let Err(e) = validate_ref(&firefox_root, r) {
            errors.push(format!("{e:#}"));
        }
    }

    if errors.is_empty() {
        println!(
            "check-firefox-refs: OK: {} firefox_refs verified",
            firefox_refs.len()
        );
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  - {e}");
        }
        bail!(
            "check-firefox-refs: {} ref(s) failed validation in {:?}",
            errors.len(),
            args.plan
        );
    }
}

/// Extract the raw YAML frontmatter and deserialize only the firefox_refs key.
/// Normalises CRLF to LF before scanning for the closing delimiter so the same
/// logic works on Windows checkouts.
fn extract_firefox_refs_frontmatter(content: &str) -> Result<FirefoxRefsFrontmatter> {
    let normalised = content.replace("\r\n", "\n");
    let trimmed = normalised.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(FirefoxRefsFrontmatter::default());
    }
    let after_open = &trimmed[3..];
    let close_pos = after_open
        .find("\n---")
        .context("unterminated YAML frontmatter (no closing ---)")?;
    let yaml_text = &after_open[..close_pos];
    let fm: FirefoxRefsFrontmatter =
        serde_yaml::from_str(yaml_text).context("failed to parse YAML frontmatter")?;
    Ok(fm)
}

/// Resolve the Firefox source root from the environment.
/// Tries `FF_RDP_FIREFOX_PATH` first, then `$HOME/devel/firefox` as a
/// best-effort default. Errors clearly if neither exists.
fn resolve_firefox_root() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("FF_RDP_FIREFOX_PATH") {
        let root = PathBuf::from(&path);
        if root.exists() {
            return Ok(root);
        }
        bail!(
            "FF_RDP_FIREFOX_PATH={:?} does not exist. \
             Point it at the root of your Firefox checkout.",
            root
        );
    }

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let candidate = PathBuf::from(home).join("devel").join("firefox");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    bail!(
        "Firefox source root not found. \
         Set FF_RDP_FIREFOX_PATH to the root of your Firefox checkout \
         (e.g. the directory containing `devtools/`)."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_range_valid() {
        assert_eq!(parse_line_range("1-10").unwrap(), (1, 10));
        assert_eq!(parse_line_range("42-42").unwrap(), (42, 42));
        assert_eq!(parse_line_range(" 5 - 20 ").unwrap(), (5, 20));
    }

    #[test]
    fn parse_line_range_malformed_no_dash() {
        assert!(parse_line_range("10").is_err());
    }

    #[test]
    fn parse_line_range_malformed_not_numbers() {
        assert!(parse_line_range("a-b").is_err());
    }

    #[test]
    fn parse_line_range_start_zero() {
        assert!(parse_line_range("0-5").is_err());
    }

    #[test]
    fn parse_line_range_empty_range() {
        assert!(parse_line_range("10-5").is_err());
    }
}
