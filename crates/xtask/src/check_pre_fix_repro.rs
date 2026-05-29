//! `xtask check-pre-fix-repro`
//!
//! Parses `pre_fix_repro_test:` annotations from iteration plan theme headings
//! and verifies that each named test:
//!
//!   1. Exists (via `cargo test --list`).
//!   2. FAILs on `origin/main` (the bug is reproducible before the fix).
//!   3. PASSes on the current branch HEAD (the fix works).
//!
//! Iterations with no `pre_fix_repro_test:` annotations are silently skipped.
//!
//! The check uses `git stash --include-untracked` before switching to
//! `origin/main` and restores via a `Drop` guard to ensure cleanup even on
//! panic. It is wired into `check-iteration-ready` after `check-dead-primitives`
//! and before `check-dogfood-script`.

use anyhow::{Context, Result, anyhow};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    #[arg(long)]
    pub plan: PathBuf,

    /// Crate to search for the pre-fix repro tests (default: whole workspace).
    #[arg(long)]
    pub crate_name: Option<String>,
}

/// A parsed `pre_fix_repro_test:` annotation.
#[derive(Debug, PartialEq, Eq)]
pub struct PreFixAnnotation {
    /// The theme title (everything after `### ` and before ` [pre_fix_repro_test:...]`).
    pub theme: String,
    /// The test slug named in the annotation.
    pub test_slug: String,
}

/// Parse `pre_fix_repro_test: <slug>` annotations from theme headings in the
/// plan body.
///
/// Recognised format:
/// ```text
/// ### Theme A — Some title [pre_fix_repro_test: my_test_slug]
/// ```
///
/// Returns a `Vec<PreFixAnnotation>`. Themes without the annotation are
/// silently skipped. Content inside fenced code blocks is ignored.
pub fn parse_pre_fix_repro_annotations(body: &str) -> Vec<PreFixAnnotation> {
    let mut annotations = Vec::new();
    let mut in_code_block = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks (``` or ~~~) — skip their contents.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Only consider ### headings (theme level)
        if !trimmed.starts_with("### ") {
            continue;
        }

        // Look for `[pre_fix_repro_test: <slug>]`
        let Some(bracket_start) = trimmed.find("[pre_fix_repro_test:") else {
            continue;
        };
        let Some(bracket_end) = trimmed[bracket_start..].find(']') else {
            continue;
        };

        let annotation_body = &trimmed[bracket_start + 1..bracket_start + bracket_end];
        // annotation_body is: "pre_fix_repro_test: <slug>"
        let Some(colon_pos) = annotation_body.find(':') else {
            continue;
        };
        let slug = annotation_body[colon_pos + 1..].trim();
        if slug.is_empty() {
            continue;
        }

        // Extract theme label — everything from "### " up to the bracket annotation.
        let heading_text = trimmed.trim_start_matches('#').trim();
        let theme_text = if let Some(bracket_pos) = heading_text.find(" [pre_fix_repro_test:") {
            heading_text[..bracket_pos].trim()
        } else {
            heading_text
        };

        annotations.push(PreFixAnnotation {
            theme: theme_text.to_owned(),
            test_slug: slug.to_owned(),
        });
    }

    annotations
}

/// Resolve a bare test slug to its fully-qualified test path (e.g.
/// `module::path::slug`) by searching `cargo test -- --list` output.
///
/// When no crate_name is specified, tries `-p xtask` first (where most
/// discipline tests live) then falls back to searching the full workspace.
fn resolve_test_path(slug: &str, crate_name: Option<&str>) -> Result<String> {
    let crates_to_try: Vec<Option<&str>> = if crate_name.is_some() {
        vec![crate_name]
    } else {
        vec![Some("xtask"), None]
    };

    for try_crate in &crates_to_try {
        if let Some(path) = try_list_tests(slug, *try_crate)? {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "test slug '{slug}' not found in `cargo test -- --list` output.\n\
         Make sure the test exists and is compiled."
    ))
}

/// Returns `Ok(Some(full_path))` if `slug` matches an entry in the test listing
/// for the given crate. Surfaces compile/invocation failures rather than
/// silently returning "not found".
fn try_list_tests(slug: &str, crate_name: Option<&str>) -> Result<Option<String>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if let Some(name) = crate_name {
        cmd.args(["-p", name]);
    }
    cmd.args(["--", "--list"]);

    let output = cmd
        .output()
        .context("failed to invoke `cargo test -- --list`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`cargo test{} -- --list` failed (exit {}): {stderr}",
            crate_name.map(|c| format!(" -p {c}")).unwrap_or_default(),
            output.status.code().unwrap_or(-1)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = stdout
        .lines()
        .filter(|l| l.ends_with(": test") || l.ends_with(": bench"))
        .find_map(|l| {
            let name = l.trim_end_matches(": test").trim_end_matches(": bench");
            if name == slug
                || name.ends_with(&format!("::{slug}"))
                || name.ends_with(&format!("/{slug}"))
            {
                Some(name.to_owned())
            } else {
                None
            }
        });
    Ok(found)
}

/// Run a single named test by its fully-qualified path and return whether it passed.
fn run_test(full_path: &str, crate_name: Option<&str>) -> Result<bool> {
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if let Some(name) = crate_name {
        cmd.args(["-p", name]);
    }
    cmd.args(["-q", "--", full_path, "--exact"]);

    let status = cmd.status().context("failed to invoke `cargo test`")?;

    Ok(status.success())
}

/// A RAII guard that pops a git stash on drop.
struct StashGuard {
    stashed: bool,
}

impl StashGuard {
    /// Stash current changes. Returns a guard that will pop on drop.
    fn stash() -> Result<Self> {
        let output = Command::new("git")
            .args(["stash", "--include-untracked"])
            .output()
            .context("failed to invoke `git stash`")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git stash failed: {stderr}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // "No local changes to save" means nothing was stashed — no pop needed.
        let stashed = !stdout.contains("No local changes to save");
        Ok(StashGuard { stashed })
    }
}

impl Drop for StashGuard {
    fn drop(&mut self) {
        if !self.stashed {
            return;
        }
        // Best-effort restore — ignore errors in Drop.
        let _ = Command::new("git").args(["stash", "pop"]).status();
    }
}

/// Checkout a git ref. Returns a guard that checks out the previous ref when dropped.
struct CheckoutGuard {
    previous_ref: String,
}

impl CheckoutGuard {
    fn checkout(git_ref: &str) -> Result<Self> {
        // Save the current branch/ref.
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .context("failed to get current HEAD ref")?;
        let mut previous_ref = String::from_utf8(output.stdout)
            .context("non-utf8 git output")?
            .trim()
            .to_owned();

        // Detached HEAD returns literal "HEAD" — capture the SHA instead.
        if previous_ref == "HEAD" {
            let sha_output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .context("failed to get HEAD SHA")?;
            previous_ref = String::from_utf8(sha_output.stdout)
                .context("non-utf8 git output")?
                .trim()
                .to_owned();
        }

        let status = Command::new("git")
            .args(["checkout", git_ref])
            .status()
            .context("failed to invoke `git checkout`")?;

        if !status.success() {
            return Err(anyhow!("git checkout {git_ref} failed"));
        }

        Ok(CheckoutGuard { previous_ref })
    }
}

impl Drop for CheckoutGuard {
    fn drop(&mut self) {
        // Best-effort restore — ignore errors in Drop.
        let _ = Command::new("git")
            .args(["checkout", &self.previous_ref])
            .status();
    }
}

pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("failed to read {:?}", args.plan))?;

    let plan = crate::check_iteration_plan::parse_plan(&content)
        .with_context(|| format!("failed to parse plan {:?}", args.plan))?;

    let annotations = parse_pre_fix_repro_annotations(&plan.body);

    if annotations.is_empty() {
        println!("check-pre-fix-repro: SKIP (no pre_fix_repro_test annotations in plan)");
        return Ok(());
    }

    let crate_name = args.crate_name.as_deref();
    let mut failures: Vec<String> = Vec::new();

    for annotation in &annotations {
        let slug = &annotation.test_slug;
        println!(
            "check-pre-fix-repro: checking '{}' (theme: {})",
            slug, annotation.theme
        );

        // Step 1: resolve the slug to a fully-qualified test path.
        let full_path = match resolve_test_path(slug, crate_name) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("  [{slug}] step 1 (resolve): {e}"));
                continue;
            }
        };

        // Step 2: stash current changes.
        let stash_guard = match StashGuard::stash() {
            Ok(g) => g,
            Err(e) => {
                failures.push(format!("  [{slug}] step 2 (stash): {e}"));
                continue;
            }
        };

        // Step 3: checkout origin/main, run test, expect FAIL.
        let checkout_result = CheckoutGuard::checkout("origin/main");
        match checkout_result {
            Err(e) => {
                failures.push(format!("  [{slug}] step 3 (checkout origin/main): {e}"));
                drop(stash_guard);
                continue;
            }
            Ok(checkout_guard) => {
                // Treat invocation errors as "test did not pass" = expected FAIL.
                let main_passed = run_test(&full_path, crate_name).unwrap_or(false);

                if main_passed {
                    failures.push(format!(
                        "  [{slug}] step 3: test PASSED on origin/main — expected FAIL. \
                         The pre-fix repro test must be red on main before the fix lands."
                    ));
                    drop(checkout_guard);
                    drop(stash_guard);
                    continue;
                }

                // Test correctly fails on main. Restore branch.
                drop(checkout_guard);

                // Step 4: restore stash, run test, expect PASS.
                drop(stash_guard);

                let branch_passed = run_test(&full_path, crate_name)
                    .context("failed to run test on branch HEAD")?;

                if !branch_passed {
                    failures.push(format!(
                        "  [{slug}] step 4: test FAILED on branch HEAD — expected PASS. \
                         The fix must make the pre-fix repro test green."
                    ));
                } else {
                    println!("  [{slug}] OK — red on main, green on branch HEAD");
                }
            }
        }
    }

    if failures.is_empty() {
        println!(
            "check-pre-fix-repro: OK ({} annotation(s) verified)",
            annotations.len()
        );
        Ok(())
    } else {
        for f in &failures {
            eprintln!("{f}");
        }
        Err(anyhow!(
            "check-pre-fix-repro: {} annotation(s) failed verification",
            failures.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pre_fix_repro_annotations_basic() {
        let body = r#"
## Some section

### Theme A — cascade fix [pre_fix_repro_test: pre_fix_cascade_red_then_green]

Some content.

### Theme B — no annotation

More content.

### Theme C — another fix [pre_fix_repro_test: pre_fix_c_slug]

Content.
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].test_slug, "pre_fix_cascade_red_then_green");
        assert!(annotations[0].theme.contains("Theme A"));
        assert_eq!(annotations[1].test_slug, "pre_fix_c_slug");
        assert!(annotations[1].theme.contains("Theme C"));
    }

    #[test]
    fn parse_pre_fix_repro_annotations_no_annotations() {
        let body = r#"
### Theme A — some theme [0/3]
### Theme B — other theme [1/2]
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_empty_slug() {
        // A malformed annotation with empty slug is ignored.
        let body = "### Theme A — test [pre_fix_repro_test:]\n";
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_whitespace_slug() {
        // Whitespace-only slug is ignored.
        let body = "### Theme A — test [pre_fix_repro_test:   ]\n";
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_skips_code_block() {
        // Annotations inside fenced code blocks must be ignored.
        let body = r#"
Some prose.

```
### Theme X — example [pre_fix_repro_test: should_be_ignored]
```

### Theme Y — real [pre_fix_repro_test: real_slug]
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].test_slug, "real_slug");
    }

    #[test]
    fn parse_pre_fix_repro_annotations_iter87_plan() {
        // Simulate iter-87 plan which has two annotations.
        let body = r#"
### Theme B — check-dogfood-script FAILs by default [0/4] [pre_fix_repro_test: live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch]

Content.

### Theme E — fix iter-86's buggy dogfood-script assertions [0/3] [pre_fix_repro_test: lint_flags_iter86_assertions_before_fix]

Content.
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 2);
        assert_eq!(
            annotations[0].test_slug,
            "live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch"
        );
        assert_eq!(
            annotations[1].test_slug,
            "lint_flags_iter86_assertions_before_fix"
        );
    }

    /// `xtask_check_pre_fix_repro_parses_iter87_annotations`:
    /// Verify the annotation parser handles the iter-87 format correctly and
    /// that a plan with no annotations skips gracefully. The full red→green
    /// round-trip requires a live git repo switch and is exercised manually
    /// — see Theme D notes in the iteration plan.
    #[test]
    fn xtask_check_pre_fix_repro_parses_iter87_annotations() {
        // Test that parse_pre_fix_repro_annotations correctly identifies annotations
        // and that a plan with annotations parses non-empty.
        let body_with_annotation = "### Theme X — fix something [pre_fix_repro_test: my_slug]\n";
        let annots = parse_pre_fix_repro_annotations(body_with_annotation);
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].test_slug, "my_slug");

        // A plan with no annotations produces an empty vec — the SKIP path.
        let body_no_annotation = "### Theme Y — some work [0/3]\n";
        let empty = parse_pre_fix_repro_annotations(body_no_annotation);
        assert!(empty.is_empty(), "expected empty, got: {empty:?}");
    }
}
