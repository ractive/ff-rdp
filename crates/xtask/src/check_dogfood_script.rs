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

/// Environment variable through which the gate tells the dogfood script where to
/// write its sentinel. Set fresh for every invocation (iter-184); scripts must
/// read it rather than hardcoding a path. `cfg(unix)` because the whole
/// script-execution path is: the non-unix `run_script` returns SKIP.
#[cfg(unix)]
const SENTINEL_ENV: &str = "FF_RDP_DOGFOOD_SENTINEL";

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
    // The sentinel path is chosen fresh for this invocation and handed to the
    // script through `FF_RDP_DOGFOOD_SENTINEL`.
    //
    // Until iter-184 it was `/tmp/ff-rdp-iter-<N>-dogfood-ok`, derived from the
    // iteration number alone, which made it shared state between every gate run
    // for the same iteration on the machine:
    //   * two concurrent runs raced — one run's "remove the stale sentinel"
    //     pre-clean deleted the file the other had just written, and that run
    //     then reported FAIL for a script that had succeeded;
    //   * a sentinel left behind by a crashed or killed earlier run satisfied a
    //     later run whose script never wrote one — a false PASS in the exact
    //     gate whose only job is to prove the script really executed.
    // A per-run directory removes both: no other run can see this path, and a
    // directory created moments ago cannot hold a stale file.
    let run_dir = tempfile::Builder::new()
        .prefix(&format!("ff-rdp-iter-{iter_num}-dogfood-"))
        .tempdir()
        .context("failed to create per-run sentinel directory")?;
    let sentinel = run_dir.path().join("dogfood-ok");

    // Assert rather than pre-clean. "Nothing to clean" is the property that
    // makes a false PASS impossible; deleting a file here would mean the path
    // was not private after all.
    if sentinel.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (per-run sentinel {:?} already exists — \
             the run directory is not private)",
            sentinel
        );
    }

    // Run the script with bash.  Pass the script path as an OsStr to avoid
    // lossy UTF-8 conversion on platforms where paths can be non-UTF-8.
    let status = std::process::Command::new("bash")
        .arg("-euo")
        .arg("pipefail")
        .arg(script_path)
        .env(SENTINEL_ENV, &sentinel)
        .status()
        .with_context(|| format!("failed to invoke bash for {:?}", script_path))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        anyhow::bail!("check-dogfood-script: FAIL (script exited with code {code})");
    }

    if !sentinel.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (script succeeded but wrote no sentinel at \
             ${SENTINEL_ENV}={}; a script still writing the pre-iter-184 fixed path \
             /tmp/ff-rdp-iter-<N>-dogfood-ok must be migrated to \
             SENTINEL=\"${{{SENTINEL_ENV}:?...}}\")",
            sentinel.display()
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

    /// A lint-clean dogfood script body that writes the sentinel the gate
    /// hands it. `tail` is appended verbatim, so a caller can undo the write.
    fn sentinel_script_body(tail: &str) -> String {
        format!(
            "set -euo pipefail\n\
             SENTINEL=\"${{FF_RDP_DOGFOOD_SENTINEL:?not set}}\"\n\
             rm -f \"$SENTINEL\"\n\
             date -u > \"$SENTINEL\"\n\
             {tail}\n"
        )
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_smoke() {
        // Happy path: script exits 0 AND writes the sentinel the gate named in
        // FF_RDP_DOGFOOD_SENTINEL.
        //
        // Before iter-184 this test had to derive its iteration number from the
        // pid, because `run_script` computed the sentinel path from the number
        // alone — one fixed `/tmp` path shared by every concurrent
        // `cargo test -p xtask` on the machine, where one run's stale-sentinel
        // pre-clean deleted the file another had just written. The gate now picks
        // a private path per run, so a fixed number here is safe again.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-99-smoke.md",
            "dogfood_script: smoke.dogfood.sh\n",
        );
        write_script(&dir, "smoke.dogfood.sh", &sentinel_script_body(""));

        let result = run_inner(&plan_path, true);
        assert!(result.is_ok(), "expected success, got: {result:?}");
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_concurrent_runs_do_not_collide() {
        // AC 1: concurrent gate runs for the *same* iteration must all pass.
        // Pre-iter-184 they shared `/tmp/ff-rdp-iter-99-dogfood-ok`, so each
        // run's pre-clean could delete a sibling's freshly written sentinel and
        // that sibling then failed its existence check.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-99-concurrent.md",
            "dogfood_script: concurrent.dogfood.sh\n",
        );
        write_script(&dir, "concurrent.dogfood.sh", &sentinel_script_body(""));

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| run_inner(&plan_path, true)))
                .collect();
            for handle in handles {
                let result = handle.join().expect("worker thread panicked");
                assert!(result.is_ok(), "concurrent run failed: {result:?}");
            }
        });
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_stale_sentinel_does_not_pass() {
        // AC 2: a sentinel planted at the pre-iter-184 fixed path — exactly what
        // a crashed earlier run used to leave behind — must not satisfy a run
        // whose script writes nothing. This is the false-PASS direction, the
        // serious one: the gate's only job is to prove the script really ran.
        let stale = PathBuf::from(format!(
            "/tmp/ff-rdp-iter-99-dogfood-ok-stale-{}",
            std::process::id()
        ));
        std::fs::write(&stale, "planted by a previous run").unwrap();

        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-99-stale.md",
            "dogfood_script: stale.dogfood.sh\n",
        );
        // Lint-clean, but it removes the sentinel again before exiting 0, so the
        // gate has nothing but the planted file to go on.
        write_script(
            &dir,
            "stale.dogfood.sh",
            &sentinel_script_body("rm -f \"$SENTINEL\""),
        );

        let result = run_inner(&plan_path, true);
        let _ = std::fs::remove_file(&stale);
        let err = result.expect_err("stale sentinel must not make the gate pass");
        assert!(
            err.to_string().contains("wrote no sentinel"),
            "unexpected failure reason: {err}"
        );
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
