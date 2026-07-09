use anyhow::{Result, anyhow};
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Subprocess-based invocation is used (not direct function calls) so that each
// sub-check's println!/eprintln! output can be cleanly captured. See Design
// notes in the iteration plan for the full rationale.

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    #[arg(long)]
    pub plan: PathBuf,

    /// Git ref to diff against (default: origin/main).
    #[arg(long, default_value = "origin/main")]
    pub base: String,

    /// Sub-check names to skip (repeatable). Used in synthetic-plan
    /// integration tests to bypass `check-discipline-regression` when
    /// the local checkout lacks the full main history required by its
    /// replay baselines. Not meant for production /create-pr use.
    #[arg(long = "skip")]
    pub skip: Vec<String>,
}

struct SubcheckResult {
    name: String,
    passed: bool,
    output: String,
}

/// Invoke a cargo xtask subcommand as a subprocess, capturing combined
/// stdout+stderr.  Returns `(success, combined_output)`.
///
/// We use subprocess invocations rather than direct function calls so that each
/// sub-check's `println!`/`eprintln!` output can be captured cleanly. The
/// build overhead is minimal because the binary is already compiled.
fn run_xtask(subcommand: &str, extra_args: &[&str]) -> (bool, String) {
    // Prefer the already-built xtask binary (current_exe) when we are running
    // as the xtask binary ourselves. Fall back to `cargo run -q -p xtask` when
    // current_exe is unavailable or looks like something else (e.g. a test
    // runner).
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_name = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();

    let output = if exe_name == "xtask" {
        Command::new(&exe).arg(subcommand).args(extra_args).output()
    } else {
        Command::new("cargo")
            .args(["run", "-q", "-p", "xtask", "--"])
            .arg(subcommand)
            .args(extra_args)
            .output()
    };

    match output {
        Ok(o) => {
            let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            (o.status.success(), combined)
        }
        Err(e) => (false, format!("failed to invoke xtask {subcommand}: {e}")),
    }
}

const LINT_DOGFOOD_SCRIPT_PATH: &str = "tools/lint-dogfood-script.sh";

/// Run tools/lint-dogfood-script.sh against the plan's dogfood script (if any).
/// Returns (true, output) on pass or SKIP; (false, output) on FAIL.
fn run_lint_dogfood_script(plan: &Path, repo_root: &Path) -> (bool, String) {
    // Read & parse the plan first so we can SKIP cheaply when there's no dogfood_script.
    let content = match std::fs::read_to_string(plan) {
        Ok(c) => c,
        Err(e) => {
            return (
                false,
                format!("lint-dogfood-script: FAIL (could not read plan: {e})"),
            );
        }
    };
    let parsed = match crate::check_iteration_plan::parse_plan(&content) {
        Ok(p) => p,
        Err(e) => {
            return (
                false,
                format!("lint-dogfood-script: FAIL (could not parse plan: {e})"),
            );
        }
    };

    let script_name = match parsed.frontmatter.dogfood_script.as_deref() {
        None | Some("") => {
            return (
                true,
                "lint-dogfood-script: SKIP (no dogfood_script field in plan)".to_owned(),
            );
        }
        Some(s) => s,
    };

    let linter = repo_root.join(LINT_DOGFOOD_SCRIPT_PATH);
    if !linter.exists() {
        return (
            false,
            format!(
                "lint-dogfood-script: FAIL (linter not found: {})",
                linter.display()
            ),
        );
    }

    let plan_dir = match plan.parent() {
        Some(d) => d,
        None => {
            return (
                false,
                "lint-dogfood-script: FAIL (plan path has no parent dir)".to_owned(),
            );
        }
    };
    let script_path = plan_dir.join(script_name);

    if !script_path.exists() {
        return (
            false,
            format!(
                "lint-dogfood-script: FAIL (script does not exist: {})",
                script_path.display()
            ),
        );
    }

    let output = Command::new("bash")
        .arg(&linter)
        .arg(&script_path)
        .current_dir(repo_root)
        .output();

    match output {
        Ok(o) => {
            let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            (o.status.success(), combined)
        }
        Err(e) => (
            false,
            format!("lint-dogfood-script: FAIL (bash invocation error: {e})"),
        ),
    }
}

/// Run check-pre-fix-repro as a subprocess sub-check.
fn run_check_pre_fix_repro(plan: &Path) -> (bool, String) {
    let plan_str = plan.to_string_lossy();
    run_xtask("check-pre-fix-repro", &["--plan", plan_str.as_ref()])
}

/// Locate the in-repo ac-fidelity-check.sh mirror, falling back to the
/// canonical skill path.
fn find_ac_fidelity_script(repo_root: &Path) -> Option<PathBuf> {
    let mirror = repo_root.join("tools/ralph-loop/scripts/ac-fidelity-check.sh");
    if mirror.exists() {
        return Some(mirror);
    }
    // Fall back to canonical skill dir.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let canonical =
        PathBuf::from(home).join(".claude/skills/ralph-loop/scripts/ac-fidelity-check.sh");
    if canonical.exists() {
        Some(canonical)
    } else {
        None
    }
}

/// Run ac-fidelity-check.sh and capture output.
fn run_ac_fidelity(plan: &Path, base: &str, repo_root: &Path) -> (bool, String) {
    let Some(script) = find_ac_fidelity_script(repo_root) else {
        return (
            false,
            "ac-fidelity-check.sh not found in tools/ralph-loop/scripts/ \
             or ~/.claude/skills/ralph-loop/scripts/"
                .to_owned(),
        );
    };

    let plan_str = plan.to_string_lossy();
    let output = Command::new("bash")
        .arg(&script)
        .arg("--plan")
        .arg(plan_str.as_ref())
        .arg("--base")
        .arg(base)
        .current_dir(repo_root)
        .output();

    match output {
        Ok(o) => {
            let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            (o.status.success(), combined)
        }
        Err(e) => (false, format!("failed to run ac-fidelity-check.sh: {e}")),
    }
}

fn locate_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| anyhow!("running git rev-parse: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!("git rev-parse --show-toplevel failed"));
    }
    let s = String::from_utf8(output.stdout).map_err(|e| anyhow!("non-utf8 git output: {e}"))?;
    Ok(PathBuf::from(s.trim()))
}

/// Print the sub-check result line. On failure, indent each line of the output.
fn print_result(index: usize, total: usize, result: &SubcheckResult) {
    let status = if result.passed { "PASS" } else { "FAIL" };
    println!("[{index}/{total}] {}: {status}", result.name);
    if !result.passed && !result.output.is_empty() {
        for line in result.output.lines() {
            println!("    {line}");
        }
    }
}

pub fn run(args: Args) -> Result<()> {
    let repo_root = locate_repo_root()?;
    let plan = args.plan.clone();
    let base = args.base.clone();
    let skip: std::collections::HashSet<String> = args.skip.into_iter().collect();

    // Resolve plan path for display and shell invocations.
    let plan_display = plan.to_string_lossy().into_owned();
    let base_str = base.as_str();

    let mut results: Vec<SubcheckResult> = Vec::new();

    let run_or_skip = |name: &str, runner: &mut dyn FnMut() -> (bool, String)| -> SubcheckResult {
        if skip.contains(name) {
            SubcheckResult {
                name: name.to_owned(),
                passed: true,
                output: format!("(skipped via --skip {name})"),
            }
        } else {
            let (passed, output) = runner();
            SubcheckResult {
                name: name.to_owned(),
                passed,
                output,
            }
        }
    };

    // --- 1. check-dead-primitives
    results.push(run_or_skip("check-dead-primitives", &mut || {
        run_xtask("check-dead-primitives", &["--since", base_str])
    }));

    // --- 2. check-pre-fix-repro (iter-87)
    results.push(run_or_skip("check-pre-fix-repro", &mut || {
        run_check_pre_fix_repro(&plan)
    }));

    // --- 3. lint-dogfood-script (iter-87)
    results.push(run_or_skip("lint-dogfood-script", &mut || {
        run_lint_dogfood_script(&plan, &repo_root)
    }));

    // --- 4. check-todo-annotations
    results.push(run_or_skip("check-todo-annotations", &mut || {
        run_xtask("check-todo-annotations", &["--since", base_str])
    }));

    // --- 5. check-actor-kb-sync
    results.push(run_or_skip("check-actor-kb-sync", &mut || {
        run_xtask("check-actor-kb-sync", &["--since", base_str])
    }));

    // --- 6. check-firefox-refs
    results.push(run_or_skip("check-firefox-refs", &mut || {
        run_xtask("check-firefox-refs", &[&plan_display])
    }));

    // --- 7. check-discipline-regression
    results.push(run_or_skip("check-discipline-regression", &mut || {
        run_xtask("check-discipline-regression", &[])
    }));

    // --- 8. ac-fidelity-check.sh
    results.push(run_or_skip("ac-fidelity-check", &mut || {
        run_ac_fidelity(&plan, base_str, &repo_root)
    }));

    // --- 9. check-dogfood-script
    results.push(run_or_skip("check-dogfood-script", &mut || {
        run_xtask("check-dogfood-script", &[&plan_display])
    }));

    // --- 10. check-live-test-layout (iter-100b)
    results.push(run_or_skip("check-live-test-layout", &mut || {
        run_xtask("check-live-test-layout", &[])
    }));

    let total = results.len();
    for (i, result) in results.iter().enumerate() {
        print_result(i + 1, total, result);
    }

    let pass_count = results.iter().filter(|r| r.passed).count();
    let fail_count = total - pass_count;

    if fail_count == 0 {
        println!("check-iteration-ready: {pass_count}/{total} PASS");
        Ok(())
    } else {
        println!(
            "check-iteration-ready: {fail_count} sub-check(s) FAILED — fix above issues before /create-pr"
        );
        Err(anyhow!("{fail_count} sub-check(s) failed"))
    }
}

#[cfg(test)]
mod tests {
    /// Verify that the check-dogfood-script sub-check is included in the results
    /// produced by run(). We use `--skip` for all other gates and a synthetic
    /// plan so this test doesn't require a full repo checkout or Firefox binary.
    #[test]
    fn xtask_check_iteration_ready_calls_dogfood_script() {
        use std::io::Write as _;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        // Write a minimal plan with dogfood_path (no dogfood_script) so the
        // check-dogfood-script sub-check skips cleanly (SKIP = pass).
        let plan_path = dir.path().join("iteration-96-test.md");
        let content = "---\ntitle: \"Test\"\nstatus: planned\ntype: iteration\ndogfood_path: \"ff-rdp --help\"\n---\n\n# Body\n";
        {
            let mut f = std::fs::File::create(&plan_path).unwrap();
            write!(f, "{content}").unwrap();
        }

        // Run check-iteration-ready via cargo run (current_exe is the test runner, not xtask).
        let output = std::process::Command::new("cargo")
            .args(["run", "-q", "-p", "xtask", "--"])
            .args([
                "check-iteration-ready",
                "--plan",
                plan_path.to_str().unwrap(),
                "--base",
                "HEAD",
                "--skip",
                "check-dead-primitives",
                "--skip",
                "check-todo-annotations",
                "--skip",
                "check-actor-kb-sync",
                "--skip",
                "check-firefox-refs",
                "--skip",
                "check-discipline-regression",
                "--skip",
                "ac-fidelity-check",
                "--skip",
                "check-live-test-layout",
            ])
            .output()
            .unwrap();

        let combined = {
            let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&output.stderr));
            s
        };

        // The sub-check name must appear in output.
        assert!(
            combined.contains("check-dogfood-script"),
            "check-dogfood-script sub-check name missing from output:\n{combined}"
        );
    }
}
