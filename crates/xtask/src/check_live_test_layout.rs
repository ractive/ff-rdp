use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Directory to scan for stray top-level live-test binaries
    /// (default: crates/ff-rdp-cli/tests).
    #[arg(long, default_value = "crates/ff-rdp-cli/tests")]
    dir: PathBuf,
}

/// Fails if any top-level `crates/ff-rdp-cli/tests/live_*.rs` integration-test
/// binary exists.
///
/// Since iter-100b every live-Firefox suite is a `mod` under `tests/live/`,
/// compiled into the single gated `tests/live/main.rs` target instead of its
/// own top-level test binary. A plain `cargo test` links one live binary
/// instead of ~45. A new top-level `tests/live_*.rs` file re-introduces the
/// per-binary linking cost this iteration removed, so it is a review defect.
///
/// This guard is wired into `check-iteration-ready` and the CI discipline job
/// so ralph-loop agents can't regress the layout silently. New live tests go
/// in `tests/live/<slug>.rs` plus a `mod` line in `tests/live/main.rs`.
///
/// Files directly *inside* `tests/live/` (the consolidated modules) are fine —
/// only files matching `tests/<top-level>/live_*.rs` are the regression this
/// scans for.
pub fn run(args: Args) -> Result<()> {
    let dir = &args.dir;

    // Non-recursive read of the tests dir: only the top-level entries become
    // auto-detected `tests/*.rs` integration-test binaries. `tests/live/*.rs`
    // are modules of the consolidated target, not binaries.
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;

    let mut stray: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading dir entry in {}", dir.display()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .with_context(|| format!("file type for {}", path.display()))?;
        if !ft.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with("live_") && name.ends_with(".rs") {
            stray.push(path.display().to_string());
        }
    }

    if stray.is_empty() {
        eprintln!(
            "check-live-test-layout: ok — no top-level live_*.rs binaries under {} \
             (live suites live in tests/live/)",
            dir.display()
        );
        Ok(())
    } else {
        stray.sort();
        bail!(
            "check-live-test-layout: found {} top-level live-test binary file(s). \
             Live suites must be modules of tests/live/main.rs, not standalone \
             tests/live_*.rs binaries (see iter-100b). Move each into \
             tests/live/<slug>.rs and add a `mod` line to tests/live/main.rs:\n{}",
            stray.len(),
            stray.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn passes_when_no_top_level_live_files() {
        let dir = TempDir::new().unwrap();
        // A consolidated live/ subdir with modules is fine.
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();
        fs::write(live.join("main.rs"), "mod live_foo;\n").unwrap();
        fs::write(live.join("live_foo.rs"), "#[test]\nfn t() {}\n").unwrap();
        // Other top-level non-live tests are fine.
        fs::write(dir.path().join("cli_version.rs"), "#[test]\nfn v() {}\n").unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(run(args).is_ok());
    }

    #[test]
    fn fails_on_stray_top_level_live_file() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("live_regression.rs"),
            "#[test]\nfn t() {}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        let err = run(args).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("live_regression.rs"),
            "error must name the stray file, got: {msg}"
        );
        assert!(
            msg.contains("tests/live/main.rs"),
            "error must point at the consolidated target, got: {msg}"
        );
    }

    #[test]
    fn ignores_live_files_inside_live_subdir() {
        // Files under tests/live/ are modules, not top-level binaries — the
        // non-recursive scan must not flag them.
        let dir = TempDir::new().unwrap();
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();
        fs::write(
            live.join("live_96_profile_cleanup.rs"),
            "#[test]\nfn t() {}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(run(args).is_ok());
    }
}
