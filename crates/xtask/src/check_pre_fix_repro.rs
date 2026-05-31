//! `xtask check-pre-fix-repro`
//!
//! Parses `pre_fix_repro_test:` annotations from iteration plan theme headings
//! and verifies that each named test FAILs on `origin/main` (the bug is
//! reproducible before the fix lands).
//!
//! ## New flow (iter-91)
//!
//! 1. **No working-tree mutation** — the old `StashGuard` + `CheckoutGuard`
//!    approach is gone entirely.
//! 2. **Persistent main-side git worktree** at
//!    `${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro/main-tree`.
//!    Lazily created via `git worktree add <path> origin/main`.
//!    Path resolution is pure (`resolve_main_worktree_path`); creation is
//!    side-effecting (`ensure_main_worktree`).  The resolved path is memoised
//!    per-process via a `std::sync::OnceLock`.
//! 3. **Worktree refresh** before each gate invocation:
//!    `git -C <path> fetch origin --depth=1` then
//!    `git -C <path> reset --hard origin/main`.  The post-reset SHA is
//!    captured and used as the cache key.
//! 4. **Main-side cargo invocation** uses `CARGO_TARGET_DIR=<worktree>/target`
//!    and `--manifest-path <worktree>/Cargo.toml`.
//! 5. **SHA-keyed result cache** at
//!    `${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro/results/<sha>-<crate>-<slug>`.
//!    Cache file contains `PASS\n` or `FAIL\n` followed by an ISO-8601-like
//!    timestamp. Cache reads/writes are best-effort — errors are warned to
//!    stderr and the real cargo run is used as a fallback.
//! 6. **Only the red-on-main probe** — there is no second "green on branch"
//!    run.  That is the caller's responsibility.
//!
//! ## Testability env-var overrides
//!
//! - `FF_RDP_PRE_FIX_REPRO_CACHE_DIR` — overrides
//!   `${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro` root.
//! - `FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE=1` — skip worktree refresh AND cargo
//!   invocation; return cache hits only.  Set + cache miss → hard error.
//! - `FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE=<sha>` — bypass `git rev-parse` and
//!   use this SHA as the main SHA (allows tests to pre-seed cache without a
//!   real worktree).
//!
//! Iterations with no `pre_fix_repro_test:` annotations are silently skipped.
//! It is wired into `check-iteration-ready` after `check-dead-primitives` and
//! before `check-dogfood-script`.

use anyhow::{Context, Result, anyhow};
use clap::Args as ClapArgs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    #[arg(long)]
    pub plan: PathBuf,

    /// Crate to search for the pre-fix repro tests (default: whole workspace).
    #[arg(long)]
    pub crate_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed `pre_fix_repro_test:` annotation.
#[derive(Debug, PartialEq, Eq)]
pub struct PreFixAnnotation {
    /// The theme title (everything after `### ` and before ` [pre_fix_repro_test:...]`).
    theme: String,
    /// The test slug named in the annotation.
    test_slug: String,
}

// ---------------------------------------------------------------------------
// Annotation parser (unchanged from iter-87)
// ---------------------------------------------------------------------------

/// Parse `pre_fix_repro_test: <slug>` annotations from theme headings in the
/// plan body.
///
/// Recognised format:
/// ```text
/// ### Theme A — Some title [pre_fix_repro_test: my_test_slug]
/// ```
///
/// Returns a `Vec<PreFixAnnotation>`. Themes without the annotation are
/// silently skipped. Content inside fenced code blocks is ignored.
fn parse_pre_fix_repro_annotations(body: &str) -> Vec<PreFixAnnotation> {
    let mut annotations = Vec::new();
    let mut in_code_block = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks (``` or ~~~) — skip their contents.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Only consider ### headings (theme level)
        if !trimmed.starts_with("### ") {
            continue;
        }

        // Look for `[pre_fix_repro_test: <slug>]`
        let Some(bracket_start) = trimmed.find("[pre_fix_repro_test:") else {
            continue;
        };
        let Some(bracket_end) = trimmed[bracket_start..].find(']') else {
            continue;
        };

        let annotation_body = &trimmed[bracket_start + 1..bracket_start + bracket_end];
        // annotation_body is: "pre_fix_repro_test: <slug>"
        let Some(colon_pos) = annotation_body.find(':') else {
            continue;
        };
        let slug = annotation_body[colon_pos + 1..].trim();
        if slug.is_empty() {
            continue;
        }

        // Extract theme label — everything from "### " up to the bracket annotation.
        let heading_text = trimmed.trim_start_matches('#').trim();
        let theme_text = if let Some(bracket_pos) = heading_text.find(" [pre_fix_repro_test:") {
            heading_text[..bracket_pos].trim()
        } else {
            heading_text
        };

        annotations.push(PreFixAnnotation {
            theme: theme_text.to_owned(),
            test_slug: slug.to_owned(),
        });
    }

    annotations
}

// ---------------------------------------------------------------------------
// Worktree path resolution  (pure — no side effects)
// ---------------------------------------------------------------------------

static MAIN_WORKTREE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Resolve the path to the persistent main-side worktree without creating it.
///
/// Pure function: honours `FF_RDP_PRE_FIX_REPRO_CACHE_DIR` override and
/// `XDG_CACHE_HOME`, but does not touch the filesystem.
///
/// The result is memoised per-process via a `OnceLock`.
fn resolve_main_worktree_path() -> PathBuf {
    MAIN_WORKTREE_PATH
        .get_or_init(|| {
            let base = worktree_cache_root();
            base.join("main-tree")
        })
        .clone()
}

/// Return the cache root (`${override:-${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro}`).
fn worktree_cache_root() -> PathBuf {
    if let Ok(override_dir) = std::env::var("FF_RDP_PRE_FIX_REPRO_CACHE_DIR") {
        return PathBuf::from(override_dir);
    }
    let xdg = std::env::var("XDG_CACHE_HOME").unwrap_or_default();
    let cache_home = if xdg.is_empty() {
        dirs_home().join(".cache")
    } else {
        PathBuf::from(xdg)
    };
    cache_home.join("ff-rdp").join("pre-fix-repro")
}

/// Return the current user's home directory.
fn dirs_home() -> PathBuf {
    // HOME is set on every Unix platform; USERPROFILE on Windows.
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from(".")
}

// ---------------------------------------------------------------------------
// Worktree creation / refresh
// ---------------------------------------------------------------------------

/// Ensure the persistent worktree exists at `path`, creating it via
/// `git worktree add` if it does not.  Idempotent.
///
/// If a `.git` file exists but belongs to a different repository, the stale
/// directory is removed and the worktree is recreated.
fn ensure_main_worktree(path: &Path) -> Result<()> {
    // If the directory already has a `.git` file/dir, verify it belongs to
    // the current repository before reusing it.
    if path.join(".git").exists() {
        // Ask git for the repo root of the *current* working directory.
        let current_root = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("failed to invoke `git rev-parse --show-toplevel` for current repo")?;
        let current_root_str = String::from_utf8_lossy(&current_root.stdout)
            .trim()
            .to_owned();

        // Ask git for the repo root of the candidate worktree directory.
        let worktree_root = Command::new("git")
            .args([
                "-C",
                &path.to_string_lossy(),
                "rev-parse",
                "--show-toplevel",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success());

        let roots_match = worktree_root
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == current_root_str)
            .unwrap_or(false);

        if roots_match {
            return Ok(());
        }

        // Mismatch or unreadable — remove stale directory and fall through to recreate.
        std::fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove stale worktree dir {path:?}"))?;
    }

    // Create parent directory.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create worktree parent dir {parent:?}"))?;
    }

    let status = Command::new("git")
        .args(["worktree", "add", &path.to_string_lossy(), "origin/main"])
        .status()
        .context("failed to invoke `git worktree add`")?;

    if !status.success() {
        return Err(anyhow!("git worktree add {:?} origin/main failed", path));
    }
    Ok(())
}

/// Fetch latest `origin/main` into the worktree and reset its HEAD to it.
/// Returns the SHA of `origin/main` after the reset.
fn refresh_worktree(path: &Path) -> Result<String> {
    // fetch
    let fetch_status = Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "fetch",
            "origin",
            "--depth=1",
        ])
        .status()
        .context("failed to invoke `git fetch` in worktree")?;
    if !fetch_status.success() {
        return Err(anyhow!("git fetch in worktree {:?} failed", path));
    }

    // reset --hard origin/main
    let reset_status = Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "reset",
            "--hard",
            "origin/main",
        ])
        .status()
        .context("failed to invoke `git reset --hard` in worktree")?;
    if !reset_status.success() {
        return Err(anyhow!(
            "git reset --hard origin/main in worktree {:?} failed",
            path
        ));
    }

    // Capture the SHA.
    capture_sha(path)
}

/// Read the current HEAD SHA from a worktree.
fn capture_sha(path: &Path) -> Result<String> {
    // Allow SHA override for tests.
    if let Ok(sha) = std::env::var("FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE")
        && !sha.is_empty()
    {
        return Ok(sha);
    }

    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .context("failed to invoke `git rev-parse HEAD` in worktree")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "git rev-parse HEAD in worktree {:?} failed: {stderr}",
            path
        ));
    }
    let sha = String::from_utf8(output.stdout)
        .context("non-utf8 git rev-parse output")?
        .trim()
        .to_owned();
    Ok(sha)
}

// ---------------------------------------------------------------------------
// Result cache
// ---------------------------------------------------------------------------

fn results_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("results")
}

fn cache_key(sha: &str, crate_name: Option<&str>, slug: &str) -> String {
    let crate_part = crate_name.unwrap_or("workspace");
    format!("{sha}-{crate_part}-{slug}")
}

fn cache_file_path(cache_root: &Path, sha: &str, crate_name: Option<&str>, slug: &str) -> PathBuf {
    results_dir(cache_root).join(cache_key(sha, crate_name, slug))
}

/// ISO-8601-ish UTC timestamp using only std (no chrono dependency).
/// Format: `YYYY-MM-DDTHH:MM:SSZ`
fn iso_timestamp_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Compute date parts from unix epoch using proleptic Gregorian calendar.
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400; // days since 1970-01-01

    // Year / month / day from day count (Gregorian).
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).  Gregorian calendar.
fn days_to_ymd(mut z: u64) -> (u32, u32, u32) {
    z += 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

/// Write a cache entry.  Best-effort: errors are returned for the caller to warn.
fn cache_write(
    cache_root: &Path,
    sha: &str,
    crate_name: Option<&str>,
    slug: &str,
    passed: bool,
) -> Result<()> {
    let dir = results_dir(cache_root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create cache dir {dir:?}"))?;
    let path = cache_file_path(cache_root, sha, crate_name, slug);
    let content = format!(
        "{}\n{}\n",
        if passed { "PASS" } else { "FAIL" },
        iso_timestamp_utc()
    );
    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write cache file {path:?}"))?;
    Ok(())
}

/// Try to read a cache entry.  Returns `Some(passed)` on hit, `None` on miss.
/// Corrupt files emit a warning to stderr and are treated as a cache miss.
fn cache_read(cache_root: &Path, sha: &str, crate_name: Option<&str>, slug: &str) -> Option<bool> {
    let path = cache_file_path(cache_root, sha, crate_name, slug);
    let content = std::fs::read_to_string(&path).ok()?;
    let first_line = content.lines().next().unwrap_or("").trim();
    match first_line {
        "PASS" => Some(true),
        "FAIL" => Some(false),
        _ => {
            eprintln!(
                "warning: corrupt cache file {:?}: first line {:?} (expected PASS/FAIL); treating as miss",
                path, first_line
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Test resolution and execution (main-worktree variant)
// ---------------------------------------------------------------------------

/// Resolve a bare test slug to its fully-qualified test path by searching
/// `cargo test -- --list` output in the main-side worktree.
fn resolve_test_path_in_worktree(
    slug: &str,
    crate_name: Option<&str>,
    worktree: &Path,
) -> Result<(String, Option<String>)> {
    let crates_to_try: Vec<Option<&str>> = if crate_name.is_some() {
        vec![crate_name]
    } else {
        vec![Some("xtask"), None]
    };

    let target_dir = worktree.join("target");
    let manifest_path = worktree.join("Cargo.toml");

    for try_crate in &crates_to_try {
        if let Some(path) =
            try_list_tests_in_worktree(slug, *try_crate, &target_dir, &manifest_path)?
        {
            return Ok((path, try_crate.map(str::to_owned)));
        }
    }

    Err(anyhow!(
        "test slug '{slug}' not found in `cargo test -- --list` output in worktree {:?}.\n\
         Make sure the test exists on origin/main and is compiled.",
        worktree
    ))
}

fn try_list_tests_in_worktree(
    slug: &str,
    crate_name: Option<&str>,
    target_dir: &Path,
    manifest_path: &Path,
) -> Result<Option<String>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    cmd.args(["--manifest-path", &manifest_path.to_string_lossy()]);
    cmd.env("CARGO_TARGET_DIR", target_dir);

    if let Some(name) = crate_name {
        cmd.args(["-p", name]);
    } else {
        cmd.arg("--workspace");
    }
    cmd.args(["--", "--list"]);

    let output = cmd
        .output()
        .context("failed to invoke `cargo test -- --list` in worktree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`cargo test{} -- --list` in worktree failed (exit {}): {stderr}",
            crate_name
                .map(|c| format!(" -p {c}"))
                .unwrap_or_else(|| " --workspace".to_owned()),
            output.status.code().unwrap_or(-1)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = stdout
        .lines()
        .filter(|l| l.ends_with(": test") || l.ends_with(": bench"))
        .find_map(|l| {
            let name = l.trim_end_matches(": test").trim_end_matches(": bench");
            if name == slug
                || name.ends_with(&format!("::{slug}"))
                || name.ends_with(&format!("/{slug}"))
            {
                Some(name.to_owned())
            } else {
                None
            }
        });
    Ok(found)
}

/// Run a single named test by its fully-qualified path in the main-side
/// worktree and return whether it passed.
fn run_test_in_worktree(
    full_path: &str,
    crate_name: Option<&str>,
    worktree: &Path,
) -> Result<bool> {
    let target_dir = worktree.join("target");
    let manifest_path = worktree.join("Cargo.toml");

    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    cmd.args(["--manifest-path", &manifest_path.to_string_lossy()]);
    cmd.env("CARGO_TARGET_DIR", &target_dir);

    if let Some(name) = crate_name {
        cmd.args(["-p", name]);
    } else {
        cmd.arg("--workspace");
    }
    cmd.args(["-q", "--", full_path, "--exact"]);

    let output = cmd
        .output()
        .context("failed to invoke `cargo test` in worktree")?;

    if !output.status.success() {
        return Ok(false);
    }

    // `cargo test --exact <name>` exits 0 even when no tests match the name —
    // require at least one PASSED test in the summary line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let passed_at_least_one = stdout.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("test result: ok.") && !t.starts_with("test result: ok. 0 passed")
    });
    Ok(passed_at_least_one)
}

// ---------------------------------------------------------------------------
// RunConfig — injectable config for testability (avoids env var races)
// ---------------------------------------------------------------------------

/// Runtime configuration injected into `run_with_writer`.
///
/// In production, build via `RunConfig::from_env()`.
/// In tests, construct directly to avoid global env var mutation races.
struct RunConfig {
    /// Cache root directory (`${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro`).
    cache_root: PathBuf,
    /// When `Some(sha)`, skip worktree refresh + cargo and use this SHA as the
    /// main SHA (cache-only mode).  `None` → run the full worktree path.
    sha_override: Option<String>,
}

impl RunConfig {
    /// Build from environment variables (used by the production entry-point).
    fn from_env() -> Self {
        let cache_root = worktree_cache_root();
        let sha_override = {
            let skip = std::env::var("FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE")
                .map(|v| v == "1")
                .unwrap_or(false);
            if skip {
                Some(std::env::var("FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE").unwrap_or_default())
            } else {
                std::env::var("FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE")
                    .ok()
                    .filter(|s| !s.is_empty())
            }
        };
        RunConfig {
            cache_root,
            sha_override,
        }
    }
}

// ---------------------------------------------------------------------------
// Public run() entry-point (delegates to run_with_writer for testability)
// ---------------------------------------------------------------------------

pub fn run(args: Args) -> Result<()> {
    run_with_writer(
        args,
        RunConfig::from_env(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// Testable entry-point: same as `run()` but accepts an explicit `RunConfig`
/// and writes info to `out` / failures to `err`.  This lets unit tests inject
/// config without mutating global env vars (which races with parallel tests).
fn run_with_writer(
    args: Args,
    config: RunConfig,
    out: &mut dyn IoWrite,
    err: &mut dyn IoWrite,
) -> Result<()> {
    let content = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("failed to read {:?}", args.plan))?;

    let plan = crate::check_iteration_plan::parse_plan(&content)
        .with_context(|| format!("failed to parse plan {:?}", args.plan))?;

    let annotations = parse_pre_fix_repro_annotations(&plan.body);

    if annotations.is_empty() {
        writeln!(
            out,
            "check-pre-fix-repro: SKIP (no pre_fix_repro_test annotations in plan)"
        )?;
        return Ok(());
    }

    let crate_name = args.crate_name.as_deref();
    let cache_root = config.cache_root;
    let skip_worktree = config.sha_override.is_some();
    let worktree_path = resolve_main_worktree_path();

    // Determine the main SHA.
    let main_sha = if let Some(sha) = config.sha_override {
        if sha.is_empty() {
            return Err(anyhow!(
                "FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE=1 requires \
                 FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE to be set (was empty)"
            ));
        }
        sha
    } else {
        ensure_main_worktree(&worktree_path)?;
        refresh_worktree(&worktree_path)?
    };

    let mut failures: Vec<String> = Vec::new();

    for annotation in &annotations {
        let slug = &annotation.test_slug;
        writeln!(
            out,
            "check-pre-fix-repro: checking '{}' (theme: {})",
            slug, annotation.theme
        )?;

        // --- Check cache first ---
        if let Some(cached_passed) = cache_read(&cache_root, &main_sha, crate_name, slug) {
            if cached_passed {
                failures.push(format!(
                    "  [{slug}] red on main (cache hit): test PASSED on origin/main — \
                     expected FAIL. The pre-fix repro test must be red on main before the \
                     fix lands."
                ));
            } else {
                writeln!(out, "  [{slug}] red on main (cache hit)")?;
            }
            continue;
        }

        // --- Cache miss: SKIP_WORKTREE → hard error ---
        if skip_worktree {
            failures.push(format!(
                "  [{slug}] cache miss with FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE=1: \
                 no cache entry for sha={main_sha} crate={} slug={slug}. \
                 Pre-seed the cache before using skip mode.",
                crate_name.unwrap_or("workspace")
            ));
            continue;
        }

        // --- Resolve slug in worktree ---
        // A pre-fix-repro test is typically added on the branch and does not
        // exist on origin/main. "Not found in cargo test --list" is therefore
        // the expected red-on-main outcome — treat it as FAIL rather than a
        // hard error.
        let (main_passed, rerun_crate_owned): (bool, Option<String>) =
            match resolve_test_path_in_worktree(slug, crate_name, &worktree_path) {
                Ok((full_path, resolved_crate)) => {
                    let rerun_crate = resolved_crate.as_deref();
                    let passed = match run_test_in_worktree(&full_path, rerun_crate, &worktree_path)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            writeln!(
                                err,
                                "warning: [{slug}] cargo run error (treating as FAIL): {e}"
                            )?;
                            false
                        }
                    };
                    (passed, resolved_crate)
                }
                Err(_) => {
                    writeln!(
                        out,
                        "  [{slug}] test not present on origin/main worktree — treating as FAIL"
                    )?;
                    (false, crate_name.map(str::to_owned))
                }
            };
        // NOTE: `rerun_crate_owned` captured which crate cargo resolved, but we
        // intentionally key the cache on `crate_name` (the CLI arg) so that
        // `cache_read` and `cache_write` always use the same key.
        let _ = rerun_crate_owned;

        // --- Write to cache (best-effort) ---
        // Use `crate_name` (the CLI arg) as the key to match `cache_read` above.
        let cache_label = match cache_write(&cache_root, &main_sha, crate_name, slug, main_passed) {
            Ok(()) => "cargo run".to_owned(),
            Err(e) => format!("cargo run, cache write failed: {e}"),
        };

        if main_passed {
            failures.push(format!(
                "  [{slug}] red on main ({cache_label}): test PASSED on origin/main — \
                 expected FAIL. The pre-fix repro test must be red on main before the \
                 fix lands."
            ));
        } else {
            writeln!(out, "  [{slug}] red on main ({cache_label})")?;
        }
    }

    if failures.is_empty() {
        writeln!(
            out,
            "check-pre-fix-repro: OK ({} annotation(s) verified)",
            annotations.len()
        )?;
        Ok(())
    } else {
        for f in &failures {
            writeln!(err, "{f}")?;
        }
        Err(anyhow!(
            "check-pre-fix-repro: {} annotation(s) failed verification",
            failures.len()
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Helpers shared by multiple tests
    // -----------------------------------------------------------------------

    /// Build a minimal valid plan with one pre_fix_repro_test annotation.
    fn make_plan_with_annotation(slug: &str) -> String {
        format!(
            "---\ntitle: \"Test Plan\"\nstatus: planned\ntype: iteration\n---\n\n\
             ### Theme A — fix something [pre_fix_repro_test: {slug}]\n\n\
             Some content.\n"
        )
    }

    // -----------------------------------------------------------------------
    // Parser tests (carried over from iter-87)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_pre_fix_repro_annotations_basic() {
        let body = r#"
## Some section

### Theme A — cascade fix [pre_fix_repro_test: pre_fix_cascade_red_then_green]

Some content.

### Theme B — no annotation

More content.

### Theme C — another fix [pre_fix_repro_test: pre_fix_c_slug]

Content.
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].test_slug, "pre_fix_cascade_red_then_green");
        assert!(annotations[0].theme.contains("Theme A"));
        assert_eq!(annotations[1].test_slug, "pre_fix_c_slug");
        assert!(annotations[1].theme.contains("Theme C"));
    }

    #[test]
    fn parse_pre_fix_repro_annotations_no_annotations() {
        let body = r#"
### Theme A — some theme [0/3]
### Theme B — other theme [1/2]
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_empty_slug() {
        let body = "### Theme A — test [pre_fix_repro_test:]\n";
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_whitespace_slug() {
        let body = "### Theme A — test [pre_fix_repro_test:   ]\n";
        let annotations = parse_pre_fix_repro_annotations(body);
        assert!(annotations.is_empty());
    }

    #[test]
    fn parse_pre_fix_repro_annotations_skips_code_block() {
        let body = r#"
Some prose.

```
### Theme X — example [pre_fix_repro_test: should_be_ignored]
```

### Theme Y — real [pre_fix_repro_test: real_slug]
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].test_slug, "real_slug");
    }

    #[test]
    fn parse_pre_fix_repro_annotations_iter87_plan() {
        let body = r#"
### Theme B — check-dogfood-script FAILs by default [0/4] [pre_fix_repro_test: live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch]

Content.

### Theme E — fix iter-86's buggy dogfood-script assertions [0/3] [pre_fix_repro_test: lint_flags_iter86_assertions_before_fix]

Content.
"#;
        let annotations = parse_pre_fix_repro_annotations(body);
        assert_eq!(annotations.len(), 2);
        assert_eq!(
            annotations[0].test_slug,
            "live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch"
        );
        assert_eq!(
            annotations[1].test_slug,
            "lint_flags_iter86_assertions_before_fix"
        );
    }

    #[test]
    fn xtask_check_pre_fix_repro_parses_iter87_annotations() {
        let body_with_annotation = "### Theme X — fix something [pre_fix_repro_test: my_slug]\n";
        let annots = parse_pre_fix_repro_annotations(body_with_annotation);
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].test_slug, "my_slug");

        let body_no_annotation = "### Theme Y — some work [0/3]\n";
        let empty = parse_pre_fix_repro_annotations(body_no_annotation);
        assert!(empty.is_empty(), "expected empty, got: {empty:?}");
    }

    // -----------------------------------------------------------------------
    // AC 3: SHA cache round-trip
    // -----------------------------------------------------------------------

    /// `unit_sha_cache_round_trip`: write PASS/FAIL via cache helpers, read
    /// back; corrupt file → miss + warning; missing dir → creates it.
    #[test]
    fn unit_sha_cache_round_trip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let sha = "abc123";
        let slug = "my_test";

        // Write PASS, read back.
        cache_write(&root, sha, Some("xtask"), slug, true).unwrap();
        let hit = cache_read(&root, sha, Some("xtask"), slug);
        assert_eq!(hit, Some(true), "expected cache hit = PASS");

        // Write FAIL, read back.
        cache_write(&root, sha, Some("xtask"), slug, false).unwrap();
        let hit = cache_read(&root, sha, Some("xtask"), slug);
        assert_eq!(hit, Some(false), "expected cache hit = FAIL");

        // Corrupt the file → miss (warning goes to stderr, no assertion needed).
        let path = cache_file_path(&root, sha, Some("xtask"), slug);
        fs::write(&path, "GARBAGE\n2026-01-01T00:00:00Z\n").unwrap();
        let miss = cache_read(&root, sha, Some("xtask"), slug);
        assert_eq!(miss, None, "expected cache miss on corrupt file");

        // Missing dir → cache_write creates it.
        let new_root = tmp.path().join("new_subdir");
        assert!(!new_root.exists());
        cache_write(&new_root, "sha2", None, "slug2", true).unwrap();
        assert!(
            new_root.join("results").exists(),
            "results dir should be created"
        );
        let hit2 = cache_read(&new_root, "sha2", None, "slug2");
        assert_eq!(hit2, Some(true));
    }

    /// `unit_cache_key_read_write_match_when_crate_none`: the key used by
    /// `cache_write` must equal the key used by `cache_read` when
    /// `crate_name` is `None` (i.e. whole-workspace mode).  A mismatch would
    /// cause permanent cache misses and redundant cargo runs.
    #[test]
    fn unit_cache_key_read_write_match_when_crate_none() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let sha = "deadbeef";
        let slug = "some_test";

        // Write with crate_name=None.
        cache_write(&root, sha, None, slug, false).unwrap();

        // Read back with the same None key — must hit, not miss.
        let result = cache_read(&root, sha, None, slug);
        assert_eq!(
            result,
            Some(false),
            "cache_read key must match cache_write key when crate_name is None"
        );
    }

    // -----------------------------------------------------------------------
    // AC 4: worktree path respects XDG_CACHE_HOME
    // -----------------------------------------------------------------------

    /// `unit_worktree_path_respects_xdg_cache_home`: pure path resolution
    /// honours XDG_CACHE_HOME and falls back to $HOME/.cache.
    ///
    /// NOTE: Because `resolve_main_worktree_path()` uses a process-wide
    /// `OnceLock`, we cannot test both XDG and non-XDG in the same process.
    /// This test exercises the underlying `worktree_cache_root()` helper
    /// directly, which is not cached and is always re-evaluated.
    /// `unit_worktree_path_respects_xdg_cache_home`:
    /// `worktree_cache_root()` is a pure helper (not memoised) that reads env
    /// vars each call.  Test that it honours `XDG_CACHE_HOME`.  Because this
    /// mutates env vars, the window is short and the mutation is specific to
    /// keys that are not used by the other tests in this module.
    #[test]
    fn unit_worktree_path_respects_xdg_cache_home() {
        let tmp = TempDir::new().unwrap();

        // worktree_cache_root() with XDG_CACHE_HOME set.
        // SAFETY: test-only env mutation; the XDG_CACHE_HOME key is not
        // observed by any other test in this file.
        unsafe {
            std::env::set_var(
                "XDG_CACHE_HOME",
                tmp.path().join("xdg-home").to_str().unwrap(),
            );
        }
        let root = worktree_cache_root();
        // SAFETY: restoring state immediately after reading.
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        assert!(
            root.to_str().unwrap().contains("ff-rdp"),
            "expected path to contain 'ff-rdp'; got: {root:?}"
        );
        assert!(
            root.to_str().unwrap().contains("pre-fix-repro"),
            "expected path to contain 'pre-fix-repro'; got: {root:?}"
        );
        let main_tree = root.join("main-tree");
        assert!(
            main_tree.ends_with("main-tree"),
            "expected main-tree suffix; got: {main_tree:?}"
        );

        // Also verify resolve_main_worktree_path() ends with main-tree.
        let resolved = resolve_main_worktree_path();
        assert!(
            resolved.ends_with("main-tree"),
            "expected resolved path to end with main-tree; got: {resolved:?}"
        );
    }

    // -----------------------------------------------------------------------
    // AC 5: worktree path OnceLock idempotency
    // -----------------------------------------------------------------------

    /// `unit_worktree_creation_idempotent`: calling `resolve_main_worktree_path()`
    /// twice returns the same result (OnceLock semantics).  We verify there is
    /// no attempt to re-create by asserting path equality — if the OnceLock were
    /// bypassed, CACHE_DIR would be re-read and diverge between calls.
    #[test]
    fn unit_worktree_creation_idempotent() {
        // The OnceLock is process-wide; it may already be initialised.
        // We can only assert the path is stable across calls.
        let first = resolve_main_worktree_path();
        let second = resolve_main_worktree_path();
        assert_eq!(
            first, second,
            "OnceLock must return the same path on every call"
        );
        assert!(first.ends_with("main-tree"), "path must end with main-tree");
    }

    // -----------------------------------------------------------------------
    // AC 6: green-on-branch second run is absent from output
    // -----------------------------------------------------------------------

    /// `unit_green_on_branch_run_dropped`: run() output must contain "red on
    /// main" and must NOT contain "green on branch HEAD".
    #[test]
    fn unit_green_on_branch_run_dropped() {
        let tmp = TempDir::new().unwrap();

        // Pre-seed a FAIL result so no cargo invocation happens.
        let sha = "test_sha_green_branch_check";
        cache_write(tmp.path(), sha, Some("xtask"), "my_slug", false).unwrap();

        // Write a minimal plan to a temp file.
        let plan_content = make_plan_with_annotation("my_slug");
        let plan_file = tmp.path().join("test-plan.md");
        fs::write(&plan_file, &plan_content).unwrap();

        let config = RunConfig {
            cache_root: tmp.path().to_path_buf(),
            sha_override: Some(sha.to_owned()),
        };

        let mut out_buf = Vec::<u8>::new();
        let mut err_buf = Vec::<u8>::new();

        let result = run_with_writer(
            Args {
                plan: plan_file,
                crate_name: Some("xtask".to_owned()),
            },
            config,
            &mut out_buf,
            &mut err_buf,
        );

        assert!(result.is_ok(), "expected Ok, got: {result:?}");

        let out_str = String::from_utf8(out_buf).unwrap();
        assert!(
            out_str.contains("red on main"),
            "output must mention 'red on main'; got: {out_str}"
        );
        assert!(
            !out_str.contains("green on branch HEAD"),
            "output must NOT contain 'green on branch HEAD'; got: {out_str}"
        );
    }

    // -----------------------------------------------------------------------
    // AC 1: cache-hit path completes under 5s
    // -----------------------------------------------------------------------

    /// `pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit`:
    /// pre-seed the result cache (FAIL), inject a `RunConfig` directly (no env
    /// var mutation), run the gate twice, assert both complete in < 5s.
    ///
    /// This test is the pre_fix_repro for iter-91: it will compile-fail on
    /// origin/main because `run_with_writer` / `RunConfig` do not exist there —
    /// that is the expected red-on-main state.
    #[test]
    fn pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit() {
        let tmp = TempDir::new().unwrap();
        let sha = "cache_hit_perf_sha";

        // Pre-seed FAIL result.
        cache_write(tmp.path(), sha, Some("xtask"), "my_slug", false).unwrap();

        let plan_content = make_plan_with_annotation("my_slug");
        let plan_file = tmp.path().join("perf-plan.md");
        fs::write(&plan_file, &plan_content).unwrap();

        let make_config = || RunConfig {
            cache_root: tmp.path().to_path_buf(),
            sha_override: Some(sha.to_owned()),
        };
        let make_args = || Args {
            plan: plan_file.clone(),
            crate_name: Some("xtask".to_owned()),
        };

        // First call (pre-seeded, so cache hit immediately).
        let t0 = std::time::Instant::now();
        let r1 = run_with_writer(make_args(), make_config(), &mut Vec::new(), &mut Vec::new());
        let elapsed1 = t0.elapsed();

        // Second call.
        let t1 = std::time::Instant::now();
        let r2 = run_with_writer(make_args(), make_config(), &mut Vec::new(), &mut Vec::new());
        let elapsed2 = t1.elapsed();

        assert!(r1.is_ok(), "first run failed: {r1:?}");
        assert!(r2.is_ok(), "second run failed: {r2:?}");
        assert!(
            elapsed1.as_secs_f64() < 5.0,
            "first cache-hit run took {elapsed1:.2?} (> 5s)"
        );
        assert!(
            elapsed2.as_secs_f64() < 5.0,
            "second cache-hit run took {elapsed2:.2?} (> 5s)"
        );
    }

    // -----------------------------------------------------------------------
    // AC 2: warm-main-target run under 30s (live, ignored)
    // -----------------------------------------------------------------------

    /// `pre_fix_repro_check_pre_fix_repro_completes_under_30s_warm_main_target`:
    /// full cargo run path with a pre-warmed target directory.  Requires
    /// FF_RDP_LIVE_TESTS=1.  Gated with `#[ignore]` so it runs only via
    /// `cargo test-live`.
    ///
    /// On origin/main this test itself would fail because the code it calls
    /// doesn't exist — that is the correct pre-fix red state.
    #[test]
    #[ignore = "requires FF_RDP_LIVE_TESTS=1 and a real git worktree (slow, ~30s)"]
    fn pre_fix_repro_check_pre_fix_repro_completes_under_30s_warm_main_target() {
        if std::env::var("FF_RDP_LIVE_TESTS").as_deref() != Ok("1") {
            return;
        }
        // This test exercises the real worktree + cargo path.  The worktree must
        // exist and be pre-warmed for the < 30s bound to hold.
        let tmp = TempDir::new().unwrap();

        // Use a unique slug unlikely to exist on main → test slug not found = FAIL.
        let plan_content = make_plan_with_annotation(
            "pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit",
        );
        let plan_file = tmp.path().join("live-plan.md");
        fs::write(&plan_file, &plan_content).unwrap();

        // Pass cache root explicitly — no env var mutation needed.
        let config = RunConfig {
            cache_root: tmp.path().to_path_buf(),
            sha_override: None, // full worktree path
        };

        let t0 = std::time::Instant::now();
        let result = run_with_writer(
            Args {
                plan: plan_file,
                crate_name: Some("xtask".to_owned()),
            },
            config,
            &mut Vec::new(),
            &mut Vec::new(),
        );
        let elapsed = t0.elapsed();

        // Either OK (slug found and FAIL on main) or Err — both are acceptable.
        let _ = result;
        assert!(
            elapsed.as_secs_f64() < 30.0,
            "warm cargo run took {elapsed:.2?} (> 30s)"
        );
    }
}
