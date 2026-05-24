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
        if sd.is_dir() {
            let mut drift = Vec::new();
            for name in MIRROR_FILES {
                let canonical = sd.join(name);
                let mirror = mirror_dir.join(name);
                let c = std::fs::read(&canonical)
                    .with_context(|| format!("reading canonical {}", canonical.display()))?;
                let m = std::fs::read(&mirror)
                    .with_context(|| format!("reading mirror {}", mirror.display()))?;
                if c != m {
                    drift.push(name.to_string());
                }
            }
            if !drift.is_empty() {
                bail!(
                    "mirror drift detected for: {}. \
                     Run: cp ~/.claude/skills/ralph-loop/scripts/*.sh tools/ralph-loop/scripts/",
                    drift.join(", ")
                );
            }
            eprintln!(
                "check-discipline-regression: mirror in sync ({} files)",
                MIRROR_FILES.len()
            );
        } else {
            eprintln!(
                "check-discipline-regression: skill dir {} not found — skipping mirror-sync check",
                sd.display()
            );
        }
    } else {
        eprintln!(
            "check-discipline-regression: no skill dir available — skipping mirror-sync check"
        );
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

    check_replay(&run_script, "61v", false /* expect FAIL */)?;
    check_replay(&run_script, "61t", true /* expect PASS */)?;

    eprintln!("check-discipline-regression: replay baselines OK (61v=FAIL, 61t=PASS)");
    Ok(())
}

fn check_replay(run_script: &Path, iter: &str, expect_pass: bool) -> Result<()> {
    let output = Command::new("bash")
        .arg(run_script)
        .arg("--replay")
        .arg(iter)
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

fn default_skill_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/skills/ralph-loop/scripts"))
}
