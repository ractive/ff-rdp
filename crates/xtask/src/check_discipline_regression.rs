use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the canonical ralph-loop scripts (defaults to
    /// $HOME/.claude/skills/ralph-loop/scripts). If absent, only the mirror's
    /// replay-baseline behaviour is verified.
    #[arg(long)]
    skill_dir: Option<PathBuf>,

    /// Path to the canonical new-ralph-loop scripts (defaults to
    /// $HOME/.claude/skills/new-ralph-loop/scripts). If absent, that mirror is
    /// not checked.
    #[arg(long)]
    new_skill_dir: Option<PathBuf>,

    /// Skip the replay baselines (mirror-sync check only). Useful for CI runs
    /// that don't have access to the merged iter-61t / iter-61v history.
    #[arg(long)]
    skip_replay: bool,
}

/// The scripts that must be mirrored 1-to-1 between the canonical skill
/// directory and the in-repo copy under `tools/ralph-loop/scripts/`.
const MIRROR_FILES: &[&str] = &[
    "claims-vs-code.sh",
    "ac-fidelity-check.sh",
    "run-iteration.sh",
];

/// The same contract for the `new-ralph-loop` skill, mirrored under
/// `tools/new-ralph-loop/scripts/`.
///
/// iter-146: this mirror did not exist until the post-138–142 sweep, and its
/// absence is exactly why a fix to `ac-fidelity-check.sh` (adding `--` to a
/// `grep -qF` so a leading-dash AC token is a pattern and not an option)
/// landed in the mirrored `ralph-loop` copy and was silently missed in the
/// unmirrored `new-ralph-loop` one. The workflow script is mirrored too: it
/// carries the orchestration logic, so an unreviewable change there is at
/// least as costly as one to the shell scripts.
const NEW_MIRROR_FILES: &[&str] = &[
    "claims-vs-code.sh",
    "ac-fidelity-check.sh",
    "preflight.sh",
    "ralph.workflow.js",
    "smoke.workflow.js",
];

pub fn run(args: Args) -> Result<()> {
    let repo_root = locate_repo_root()?;
    let mirror_dir = repo_root.join("tools/ralph-loop/scripts");

    if !mirror_dir.is_dir() {
        bail!(
            "tools/ralph-loop/scripts not found at {} — mirror missing",
            mirror_dir.display()
        );
    }

    let skill_dir = args.skill_dir.or_else(default_skill_dir);

    // --- 1. Mirror-sync check (if a skill_dir is available).
    if let Some(sd) = &skill_dir {
        check_mirror(sd, &mirror_dir, MIRROR_FILES, "ralph-loop")?;
    } else {
        eprintln!(
            "check-discipline-regression: no skill dir available — skipping mirror-sync check"
        );
    }

    // The new-ralph-loop skill has its own canonical directory and mirror; it
    // is checked independently so a repo without the skill installed (or
    // without the mirror yet) degrades to a notice rather than a hard failure.
    let new_skill_dir = args.new_skill_dir.or_else(default_new_skill_dir);
    let new_mirror_dir = repo_root.join("tools/new-ralph-loop/scripts");
    if let Some(sd) = &new_skill_dir {
        if new_mirror_dir.is_dir() {
            check_mirror(sd, &new_mirror_dir, NEW_MIRROR_FILES, "new-ralph-loop")?;
        } else {
            eprintln!(
                "check-discipline-regression: {} not found — skipping new-ralph-loop mirror check",
                new_mirror_dir.display()
            );
        }
    }

    if args.skip_replay {
        return Ok(());
    }

    // --- 2. Replay baselines: iter-61v must FAIL, iter-61t must PASS.
    // Use whichever copy of run-iteration.sh is present: canonical if available,
    // otherwise the mirror (so this still works in fresh checkouts without the
    // skill installed).
    let run_script = skill_dir
        .as_ref()
        .map(|sd| sd.join("run-iteration.sh"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| mirror_dir.join("run-iteration.sh"));

    if !run_script.exists() {
        bail!("run-iteration.sh not found at {}", run_script.display());
    }

    check_replay(&run_script, "61v", false /* expect FAIL */, &repo_root)?;
    check_replay(&run_script, "61t", true /* expect PASS */, &repo_root)?;

    eprintln!("check-discipline-regression: replay baselines OK (61v=FAIL, 61t=PASS)");
    Ok(())
}

fn check_replay(run_script: &Path, iter: &str, expect_pass: bool, repo_root: &Path) -> Result<()> {
    let output = Command::new("bash")
        .arg(run_script)
        .arg("--replay")
        .arg(iter)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("running replay for iter-{iter}"))?;

    let passed = output.status.success();
    if passed != expect_pass {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "replay iter-{iter}: expected {} but got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            if expect_pass { "PASS" } else { "FAIL" },
            if passed { "PASS" } else { "FAIL" },
            stdout,
            stderr,
        );
    }
    Ok(())
}

fn locate_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("running git rev-parse")?;
    if !output.status.success() {
        bail!("git rev-parse --show-toplevel failed");
    }
    let s = String::from_utf8(output.stdout).context("non-utf8 git output")?;
    Ok(PathBuf::from(s.trim()))
}

/// Compare every file in `files` between the canonical skill directory and its
/// in-repo mirror, failing with the drift list and the exact fix command.
///
/// A missing canonical directory is a skip, not a failure: a fresh checkout
/// without the skill installed (or a Windows CI run) must still be able to run
/// the rest of the discipline gates.
fn check_mirror(skill_dir: &Path, mirror_dir: &Path, files: &[&str], label: &str) -> Result<()> {
    if !skill_dir.is_dir() {
        eprintln!(
            "check-discipline-regression: skill dir {} not found — skipping {label} mirror-sync check",
            skill_dir.display()
        );
        return Ok(());
    }

    let mut drift = Vec::new();
    for name in files {
        let canonical = skill_dir.join(name);
        let mirror = mirror_dir.join(name);
        let c = std::fs::read(&canonical)
            .with_context(|| format!("reading canonical {}", canonical.display()))?;
        let m = std::fs::read(&mirror)
            .with_context(|| format!("reading mirror {}", mirror.display()))?;
        if c != m {
            drift.push((*name).to_owned());
        }
    }

    if !drift.is_empty() {
        bail!(
            "{label} mirror drift detected for: {}. \
             Run: cp {}/* {}/",
            drift.join(", "),
            skill_dir.display(),
            mirror_dir.display()
        );
    }

    eprintln!(
        "check-discipline-regression: {label} mirror in sync ({} files)",
        files.len()
    );
    Ok(())
}

fn default_new_skill_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".claude/skills/new-ralph-loop/scripts"))
}

fn default_skill_dir() -> Option<PathBuf> {
    // `HOME` on Unix, `USERPROFILE` on Windows. The skill itself is
    // Unix-only (bash scripts), but we keep the lookup cross-platform so a
    // Windows CI run skips the mirror-sync check cleanly rather than
    // crashing.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".claude/skills/ralph-loop/scripts"))
}
