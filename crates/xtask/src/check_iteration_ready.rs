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

    // --- 2. check-todo-annotations
    results.push(run_or_skip("check-todo-annotations", &mut || {
        run_xtask("check-todo-annotations", &["--since", base_str])
    }));

    // --- 3. check-actor-kb-sync
    results.push(run_or_skip("check-actor-kb-sync", &mut || {
        run_xtask("check-actor-kb-sync", &["--since", base_str])
    }));

    // --- 4. check-firefox-refs
    results.push(run_or_skip("check-firefox-refs", &mut || {
        run_xtask("check-firefox-refs", &[&plan_display])
    }));

    // --- 5. check-discipline-regression
    results.push(run_or_skip("check-discipline-regression", &mut || {
        run_xtask("check-discipline-regression", &[])
    }));

    // --- 6. ac-fidelity-check.sh
    results.push(run_or_skip("ac-fidelity-check", &mut || {
        run_ac_fidelity(&plan, base_str, &repo_root)
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
