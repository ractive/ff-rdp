use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    pub plan: Option<PathBuf>,

    /// Path to the iteration plan markdown file (named form of the positional
    /// argument, so `--plan <path>` and `<path>` are interchangeable).
    #[arg(long = "plan", value_name = "PATH", conflicts_with = "plan")]
    pub plan_flag: Option<PathBuf>,
}

impl Args {
    /// Resolve the plan path from either the positional or the `--plan` form.
    fn plan_path(&self) -> Result<&Path> {
        self.plan_flag
            .as_deref()
            .or(self.plan.as_deref())
            .context("a plan path is required (positional or --plan <PATH>)")
    }
}

/// The ff-rdp-specific `.dogfood.sh` linter, rehosted here in iter-162a when
/// `check-iteration-ready` — its only non-test caller — was deleted.
const LINT_DOGFOOD_SCRIPT_PATH: &str = "tools/lint-dogfood-script.sh";

/// Result of the `lint-dogfood-script` sub-check.
enum LintOutcome {
    Pass(String),
    Skip(String),
    Fail(String),
}

/// Extract the iteration number from a plan filename like `iteration-85-slug.md`.
fn extract_iteration_number(plan: &std::path::Path) -> Option<u32> {
    let stem = plan.file_stem()?.to_str()?;
    // Expected prefix: "iteration-N-"
    let rest = stem.strip_prefix("iteration-")?;
    let end = rest.find('-').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Detect the current git branch name.
///
/// Priority (highest first):
/// 1. `FF_RDP_CURRENT_BRANCH` env var — allows tests to override branch detection.
/// 2. `BRANCH_NAME` env var — set by CI (GitHub Actions, Jenkins, etc.).
/// 3. `git rev-parse --abbrev-ref HEAD`.
///
/// Returns `None` if detection fails in all three paths.
fn detect_current_branch() -> Option<String> {
    // Test override.
    if let Ok(b) = std::env::var("FF_RDP_CURRENT_BRANCH")
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_owned());
    }
    // CI env var.
    if let Ok(b) = std::env::var("BRANCH_NAME")
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_owned());
    }
    // Git command.
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !branch.is_empty() && branch != "HEAD" {
            return Some(branch);
        }
    }
    None
}

/// Returns `true` if `branch` looks like an iter-* branch.
fn is_iter_branch(branch: &str) -> bool {
    branch.starts_with("iter-")
}

/// Run `tools/lint-dogfood-script.sh` against the plan's dogfood script.
///
/// Skips cheaply when the plan carries no `dogfood_script` field. This is a
/// static lint — it never executes the script — so it runs regardless of
/// `FF_RDP_LIVE_TESTS`, which is the whole point of hosting it here: it is the
/// one gate in the repo with a track record of stopping a real false-green
/// (iter-86's `grep -qi 'headless'`).
///
/// Non-unix skips, matching `run_script` below. `bash` is not guaranteed on
/// Windows: `Command::new("bash")` there resolves to `C:\Windows\System32\bash.exe`,
/// the WSL shim, which exits non-zero with *"Windows Subsystem for Linux has no
/// installed distributions"* — a hard FAIL for every plan on the Windows runner.
/// `cfg!` rather than `#[cfg]` so every `LintOutcome` variant stays constructible
/// on all platforms.
fn lint_dogfood_script(plan: &Path, repo_root: &Path) -> LintOutcome {
    if !cfg!(unix) {
        return LintOutcome::Skip("bash invocation not supported on this platform".to_owned());
    }

    let content = match std::fs::read_to_string(plan) {
        Ok(c) => c,
        Err(e) => return LintOutcome::Fail(format!("could not read plan: {e}")),
    };
    let parsed = match crate::check_iteration_plan::parse_plan(&content) {
        Ok(p) => p,
        Err(e) => return LintOutcome::Fail(format!("could not parse plan: {e}")),
    };

    let script_name = match parsed.frontmatter.dogfood_script.as_deref() {
        None | Some("") => {
            return LintOutcome::Skip("no dogfood_script field in plan".to_owned());
        }
        Some(s) => s,
    };

    let linter = repo_root.join(LINT_DOGFOOD_SCRIPT_PATH);
    if !linter.exists() {
        return LintOutcome::Fail(format!("linter not found: {}", linter.display()));
    }

    let Some(plan_dir) = plan.parent() else {
        return LintOutcome::Fail("plan path has no parent dir".to_owned());
    };
    let script_path = plan_dir.join(script_name);
    if !script_path.exists() {
        return LintOutcome::Fail(format!("script does not exist: {}", script_path.display()));
    }

    match Command::new("bash")
        .arg(&linter)
        .arg(&script_path)
        .current_dir(repo_root)
        .output()
    {
        Ok(o) => {
            let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            let combined = combined.trim_end().to_owned();
            if o.status.success() {
                LintOutcome::Pass(combined)
            } else {
                LintOutcome::Fail(combined)
            }
        }
        Err(e) => LintOutcome::Fail(format!("bash invocation error: {e}")),
    }
}

/// Print the lint result and turn a failure into an error.
fn report_lint(outcome: LintOutcome) -> Result<()> {
    let (label, detail, ok) = match outcome {
        LintOutcome::Pass(d) => ("PASS", d, true),
        LintOutcome::Skip(d) => ("SKIP", d, true),
        LintOutcome::Fail(d) => ("FAIL", d, false),
    };
    println!("lint-dogfood-script: {label}");
    for line in detail.lines() {
        println!("    {line}");
    }
    if ok {
        Ok(())
    } else {
        anyhow::bail!("lint-dogfood-script: FAIL");
    }
}

pub fn run(args: Args) -> Result<()> {
    let plan = args.plan_path()?;
    let repo_root = crate::stderr_scan::locate_repo_root()?;
    report_lint(lint_dogfood_script(plan, &repo_root))?;
    run_inner(plan, false)
}

/// Inner implementation. When `force` is true the `FF_RDP_LIVE_TESTS` guard is
/// bypassed — used by unit tests to avoid depending on the environment.
pub fn run_inner(plan: &Path, force: bool) -> Result<()> {
    if !force && std::env::var("FF_RDP_LIVE_TESTS").as_deref() != Ok("1") {
        // On iter-* branches the skip is replaced by a hard FAIL so the gate
        // cannot be silently bypassed. On other branches (main, release, etc.)
        // the original SKIP behaviour is preserved.
        if let Some(branch) = detect_current_branch()
            && is_iter_branch(&branch)
        {
            anyhow::bail!(
                "check-dogfood-script: FAIL — iter-* branch requires \
                 FF_RDP_LIVE_TESTS=1 to verify dogfood script. \
                 Re-run with FF_RDP_LIVE_TESTS=1 to execute the dogfood gate."
            );
        }
        println!("check-dogfood-script: SKIP (FF_RDP_LIVE_TESTS not set)");
        return Ok(());
    }

    // Parse frontmatter to find dogfood_script.
    let content =
        std::fs::read_to_string(plan).with_context(|| format!("failed to read {:?}", plan))?;

    let parsed = crate::check_iteration_plan::parse_plan(&content)
        .with_context(|| format!("failed to parse plan {:?}", plan))?;

    let script_name = match parsed.frontmatter.dogfood_script.as_deref() {
        None | Some("") => {
            println!("check-dogfood-script: SKIP (no dogfood_script field in plan)");
            return Ok(());
        }
        Some(s) => s,
    };

    // Resolve the script path relative to the plan's parent directory.
    let plan_dir = plan
        .parent()
        .with_context(|| format!("plan path has no parent dir: {:?}", plan))?;
    let script_path = plan_dir.join(script_name);

    if !script_path.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (script does not exist: {:?})",
            script_path
        );
    }

    // Extract iteration number to determine the sentinel path.
    let iter_num = extract_iteration_number(plan).with_context(|| {
        format!("could not extract iteration number from plan filename: {plan:?}")
    })?;

    run_script(&script_path, iter_num)
}

#[cfg(unix)]
fn run_script(script_path: &std::path::Path, iter_num: u32) -> Result<()> {
    let sentinel = PathBuf::from(format!("/tmp/ff-rdp-iter-{iter_num}-dogfood-ok"));

    // Pre-clean: remove any stale sentinel.
    if sentinel.exists() {
        std::fs::remove_file(&sentinel)
            .with_context(|| format!("failed to remove stale sentinel {:?}", sentinel))?;
    }

    // Run the script with bash.  Pass the script path as an OsStr to avoid
    // lossy UTF-8 conversion on platforms where paths can be non-UTF-8.
    let status = std::process::Command::new("bash")
        .arg("-euo")
        .arg("pipefail")
        .arg(script_path)
        .status()
        .with_context(|| format!("failed to invoke bash for {:?}", script_path))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        anyhow::bail!("check-dogfood-script: FAIL (script exited with code {code})");
    }

    if !sentinel.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (missing sentinel {:?} after script succeeded)",
            sentinel
        );
    }

    println!("check-dogfood-script: OK");
    Ok(())
}

#[cfg(not(unix))]
fn run_script(_script_path: &std::path::Path, _iter_num: u32) -> Result<()> {
    // bash is not guaranteed on Windows; the CI gate runs on ubuntu-latest only.
    println!("check-dogfood-script: SKIP (bash invocation not supported on this platform)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    /// Write a minimal plan file with the given extra frontmatter into `dir`.
    fn write_plan(dir: &TempDir, name: &str, extra_fm: &str) -> PathBuf {
        let path = dir.path().join(name);
        let content = format!(
            "---\ntitle: \"Test Plan\"\nstatus: planned\ntype: iteration\n{extra_fm}---\n\n# Body\n"
        );
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Write an executable shell script into `dir`.
    fn write_script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/usr/bin/env bash").unwrap();
        writeln!(f, "{body}").unwrap();
        // Mark executable on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_smoke() {
        // Happy path: script exits 0 AND writes the sentinel.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-99-smoke.md",
            "dogfood_script: smoke.dogfood.sh\n",
        );
        write_script(
            &dir,
            "smoke.dogfood.sh",
            "touch /tmp/ff-rdp-iter-99-dogfood-ok",
        );

        // Pre-clean sentinel in case a prior run left it.
        let _ = std::fs::remove_file("/tmp/ff-rdp-iter-99-dogfood-ok");

        let result = run_inner(&plan_path, true);
        assert!(result.is_ok(), "expected success, got: {result:?}");
        assert!(
            std::path::Path::new("/tmp/ff-rdp-iter-99-dogfood-ok").exists(),
            "sentinel should exist after successful run"
        );
        // Clean up.
        let _ = std::fs::remove_file("/tmp/ff-rdp-iter-99-dogfood-ok");
    }

    #[test]
    fn xtask_check_dogfood_script_no_field_skip() {
        // Plan with no dogfood_script field → SKIP, exit 0.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-97-no-script.md",
            "dogfood_path: \"ff-rdp --help\"\n",
        );

        let result = run_inner(&plan_path, true);
        assert!(result.is_ok(), "expected SKIP success: {result:?}");
    }

    /// Copy an in-repo lint fixture next to a synthetic plan in `dir`.
    fn stage_fixture(dir: &TempDir, fixture: &str) -> PathBuf {
        let repo_root = crate::stderr_scan::locate_repo_root().unwrap();
        let src = repo_root
            .join("tools/tests/lint-dogfood-script")
            .join(fixture);
        let dst = dir.path().join(fixture);
        std::fs::copy(&src, &dst).unwrap();
        dst
    }

    #[test]
    fn lint_dogfood_script_skips_without_field() {
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(&dir, "iteration-93-no-script.md", "");
        let repo_root = crate::stderr_scan::locate_repo_root().unwrap();
        assert!(matches!(
            lint_dogfood_script(&plan_path, &repo_root),
            LintOutcome::Skip(_)
        ));
    }

    #[test]
    #[cfg(unix)]
    fn lint_dogfood_script_passes_on_clean_script() {
        // The rehosted linter must still be reachable and still pass a script
        // that satisfies every rule.
        let dir = TempDir::new().unwrap();
        stage_fixture(&dir, "all-rules-good.sh");
        let plan_path = write_plan(
            &dir,
            "iteration-92-clean.md",
            "dogfood_script: all-rules-good.sh\n",
        );
        let repo_root = crate::stderr_scan::locate_repo_root().unwrap();
        let outcome = lint_dogfood_script(&plan_path, &repo_root);
        assert!(
            matches!(outcome, LintOutcome::Pass(_)),
            "clean fixture must lint clean"
        );
    }

    #[test]
    #[cfg(unix)]
    fn lint_dogfood_script_fails_on_dirty_script() {
        // The catch that justified keeping this gate: a rule violation must
        // reach the caller as a hard failure, not a silent pass.
        let dir = TempDir::new().unwrap();
        stage_fixture(&dir, "missing-set-euo-bad.sh");
        let plan_path = write_plan(
            &dir,
            "iteration-91-dirty.md",
            "dogfood_script: missing-set-euo-bad.sh\n",
        );
        let repo_root = crate::stderr_scan::locate_repo_root().unwrap();
        let outcome = lint_dogfood_script(&plan_path, &repo_root);
        let LintOutcome::Fail(detail) = outcome else {
            panic!("dirty fixture must fail the lint");
        };
        assert!(
            detail.contains("missing-set-euo-pipefail"),
            "failure must name the rule that fired:\n{detail}"
        );
        assert!(report_lint(LintOutcome::Fail(detail)).is_err());
    }

    #[test]
    fn xtask_extract_iteration_number() {
        let p = std::path::Path::new("iteration-85-dogfood-57-carryovers.md");
        assert_eq!(extract_iteration_number(p), Some(85));

        let p2 = std::path::Path::new("iteration-1-foo.md");
        assert_eq!(extract_iteration_number(p2), Some(1));

        let p3 = std::path::Path::new("not-an-iteration.md");
        assert_eq!(extract_iteration_number(p3), None);
    }
}
