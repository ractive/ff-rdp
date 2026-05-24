use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use regex::Regex;
use std::path::PathBuf;

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
/// The regex allows arbitrary whitespace between `.lock()` and `.unwrap()`
/// so rustfmt-split chains like `firefox_writer\n    .lock()\n    .unwrap()`
/// are caught — these were previously bypassing the check, which is why
/// iter-63's post-review hardening added the `\s*` combination.
///
/// `.lock().expect(...)` is intentionally NOT in the regex: test modules
/// (gated by `#[cfg(test)]`) still use that form against a `buffer` mutex
/// where panic-on-poison is the desired test behaviour. Production code
/// is verified by inspection during code review.
///
/// This function uses `walkdir` + `regex` (both workspace deps) instead of
/// shelling out to ripgrep so it works on CI runners that don't ship `rg`.
pub fn run(args: Args) -> Result<()> {
    let dir = args
        .dir
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", args.dir.display()))?;

    let pattern =
        Regex::new(r"\.lock\(\)\s*\.unwrap\(\)").context("compiling lock-unwrap regex")?;

    let mut hits: Vec<String> = Vec::new();
    scan_dir(&dir, &pattern, &mut hits)?;

    if hits.is_empty() {
        eprintln!(
            "check-daemon-locks: ok — no `.lock().unwrap()` under {}",
            dir.display()
        );
        Ok(())
    } else {
        bail!(
            "check-daemon-locks: found `.lock().unwrap()` in daemon code — use `lock_or_recover!` instead:\n{}",
            hits.join("\n")
        );
    }
}

/// Recursively walk `dir`, reading every `.rs` file and appending
/// `file:line: <content>` entries to `hits` for any line that matches
/// `pattern` (after stripping newlines so multiline chains are handled by
/// concatenating consecutive lines into a sliding window).
fn scan_dir(dir: &std::path::Path, pattern: &Regex, hits: &mut Vec<String>) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading dir entry in {}", dir.display()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .with_context(|| format!("file type for {}", path.display()))?;

        if ft.is_dir() {
            scan_dir(&path, pattern, hits)?;
        } else if ft.is_file() && path.extension().is_some_and(|e| e == "rs") {
            let src = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            // Build a whitespace-normalised view: join all lines with a single
            // space so that rustfmt-split `.lock()\n    .unwrap()` is treated
            // as `.lock() .unwrap()` — which still matches `\.lock\(\)\s*\.unwrap\(\)`.
            // We also report per-line hits for readability.
            // Strategy: search the normalised full-file string but report the
            // original line range for each match so the output is actionable.
            let normalised = src.replace('\n', " ");
            if pattern.is_match(&normalised) {
                // Find the original lines that contain fragments of the pattern.
                // A simple conservative approach: flag every line that contains
                // `.lock()` or `.unwrap()` when both appear in the file, letting
                // the developer see the context.  For precise attribution, scan
                // each line for the single-line form first, then also report the
                // multiline case at the first `.lock()` line.
                let mut i = 0usize;
                let lines: Vec<&str> = src.lines().collect();
                while i < lines.len() {
                    // Single-line match.
                    if pattern.is_match(lines[i]) {
                        hits.push(format!("{}:{}: {}", path.display(), i + 1, lines[i].trim()));
                        i += 1;
                        continue;
                    }
                    // Multiline: build a two-line window with whitespace normalised.
                    if i + 1 < lines.len() {
                        let window = format!("{} {}", lines[i].trim(), lines[i + 1].trim());
                        if pattern.is_match(&window) {
                            hits.push(format!("{}:{}: {}", path.display(), i + 1, lines[i].trim()));
                            i += 2;
                            continue;
                        }
                    }
                    // Three-line window.
                    if i + 2 < lines.len() {
                        let window = format!(
                            "{} {} {}",
                            lines[i].trim(),
                            lines[i + 1].trim(),
                            lines[i + 2].trim()
                        );
                        if pattern.is_match(&window) {
                            hits.push(format!("{}:{}: {}", path.display(), i + 1, lines[i].trim()));
                            i += 3;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
        }
    }
    Ok(())
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
