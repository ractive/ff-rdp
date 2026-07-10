use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};

#[derive(ClapArgs)]
pub struct Args {
    /// Directory to scan for stray top-level live-test binaries
    /// (default: crates/ff-rdp-cli/tests).
    #[arg(long, default_value = "crates/ff-rdp-cli/tests")]
    dir: PathBuf,
}

/// Marker comment that exempts an otherwise-ungated `#[test]` under
/// `tests/live/` from the `#[ignore]` requirement (iter-113 Theme B).
///
/// Reserved for the handful of runtime-gated fast probes that must run by
/// default — e.g. the `check-pre-fix-repro` target (which invokes
/// `cargo test --exact` *without* `--include-ignored`), and Firefox-free mock
/// probes. Must appear on a comment line within the attribute block directly
/// above the `#[test]`.
const ALLOW_UNGATED_MARKER: &str = "// allow-ungated-live:";

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

    if !stray.is_empty() {
        stray.sort();
        bail!(
            "check-live-test-layout: found {} top-level live-test binary file(s). \
             Live suites must be modules of tests/live/main.rs, not standalone \
             tests/live_*.rs binaries (see iter-100b). Move each into \
             tests/live/<slug>.rs and add a `mod` line to tests/live/main.rs:\n{}",
            stray.len(),
            stray.join("\n")
        );
    }

    // iter-113 Theme B: every `#[test]` under tests/live/ must carry `#[ignore]`
    // (so a plain `cargo test` never launches Firefox) or an explicit
    // `// allow-ungated-live:` marker. This closes the failure class where an
    // ungated live test hangs a Firefox-free CI job for the whole job budget.
    let live_dir = dir.join("live");
    let mut violations: Vec<String> = Vec::new();
    if live_dir.is_dir() {
        scan_live_dir_for_ungated_tests(&live_dir, &mut violations)?;
    }

    if !violations.is_empty() {
        violations.sort();
        bail!(
            "check-live-test-layout: found {} ungated `#[test]` under {}. Every live \
             test must carry `#[ignore]` so a plain `cargo test` stays Firefox-free \
             and fast (a bare live test hangs a Firefox-less CI job for the whole \
             job budget — iter-112/113). Add `#[ignore = \"…\"]`, or, for a \
             runtime-gated fast probe that must run by default, an \
             `{ALLOW_UNGATED_MARKER} <reason>` comment in the attribute block:\n{}",
            violations.len(),
            live_dir.display(),
            violations.join("\n")
        );
    }

    eprintln!(
        "check-live-test-layout: ok — no top-level live_*.rs binaries under {} and \
         every `#[test]` under tests/live/ is `#[ignore]`-gated (or explicitly \
         allow-ungated-live)",
        dir.display()
    );
    Ok(())
}

/// Walk `tests/live/`'s `.rs` files and record every `#[test]` that lacks an
/// `#[ignore]` attribute and an `// allow-ungated-live:` marker.
fn scan_live_dir_for_ungated_tests(live_dir: &Path, violations: &mut Vec<String>) -> Result<()> {
    let entries = std::fs::read_dir(live_dir)
        .with_context(|| format!("reading directory {}", live_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading dir entry in {}", live_dir.display()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .with_context(|| format!("file type for {}", path.display()))?;
        if !ft.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        // Normalize CRLF so `\n`-based line scanning works on Windows checkouts
        // even if .gitattributes eol=lf is not yet honored (belt-and-braces,
        // matching the iter-112 pattern in unit_error_enums_non_exhaustive).
        let src = src.replace("\r\n", "\n");
        for (idx, line) in src.lines().enumerate() {
            if is_test_attr(line) && !test_is_gated(&src, idx) {
                violations.push(format!(
                    "{}:{}: `#[test]` without `#[ignore]` or `{ALLOW_UNGATED_MARKER}`",
                    path.display(),
                    idx + 1
                ));
            }
        }
    }
    Ok(())
}

/// True if `line` is exactly a `#[test]` attribute (ignoring leading whitespace).
/// Excludes `#[tokio::test]` etc. and attribute-macro paths that merely contain
/// `test`, matching the plain-`#[test]` convention live suites use.
fn is_test_attr(line: &str) -> bool {
    let t = line.trim();
    t == "#[test]"
}

/// Decide whether the `#[test]` at line index `test_idx` (0-based) is gated.
///
/// A `#[test]` is considered gated if EITHER:
///  - an `#[ignore` attribute appears in the contiguous attribute/comment block
///    that surrounds it (attributes may sit above or below `#[test]`, possibly
///    with `#[cfg(...)]` between them, e.g. `#[test] #[cfg(unix)] #[ignore]`), or
///  - an `// allow-ungated-live:` marker appears in the comment/attribute block
///    directly above the `#[test]`.
fn test_is_gated(src: &str, test_idx: usize) -> bool {
    let lines: Vec<&str> = src.lines().collect();

    // Scan upward through the contiguous block of attributes and comments
    // immediately above the `#[test]` line. Stop at the first line that is
    // neither an attribute (`#[...]`), a `//` comment, nor blank-inside-a-doc.
    let mut i = test_idx;
    while i > 0 {
        let prev = lines[i - 1].trim();
        let is_attr = prev.starts_with("#[");
        let is_comment = prev.starts_with("//");
        if is_attr || is_comment {
            if prev.starts_with("#[ignore") {
                return true;
            }
            if prev.contains(ALLOW_UNGATED_MARKER) {
                return true;
            }
            i -= 1;
        } else {
            break;
        }
    }

    // Scan downward through attributes between `#[test]` and the `fn`. Handles
    // `#[test]` / `#[cfg(unix)]` / `#[ignore]` / `fn …` ordering.
    let mut j = test_idx + 1;
    while j < lines.len() {
        let next = lines[j].trim();
        if next.starts_with("#[") {
            if next.starts_with("#[ignore") {
                return true;
            }
            j += 1;
        } else {
            // Reached the `fn` (or something that ends the attribute block).
            break;
        }
    }

    false
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
        // The live module's test is #[ignore]-gated so the iter-113 gating
        // check passes alongside the top-level-binary check under test here.
        fs::write(
            live.join("live_foo.rs"),
            "#[test]\n#[ignore = \"live\"]\nfn t() {}\n",
        )
        .unwrap();
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
        // non-recursive top-level scan must not flag them as stray binaries. The
        // module content is `#[ignore]`-gated so the iter-113 gating check passes
        // too.
        let dir = TempDir::new().unwrap();
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();
        fs::write(
            live.join("live_96_profile_cleanup.rs"),
            "#[test]\n#[ignore = \"live\"]\nfn t() {}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(run(args).is_ok());
    }

    // ── iter-113 Theme B: mandatory #[ignore] gating under tests/live/ ────────

    /// AC: `layout_guard_rejects_ungated_test` — a bare `#[test]` (no
    /// `#[ignore]`, no allow marker) injected under tests/live/ must FAIL the
    /// guard; a properly-gated tree must PASS.
    #[test]
    fn layout_guard_rejects_ungated_test() {
        let dir = TempDir::new().unwrap();
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();

        // A correctly-gated file — the positive fixture (mirrors the real
        // convention `#[test]` then `#[ignore = "…"]`).
        fs::write(
            live.join("live_ok.rs"),
            "#[test]\n#[ignore = \"requires a live Firefox instance\"]\nfn ok() {}\n",
        )
        .unwrap();

        // Baseline: with only gated tests, the guard passes.
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(
            run(args).is_ok(),
            "guard must pass when every #[test] is #[ignore]-gated"
        );

        // Inject a bare, ungated #[test]. Now the guard must fail and name it.
        fs::write(live.join("live_bad.rs"), "#[test]\nfn bare() {}\n").unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        let err = run(args).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("live_bad.rs"),
            "error must name the ungated file, got: {msg}"
        );
        assert!(
            msg.contains("ungated"),
            "error must explain the gating requirement, got: {msg}"
        );
    }

    /// An `// allow-ungated-live:` marker exempts a runtime-gated fast probe.
    #[test]
    fn allow_ungated_marker_exempts_test() {
        let dir = TempDir::new().unwrap();
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();
        fs::write(
            live.join("live_probe.rs"),
            "// allow-ungated-live: mock server, no Firefox\n#[test]\nfn probe() {}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(
            run(args).is_ok(),
            "an allow-ungated-live marker must exempt the test from #[ignore]"
        );
    }

    /// `#[cfg(...)]` may sit between `#[test]` and `#[ignore]` — still gated.
    #[test]
    fn cfg_between_test_and_ignore_is_gated() {
        let dir = TempDir::new().unwrap();
        let live = dir.path().join("live");
        fs::create_dir(&live).unwrap();
        fs::write(
            live.join("live_cfg.rs"),
            "#[test]\n#[cfg(unix)]\n#[ignore = \"live\"]\nfn t() {}\n",
        )
        .unwrap();
        let args = Args {
            dir: dir.path().to_path_buf(),
        };
        assert!(
            run(args).is_ok(),
            "an #[ignore] after an intervening #[cfg] must still count as gated"
        );
    }

    /// The real tree must pass: run the check against the actual
    /// crates/ff-rdp-cli/tests directory so this guard is a live regression
    /// fence, not just a synthetic-fixture test.
    #[test]
    fn real_tree_passes_gating_check() {
        // Resolve the repo's tests dir relative to this crate.
        let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ff-rdp-cli")
            .join("tests");
        if !tests_dir.join("live").is_dir() {
            // Defensive: skip if the layout ever moves (keeps the check crate
            // relocatable). The CI invocation covers the real path regardless.
            eprintln!("real_tree_passes_gating_check: tests/live not found — skipping");
            return;
        }
        let args = Args { dir: tests_dir };
        assert!(
            run(args).is_ok(),
            "the real tests/live/ tree must satisfy the #[ignore] gating check"
        );
    }
}
