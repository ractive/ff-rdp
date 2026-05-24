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
///
/// The regex uses ripgrep's multiline mode (`-U`) and allows arbitrary
/// whitespace between `.lock()` and `.unwrap()` so rustfmt-split chains
/// like `firefox_writer\n    .lock()\n    .unwrap()` are caught — these
/// were previously bypassing the check, which is why iter-63's
/// post-review hardening added the `-U`/`\s*` combination.
///
/// `.lock().expect(...)` is intentionally NOT in the regex: test modules
/// (gated by `#[cfg(test)]`) still use that form against a `buffer` mutex
/// where panic-on-poison is the desired test behaviour. Production code
/// is verified by inspection during code review.
pub fn run(args: Args) -> Result<()> {
    let dir = args
        .dir
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", args.dir.display()))?;

    let output = Command::new("rg")
        .args([
            "-U",
            "--no-heading",
            "--line-number",
            r"\.lock\(\)\s*\.unwrap\(\)",
        ])
        .arg(&dir)
        .output()
        .context("running ripgrep — install with `apt-get install ripgrep`")?;

    // rg exit codes: 0 = matches found, 1 = no matches, ≥2 = error.
    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "check-daemon-locks: found `.lock().unwrap()` in daemon code — use `lock_or_recover!` instead:\n{}",
                stdout.trim()
            );
        }
        Some(1) => {
            eprintln!(
                "check-daemon-locks: ok — no `.lock().unwrap()` under {}",
                dir.display()
            );
            Ok(())
        }
        other => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "check-daemon-locks: ripgrep exited with status {other:?}: {}",
                stderr.trim()
            );
        }
    }
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

    #[test]
    fn fails_on_rustfmt_split_regression() {
        // Guard: rustfmt-split `.lock()\n    .unwrap()` must still be
        // caught — this was the post-review gap that motivated -U/\s*.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("split.rs"),
            "fn f() {\n    firefox_writer\n        .lock()\n        .unwrap()\n        .send(&msg);\n}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        let err = run(args).unwrap_err();
        assert!(
            err.to_string().contains("lock_or_recover"),
            "multiline split must still be caught, got: {err}"
        );
    }
}
