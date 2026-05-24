use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;

fn iter_branch_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^iter-([0-9]+[a-z]?)/").unwrap())
}

#[derive(ClapArgs)]
pub struct Args {
    /// Git branch name to resolve (e.g. `iter-75b/some-slug`).
    #[arg(long)]
    pub branch: String,

    /// Repository root directory (default: current working directory).
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
}

/// Parse `iter-<N>/...` or `iter-<N[a-z]>/...` from a branch name.
/// Returns the iteration ID string (e.g. `"75b"`, `"77"`) on success.
pub(crate) fn parse_iter_id(branch: &str) -> Option<String> {
    iter_branch_re()
        .captures(branch)
        .map(|caps| caps[1].to_owned())
}

/// Resolve the plan file for the given iteration ID under `repo_root/kb/iterations/`.
/// Returns the absolute path if exactly one match is found.
pub(crate) fn resolve_plan(iter_id: &str, repo_root: &std::path::Path) -> Result<PathBuf> {
    let iterations_dir = repo_root.join("kb").join("iterations");
    if !iterations_dir.is_dir() {
        bail!(
            "kb/iterations/ directory not found under {}",
            repo_root.display()
        );
    }

    let prefix = format!("iteration-{iter_id}-");
    let mut matches: Vec<PathBuf> = std::fs::read_dir(&iterations_dir)
        .map_err(|e| anyhow!("reading kb/iterations/: {e}"))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) && name_str.ends_with(".md") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    matches.sort();

    match matches.len() {
        0 => bail!(
            "no plan found matching 'iteration-{iter_id}-*.md' under {}\n\
             Hint: create the plan file first: kb/iterations/iteration-{iter_id}-<slug>.md",
            iterations_dir.display()
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let names: Vec<String> = matches
                .iter()
                .map(|p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            bail!(
                "multiple plans found for iteration-{iter_id}: {}\n\
                 Disambiguate by removing duplicate plan files.",
                names.join(", ")
            )
        }
    }
}

pub fn run(args: Args) -> Result<()> {
    let iter_id = parse_iter_id(&args.branch).ok_or_else(|| {
        anyhow!(
            "not an iter-* branch: {}\n\
             Expected format: iter-<N>/<slug> or iter-<Na>/<slug> (e.g. iter-77/my-feature)",
            args.branch
        )
    })?;

    let repo_root = match args.repo_root {
        Some(r) => r,
        None => {
            // Detect from git rev-parse.
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .output()
                .map_err(|e| anyhow!("running git rev-parse: {e}"))?;
            if !output.status.success() {
                bail!("git rev-parse --show-toplevel failed");
            }
            let s = String::from_utf8(output.stdout)
                .map_err(|e| anyhow!("non-utf8 git output: {e}"))?;
            PathBuf::from(s.trim())
        }
    };

    let plan_path = resolve_plan(&iter_id, &repo_root)?;
    println!("{}", plan_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iter_id_pure_integer() {
        assert_eq!(
            parse_iter_id("iter-77/spec-drift-and-windows-reparse-points"),
            Some("77".to_owned())
        );
    }

    #[test]
    fn parse_iter_id_letter_suffix() {
        assert_eq!(
            parse_iter_id("iter-75b/pre-create-pr-discipline-gate"),
            Some("75b".to_owned())
        );
    }

    #[test]
    fn parse_iter_id_non_iter_branch() {
        assert_eq!(parse_iter_id("main"), None);
        assert_eq!(parse_iter_id("feature/some-thing"), None);
        assert_eq!(parse_iter_id(""), None);
    }

    #[test]
    fn parse_iter_id_requires_slash() {
        // "iter-75b" without a trailing slash should not match.
        assert_eq!(parse_iter_id("iter-75b"), None);
    }

    #[test]
    fn resolve_plan_single_match() {
        let tmp = tempfile::tempdir().unwrap();
        let kb_dir = tmp.path().join("kb").join("iterations");
        std::fs::create_dir_all(&kb_dir).unwrap();
        std::fs::write(
            kb_dir.join("iteration-42-my-slug.md"),
            "---\ntitle: test\n---\n",
        )
        .unwrap();

        let result = resolve_plan("42", tmp.path()).unwrap();
        assert_eq!(
            result.file_name().unwrap(),
            std::ffi::OsStr::new("iteration-42-my-slug.md")
        );
    }

    #[test]
    fn resolve_plan_no_match() {
        let tmp = tempfile::tempdir().unwrap();
        let kb_dir = tmp.path().join("kb").join("iterations");
        std::fs::create_dir_all(&kb_dir).unwrap();

        let err = resolve_plan("99", tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("no plan found"),
            "expected 'no plan found', got: {err}"
        );
    }

    #[test]
    fn resolve_plan_multiple_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let kb_dir = tmp.path().join("kb").join("iterations");
        std::fs::create_dir_all(&kb_dir).unwrap();
        std::fs::write(kb_dir.join("iteration-7-first.md"), "---\ntitle: a\n---\n").unwrap();
        std::fs::write(kb_dir.join("iteration-7-second.md"), "---\ntitle: b\n---\n").unwrap();

        let err = resolve_plan("7", tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("multiple plans"),
            "expected 'multiple plans', got: {err}"
        );
    }
}
