use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Directory to scan (default: crates/ff-rdp-cli/src/daemon).
    #[arg(long, default_value = "crates/ff-rdp-cli/src/daemon")]
    dir: PathBuf,
}

/// Fails if any `.lock().unwrap()` occurrence remains under `dir`.
///
/// The daemon must use `lock_or_recover!` so a poisoned mutex doesn't take
/// the whole process down. This guard runs in CI to prevent regressions —
/// see iter-63.
pub fn run(args: Args) -> Result<()> {
    let dir = args
        .dir
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", args.dir.display()))?;

    let output = Command::new("rg")
        .args(["--no-heading", "--line-number", r"\.lock\(\)\.unwrap\(\)"])
        .arg(&dir)
        .output()
        .context("running ripgrep — install with `apt-get install ripgrep`")?;

    // rg exits 1 when there are no matches, which is the success case here.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout_trim = stdout.trim();
    if stdout_trim.is_empty() {
        eprintln!(
            "check-daemon-locks: ok — no `.lock().unwrap()` under {}",
            dir.display()
        );
        return Ok(());
    }

    bail!(
        "check-daemon-locks: found `.lock().unwrap()` in daemon code — use `lock_or_recover!` instead:\n{stdout_trim}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn passes_when_no_unwraps() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("clean.rs"),
            "fn f() { let _ = lock_or_recover!(state.x); }\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(run(args).is_ok());
    }

    #[test]
    fn fails_on_regression() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("regression.rs"),
            "fn f() { let _ = state.mu.lock().unwrap(); }\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        let err = run(args).unwrap_err();
        assert!(
            err.to_string().contains("lock_or_recover"),
            "error must point at the macro, got: {err}"
        );
    }
}
