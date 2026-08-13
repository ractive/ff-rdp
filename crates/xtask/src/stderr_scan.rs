//! Shared source-scanning helpers.
//!
//! The three invariants in [`crate::check_source_invariants`] walk directory
//! trees the same way and (for the two `eprintln!` ones) apply the same
//! test-module exclusion rule. This module holds that common walk so each
//! invariant only differs in what it looks for at each site.
//!
//! [`locate_repo_root`] is also used by `check-dogfood-script` to find
//! `tools/lint-dogfood-script.sh`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Strip everything from the first `#[cfg(test)]` module onward — a cheap
/// heuristic (not a real parser) that is good enough because every source
/// file in `commands/` puts its `#[cfg(test)] mod tests { ... }` block last.
pub fn strip_test_module(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(idx) => &src[..idx],
        None => src,
    }
}

pub fn locate_repo_root() -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("running git rev-parse --show-toplevel")?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse --show-toplevel failed");
    }
    let s = String::from_utf8(output.stdout).context("non-utf8 git output")?;
    Ok(PathBuf::from(s.trim()))
}

/// Recursively walk `dir` for `.rs` files (sorted for deterministic output),
/// running `check_source` against each file's (repo-relative label, full
/// source text) and collecting all findings.
pub fn scan_rs_files<T>(
    dir: &Path,
    repo_root: &Path,
    check_source: &mut dyn FnMut(&str, &str) -> Vec<T>,
) -> Result<Vec<T>> {
    let mut findings = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            findings.extend(scan_rs_files(&path, repo_root, check_source)?);
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
