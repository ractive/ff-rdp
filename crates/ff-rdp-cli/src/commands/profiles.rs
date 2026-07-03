//! `ff-rdp profiles {list,prune}` — inspect and clean up the ephemeral
//! Firefox profile directories ff-rdp creates for itself under
//! `secure_profile_root()` (iter-96 Theme C).
//!
//! Theme A (`daemon stop`) and Theme B (`launch`'s background pruning,
//! `crate::util::profile_dir`) already remove profile directories
//! automatically. This module is the manual escape hatch: `profiles list`
//! reports how much has accumulated, and `profiles prune` removes it on
//! demand — e.g. after a crash, a `kill -9`, or a long-running host where
//! Theme B's bounded per-launch pruning hasn't caught up.
//!
//! # Safety
//!
//! Every function here that touches disk is built on
//! [`crate::util::profile_dir::is_managed_profile_basename`] — the exact
//! matcher Theme A/B use — so a `--profile` directory the user passed to
//! `launch` themselves is never listed or removed, even if it happens to
//! live under the same root. See that module's doc comment for the full
//! threat model.
//!
//! # Testability
//!
//! The listing/aggregation/prune-selection logic takes `&Path` rather than
//! calling `secure_profile_root()` itself, so unit tests (and the `doctor`
//! `profile_disk_usage` check) can point it at a temp directory. Only the
//! thin `run_list`/`run_prune` wrappers resolve the real root.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;
use crate::util::profile_dir::{is_managed_profile_basename, secure_profile_root};

/// One managed `ff-rdp-profile-*` directory found directly under a profile
/// root, with just enough metadata for aggregation/pruning decisions.
struct ManagedProfileEntry {
    path: PathBuf,
    basename: String,
    mtime: Option<SystemTime>,
}

/// Scan the direct children of `root`, returning only entries that match the
/// managed-profile naming convention and are themselves directories.
///
/// Mirrors the tolerant, never-fail scanning style of
/// `prune_orphan_profiles`: a missing/unreadable root, or an unreadable
/// individual entry, is skipped rather than propagated — `profiles
/// list`/`profiles prune` must not fail just because the profile root
/// hasn't been created yet (e.g. `ff-rdp launch` has never run on this
/// machine).
fn scan_managed_profiles(root: &Path) -> Vec<ManagedProfileEntry> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(basename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_managed_profile_basename(basename) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        // `basename` borrows from `path`; own it before moving `path` into
        // the struct literal below.
        let basename = basename.to_owned();
        out.push(ManagedProfileEntry {
            path,
            basename,
            mtime: metadata.modified().ok(),
        });
    }
    out
}

/// Recursively sum the size in bytes of every regular file under `path` (a
/// single managed profile directory), stopping early once the running total
/// exceeds `cap` — callers that only need "is it bigger than X" (the doctor
/// check) must not pay for a full walk of a multi-GiB backlog.
///
/// Uses `walkdir` because a Firefox profile is a deep tree (places.sqlite,
/// storage/, cache2/, ...), not a flat directory — a plain `read_dir` would
/// only see the top level and drastically undercount.
fn dir_size_bytes_capped(path: &Path, cap: u64) -> u64 {
    let mut total: u64 = 0;
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        if let Ok(metadata) = entry.metadata() {
            total = total.saturating_add(metadata.len());
            if total > cap {
                break;
            }
        }
    }
    total
}

/// Aggregated view of every managed profile directory under a profile root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProfileListSummary {
    pub(crate) count: usize,
    pub(crate) total_size_bytes: u64,
    pub(crate) oldest_mtime: Option<SystemTime>,
}

/// Count, total on-disk size, and oldest mtime of every managed
/// `ff-rdp-profile-*` directory directly under `root`.
///
/// Used by `ff-rdp profiles list`, which reports exact totals.
pub(crate) fn aggregate_profiles(root: &Path) -> ProfileListSummary {
    aggregate_profiles_capped(root, u64::MAX)
}

/// Like [`aggregate_profiles`], but the size walk stops as soon as the
/// running total exceeds `byte_cap` — `total_size_bytes` is then a lower
/// bound, not an exact figure. `count` and `oldest_mtime` are always exact
/// (they only need top-level metadata, which is cheap).
///
/// Used by the `doctor` `profile_disk_usage` check
/// (`crate::commands::doctor`), which only needs to know whether the store
/// crossed its warn threshold — walking every file of a multi-GiB backlog
/// would make `ff-rdp doctor` appear hung on exactly the machines the check
/// exists to help.
pub(crate) fn aggregate_profiles_capped(root: &Path, byte_cap: u64) -> ProfileListSummary {
    let entries = scan_managed_profiles(root);
    let count = entries.len();
    let oldest_mtime = entries.iter().filter_map(|e| e.mtime).min();
    let mut total_size_bytes: u64 = 0;
    for entry in &entries {
        total_size_bytes = total_size_bytes.saturating_add(dir_size_bytes_capped(
            &entry.path,
            byte_cap.saturating_sub(total_size_bytes),
        ));
        if total_size_bytes > byte_cap {
            break;
        }
    }
    ProfileListSummary {
        count,
        total_size_bytes,
        oldest_mtime,
    }
}

/// Select the managed profile directories under `root` a prune should touch.
///
/// `older_than = None` means `--all`: age is ignored and every managed entry
/// is a candidate. `older_than = Some(threshold)` keeps only entries that are
/// stale by *both* signals `prune_orphan_profiles` uses — the directory's
/// own mtime AND its newest top-level file mtime
/// ([`crate::util::profile_dir::latest_profile_activity`]) — so a profile a
/// long-running Firefox is still writing into is never selected. An entry
/// whose mtime can't be read, or whose mtime is in the future (clock skew),
/// is treated as "not stale" and excluded — the same conservative default.
fn select_prune_targets(root: &Path, older_than: Option<Duration>) -> Vec<ManagedProfileEntry> {
    let now = SystemTime::now();
    scan_managed_profiles(root)
        .into_iter()
        .filter(|entry| match older_than {
            None => true,
            Some(threshold) => {
                let dir_stale = entry
                    .mtime
                    .and_then(|mtime| now.duration_since(mtime).ok())
                    .is_some_and(|age| age >= threshold);
                dir_stale
                    && entry.mtime.is_some_and(|mtime| {
                        let newest =
                            crate::util::profile_dir::latest_profile_activity(&entry.path, mtime);
                        now.duration_since(newest)
                            .ok()
                            .is_some_and(|age| age >= threshold)
                    })
            }
        })
        .collect()
}

/// Outcome of a [`prune_profiles`] call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PruneOutcome {
    /// Basenames a dry run would remove. Empty on a real run.
    pub(crate) would_remove: Vec<String>,
    /// Basenames actually removed. Empty on a dry run.
    pub(crate) removed: Vec<String>,
}

/// Prune managed profile directories under `root`.
///
/// `dry_run = true` only selects and reports candidates via `would_remove`;
/// nothing is touched on disk, and the candidate directories still exist
/// afterwards. `dry_run = false` removes each candidate with
/// `remove_dir_all`; a per-entry failure (permission error, directory
/// vanishing mid-scan because of a concurrent prune, ...) is logged at
/// `warn` and skipped rather than aborting the rest of the batch — the same
/// warn-and-continue tolerance `prune_orphan_profiles` and
/// `cleanup_profile_dir` use.
pub(crate) fn prune_profiles(
    root: &Path,
    older_than: Option<Duration>,
    dry_run: bool,
) -> PruneOutcome {
    let targets = select_prune_targets(root, older_than);

    if dry_run {
        return PruneOutcome {
            would_remove: targets.into_iter().map(|e| e.basename).collect(),
            removed: Vec::new(),
        };
    }

    let mut removed = Vec::new();
    for entry in targets {
        match std::fs::remove_dir_all(&entry.path) {
            Ok(()) => removed.push(entry.basename),
            Err(e) => {
                tracing::warn!(
                    "profiles prune: failed to remove {}: {e}",
                    entry.path.display()
                );
            }
        }
    }
    PruneOutcome {
        would_remove: Vec::new(),
        removed,
    }
}

/// Parse a `--older-than` value: `<N>d`, `<N>h`, `<N>m`, `<N>s`, or a bare
/// number of seconds.
///
/// No external duration-parsing crate is pulled in for this — the accepted
/// grammar is intentionally tiny, so hand-rolled parsing is clearer than a
/// dependency.
pub(crate) fn parse_older_than(input: &str) -> Result<Duration, AppError> {
    let s = input.trim();
    let invalid = || {
        AppError::User(format!(
            "--older-than: invalid duration {input:?} — expected e.g. '7d', '12h', \
             '30m', '45s', or a bare number of seconds"
        ))
    };

    let (digits, multiplier) = match s.as_bytes().last() {
        Some(b'd') => (&s[..s.len() - 1], 86_400u64),
        Some(b'h') => (&s[..s.len() - 1], 3_600),
        Some(b'm') => (&s[..s.len() - 1], 60),
        Some(b's') => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };

    let n: u64 = digits.parse().map_err(|_| invalid())?;
    Ok(Duration::from_secs(n.saturating_mul(multiplier)))
}

// ---------------------------------------------------------------------------
// Thin command wrappers — resolve `secure_profile_root()`, delegate to the
// pure-`&Path` core above, and emit the standard JSON envelope.
// ---------------------------------------------------------------------------

/// `ff-rdp profiles list`.
pub fn run_list(cli: &Cli) -> Result<(), AppError> {
    let root = secure_profile_root()?;
    let summary = aggregate_profiles(&root);
    let oldest_mtime = summary
        .oldest_mtime
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

    let results = json!({
        "path": root.display().to_string(),
        "count": summary.count,
        "total_size_bytes": summary.total_size_bytes,
        "oldest_mtime": oldest_mtime,
    });

    let envelope = output::envelope(&results, 1, &json!({}));
    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// `ff-rdp profiles prune`.
pub fn run_prune(cli: &Cli, older_than: &str, all: bool, dry_run: bool) -> Result<(), AppError> {
    let root = secure_profile_root()?;
    let threshold = if all {
        None
    } else {
        Some(parse_older_than(older_than)?)
    };
    let outcome = prune_profiles(&root, threshold, dry_run);

    let total = if dry_run {
        outcome.would_remove.len()
    } else {
        outcome.removed.len()
    };
    let results = json!({
        "path": root.display().to_string(),
        "would_remove": outcome.would_remove,
        "removed": outcome.removed,
        "dry_run": dry_run,
    });

    let envelope = output::envelope(&results, total, &json!({}));
    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a fake managed profile dir `ff-rdp-profile-<suffix>` under
    /// `root`, containing one file of `file_bytes` length, and back-date both
    /// the directory mtime AND the payload file mtime by `age` — an orphan's
    /// files are as old as the directory itself; a fresh inner file would
    /// (correctly) make `select_prune_targets`'s activity check treat the
    /// profile as live. `suffix` must be exactly 16 alphanumeric characters
    /// to satisfy `is_managed_profile_basename`.
    fn seed_profile(root: &Path, suffix: &str, file_bytes: usize, age: Duration) -> PathBuf {
        assert_eq!(suffix.len(), 16, "test fixture suffix must be 16 chars");
        let dir = root.join(format!("ff-rdp-profile-{suffix}"));
        std::fs::create_dir_all(&dir).expect("create fake profile dir");
        let payload = dir.join("payload.bin");
        std::fs::write(&payload, vec![0u8; file_bytes]).expect("write fake payload file");
        let mtime = SystemTime::now()
            .checked_sub(age)
            .expect("age fits before now");
        let ft = filetime::FileTime::from_system_time(mtime);
        filetime::set_file_mtime(&payload, ft).expect("set_file_mtime payload");
        filetime::set_file_mtime(&dir, ft).expect("set_file_mtime dir");
        dir
    }

    // -----------------------------------------------------------------
    // aggregate_profiles / `profiles list`
    // -----------------------------------------------------------------

    /// AC: `unit_profiles_list_aggregates_size_and_count_correctly` — two
    /// managed profile dirs with known payload sizes (one with a nested
    /// subdirectory, to exercise the recursive `walkdir` size walk) sum to
    /// the expected `count`/`total_size_bytes`.
    #[test]
    fn unit_profiles_list_aggregates_size_and_count_correctly() {
        let root = tempfile::tempdir().expect("tempdir");
        let fresh = Duration::from_secs(0);
        seed_profile(root.path(), &"a".repeat(16), 100, fresh);
        let dir2 = seed_profile(root.path(), &"b".repeat(16), 250, fresh);
        // Nested subdirectory under dir2 — walkdir must recurse into it.
        std::fs::create_dir_all(dir2.join("storage")).expect("create nested dir");
        std::fs::write(dir2.join("storage").join("nested.bin"), vec![0u8; 50])
            .expect("write nested file");

        let summary = aggregate_profiles(root.path());

        assert_eq!(summary.count, 2, "expected exactly 2 managed profile dirs");
        assert_eq!(
            summary.total_size_bytes, 400,
            "100 + 250 + 50 (nested) = 400 bytes"
        );
        assert!(summary.oldest_mtime.is_some());
    }

    /// `aggregate_profiles_capped` stops the size walk once the cap is
    /// crossed but still reports the exact entry count. Each seeded dir
    /// holds a single 100-byte file, so regardless of walk order the total
    /// is 100 (≤ cap, keep going) then 200 (> cap, stop) — the third dir is
    /// never walked.
    #[test]
    fn unit_aggregate_profiles_capped_stops_early_but_counts_all() {
        let root = tempfile::tempdir().expect("tempdir");
        let fresh = Duration::from_secs(0);
        seed_profile(root.path(), &"d".repeat(16), 100, fresh);
        seed_profile(root.path(), &"e".repeat(16), 100, fresh);
        seed_profile(root.path(), &"f".repeat(16), 100, fresh);

        let summary = aggregate_profiles_capped(root.path(), 150);

        assert_eq!(summary.count, 3, "count must stay exact under a cap");
        assert_eq!(
            summary.total_size_bytes, 200,
            "walk must stop at the first total exceeding the 150-byte cap"
        );
        assert!(summary.oldest_mtime.is_some());
    }

    #[test]
    fn unit_profiles_list_empty_root_reports_zero() {
        let root = tempfile::tempdir().expect("tempdir");
        let summary = aggregate_profiles(root.path());
        assert_eq!(summary.count, 0);
        assert_eq!(summary.total_size_bytes, 0);
        assert!(summary.oldest_mtime.is_none());
    }

    #[test]
    fn unit_profiles_list_missing_root_does_not_panic() {
        let root = tempfile::tempdir().expect("tempdir");
        let missing = root.path().join("does-not-exist");
        let summary = aggregate_profiles(&missing);
        assert_eq!(summary.count, 0);
    }

    #[test]
    fn unit_profiles_list_ignores_unmanaged_directories() {
        let root = tempfile::tempdir().expect("tempdir");
        seed_profile(root.path(), &"c".repeat(16), 10, Duration::from_secs(0));
        std::fs::create_dir_all(root.path().join("not-a-profile")).expect("create dir");

        let summary = aggregate_profiles(root.path());

        assert_eq!(
            summary.count, 1,
            "the non-managed sibling directory must not be counted"
        );
    }

    // -----------------------------------------------------------------
    // prune_profiles / `profiles prune`
    // -----------------------------------------------------------------

    /// AC: `pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans` —
    /// a dry-run prune reports every seeded managed profile dir in
    /// `would_remove`, and none of them are actually deleted.
    #[test]
    fn pre_fix_repro_profiles_subcommand_prune_dry_run_lists_orphans() {
        let root = tempfile::tempdir().expect("tempdir");
        let eight_days = Duration::from_hours(192);
        let seeded: Vec<PathBuf> = (0..3)
            .map(|i| seed_profile(root.path(), &format!("{i:016}"), 0, eight_days))
            .collect();

        let outcome = prune_profiles(root.path(), Some(Duration::from_hours(168)), true);

        assert_eq!(outcome.removed.len(), 0, "dry run must not remove anything");
        assert_eq!(outcome.would_remove.len(), 3);
        for dir in &seeded {
            let basename = dir.file_name().unwrap().to_str().unwrap();
            assert!(
                outcome.would_remove.contains(&basename.to_owned()),
                "would_remove must list {basename}"
            );
            assert!(
                dir.exists(),
                "{} must still exist after a dry-run prune",
                dir.display()
            );
        }
    }

    #[test]
    fn unit_profiles_prune_real_run_removes_selected_and_leaves_others() {
        let root = tempfile::tempdir().expect("tempdir");
        let old_dir = seed_profile(root.path(), &"a".repeat(16), 0, Duration::from_hours(192));
        let fresh_dir = seed_profile(root.path(), &"b".repeat(16), 0, Duration::from_mins(1));

        let outcome = prune_profiles(root.path(), Some(Duration::from_hours(168)), false);

        assert_eq!(
            outcome.would_remove.len(),
            0,
            "real run leaves would_remove empty"
        );
        assert_eq!(outcome.removed.len(), 1);
        assert!(!old_dir.exists(), "stale dir should be removed");
        assert!(fresh_dir.exists(), "fresh dir should survive");
    }

    /// A directory whose own mtime is stale but which contains a
    /// recently-written top-level file (the signature of a live Firefox
    /// session — content rewrites bump file mtimes, not the parent dir's)
    /// must NOT be selected by an age-gated prune.
    #[test]
    fn unit_profiles_prune_age_gated_skips_profile_with_fresh_inner_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = seed_profile(root.path(), &"g".repeat(16), 10, Duration::from_hours(192));
        // Simulate live-session activity: refresh one inner file's mtime,
        // then re-backdate the directory itself (the write bumps it).
        std::fs::write(dir.join("prefs.js"), b"user_pref!").expect("write fresh inner file");
        let stale = SystemTime::now()
            .checked_sub(Duration::from_hours(192))
            .expect("age fits before now");
        filetime::set_file_mtime(&dir, filetime::FileTime::from_system_time(stale))
            .expect("re-backdate dir mtime");

        let outcome = prune_profiles(root.path(), Some(Duration::from_hours(168)), false);

        assert_eq!(
            outcome.removed.len(),
            0,
            "a profile with fresh top-level file activity must survive an age-gated prune"
        );
        assert!(dir.exists(), "{} must survive", dir.display());
    }

    #[test]
    fn unit_profiles_prune_all_ignores_age() {
        let root = tempfile::tempdir().expect("tempdir");
        let fresh_dir = seed_profile(root.path(), &"c".repeat(16), 0, Duration::from_secs(1));

        let outcome = prune_profiles(root.path(), None, false);

        assert_eq!(
            outcome.removed,
            vec!["ff-rdp-profile-cccccccccccccccc".to_owned()]
        );
        assert!(
            !fresh_dir.exists(),
            "--all must remove even a freshly-created managed dir"
        );
    }

    #[test]
    fn unit_profiles_prune_never_touches_unmanaged_directories() {
        let root = tempfile::tempdir().expect("tempdir");
        let unmanaged = root.path().join("some-other-dir");
        std::fs::create_dir_all(&unmanaged).expect("create dir");
        filetime::set_file_mtime(
            &unmanaged,
            filetime::FileTime::from_system_time(
                SystemTime::now()
                    .checked_sub(Duration::from_hours(720))
                    .unwrap(),
            ),
        )
        .expect("set_file_mtime");

        let outcome = prune_profiles(root.path(), None, false);

        assert!(outcome.removed.is_empty());
        assert!(
            unmanaged.exists(),
            "a directory not matching the naming convention must never be removed, even with --all"
        );
    }

    // -----------------------------------------------------------------
    // parse_older_than
    // -----------------------------------------------------------------

    #[test]
    fn unit_parse_older_than_accepts_days_hours_minutes_seconds_and_bare_number() {
        assert_eq!(parse_older_than("7d").unwrap(), Duration::from_hours(168));
        assert_eq!(parse_older_than("12h").unwrap(), Duration::from_hours(12));
        assert_eq!(parse_older_than("30m").unwrap(), Duration::from_mins(30));
        assert_eq!(parse_older_than("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_older_than("3600").unwrap(), Duration::from_hours(1));
    }

    #[test]
    fn unit_parse_older_than_rejects_garbage() {
        let err = parse_older_than("banana").expect_err("must reject a non-numeric duration");
        match err {
            AppError::User(msg) => assert!(msg.contains("invalid duration")),
            other => panic!("expected AppError::User, got {other:?}"),
        }
    }
}
