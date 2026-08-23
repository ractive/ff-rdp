//! Lightweight single-file record shared by `launch` and `daemon stop`.
//!
//! Every Firefox instance spawned by ff-rdp (whether via `launch` or
//! `daemon start`) writes one [`DaemonRecord`] to a well-known cache file.
//! `daemon stop` reads that record to find the PID and port regardless of
//! which subcommand launched Firefox.
//!
//! ## File location
//!
//! `~/.ff-rdp/launch-record.<port>.json` on all platforms — shares the same
//! parent directory as the per-port proxy-daemon registry files
//! (`~/.ff-rdp/daemon.<port>.json`, iter-123 Theme B) so a single
//! `FF_RDP_HOME` cleanup wipes all ff-rdp state. The file name differs to
//! avoid colliding with the existing registry files.
//!
//! ## Per-port scoping (iter-142 Theme A)
//!
//! Prior to iter-142 this was a single global `launch-record.json` shared by
//! every `launch`/`daemon start` invocation on the machine. Two concurrent
//! agents launching Firefox on different ports (e.g. 6100 and 6101) would
//! clobber each other's record: the second `launch` overwrote the first's
//! entry, so a subsequent `daemon stop --port 6100` read back port 6101's
//! PID, found it didn't match, and fell through to the proxy-daemon registry
//! path — which reports the *daemon's own* PID, not Firefox's (see
//! `daemon/client.rs::run_daemon_stop`). Dogfooding session 63 reproduced
//! this 3/3 with four parallel agents on separate ports. Scoping the record
//! filename by port (mirroring the iter-123 Theme B fix for the proxy
//! registry) makes concurrent instances independent.
//!
//! The `FF_RDP_HOME` env-var overrides the home directory (same convention as
//! `daemon/registry.rs`): when set, the file is written to
//! `$FF_RDP_HOME/.ff-rdp/launch-record.json`.
//!
//! ## Staleness and reclamation (iter-186)
//!
//! [`read`] / [`read_in`] perform a PID-liveness check on every read.
//! If the recorded PID is no longer running the record is treated as absent
//! and the file is removed so the stale entry does not block a future launch.
//!
//! That read-triggered removal is *not* a garbage collector, and iter-142's
//! per-port split turned that into a leak: one file per launch, nothing
//! sweeping them (4040 files / 17 MB measured on the dev machine over ten
//! days). [`gc_stale_launch_records_in`] is the actual reclamation path,
//! swept on every `launch`; its section comment explains why keying
//! reclamation on the port could never work.
//!
//! ## Atomicity
//!
//! Writes use a write-to-temp + rename strategy identical to the daemon
//! registry, guaranteeing that readers never see a partially written file.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::daemon::process;

// ---------------------------------------------------------------------------
// Record type
// ---------------------------------------------------------------------------

/// State persisted to disk whenever ff-rdp spawns Firefox.
///
/// Shared between `launch` (which writes it) and `daemon stop` / `launch
/// --replace` (which read it). The record lets `daemon stop` terminate
/// instances that were started with `launch` rather than `daemon start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRecord {
    pub pid: u32,
    pub port: u16,
    pub headless: bool,
    pub launched_at: DateTime<Utc>,
    pub profile_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Directory resolution
// ---------------------------------------------------------------------------

/// Return the per-port filename used for the launch record
/// (e.g. `launch-record.6100.json`), sharing `~/.ff-rdp/` with the per-port
/// proxy-daemon registry files (`daemon.<port>.json`).
fn record_filename(port: u16) -> String {
    format!("launch-record.{port}.json")
}

/// Return the directory that contains the launch-record file.
///
/// Respects `FF_RDP_HOME` for test isolation (same convention as
/// `daemon/registry.rs`):
/// - If set: `$FF_RDP_HOME/.ff-rdp/`
/// - Otherwise: `$HOME/.ff-rdp/` via `dirs::home_dir()`.
pub fn record_base_dir() -> Result<PathBuf> {
    let home = match std::env::var_os("FF_RDP_HOME") {
        Some(h) => PathBuf::from(h),
        None => dirs::home_dir().context("could not determine home directory")?,
    };
    Ok(home.join(".ff-rdp"))
}

// ---------------------------------------------------------------------------
// Test-injectable base-dir variants
// ---------------------------------------------------------------------------

/// Read the daemon record for `port` from `<dir>/launch-record.<port>.json`.
///
/// Returns `None` if the file is absent or if the recorded PID is no longer
/// alive (stale entry). When a stale entry is detected the file is removed
/// so it cannot interfere with future launches.
pub fn read_in(dir: &Path, port: u16) -> Result<Option<DaemonRecord>> {
    let path = dir.join(record_filename(port));
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading daemon record at {}", path.display()))?;
    let rec: DaemonRecord = serde_json::from_str(&contents)
        .with_context(|| format!("parsing daemon record at {}", path.display()))?;

    // Staleness check: if PID is dead, treat as absent and remove the file.
    if !process::is_process_alive(rec.pid) {
        let _ = fs::remove_file(&path);
        return Ok(None);
    }

    Ok(Some(rec))
}

/// Write `rec` to `<dir>/launch-record.<rec.port>.json` atomically
/// (write-to-tmp + rename).
pub fn write_in(dir: &Path, rec: &DaemonRecord) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating daemon record directory {}", dir.display()))?;

    let filename = record_filename(rec.port);
    let record_path = dir.join(&filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    let json = serde_json::to_string_pretty(rec).context("serializing DaemonRecord to JSON")?;

    let mut opts = fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }

    let mut tmp_file = opts
        .open(&tmp_path)
        .with_context(|| format!("opening tmp file {}", tmp_path.display()))?;
    tmp_file
        .write_all(json.as_bytes())
        .with_context(|| format!("writing to tmp file {}", tmp_path.display()))?;
    tmp_file
        .flush()
        .with_context(|| format!("flushing tmp file {}", tmp_path.display()))?;
    drop(tmp_file);

    fs::rename(&tmp_path, &record_path).with_context(|| {
        format!(
            "renaming {} -> {}",
            tmp_path.display(),
            record_path.display()
        )
    })?;

    Ok(())
}

/// Remove `<dir>/launch-record.<port>.json` if it exists (idempotent).
pub fn remove_in(dir: &Path, port: u16) -> Result<()> {
    let path = dir.join(record_filename(port));
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing daemon record {}", path.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public convenience wrappers using the real cache directory
// ---------------------------------------------------------------------------

/// Read the daemon record for `port` from the default cache location.
///
/// Returns `None` if absent or if the recorded PID is dead (stale).
pub fn read(port: u16) -> Result<Option<DaemonRecord>> {
    read_in(&record_base_dir()?, port)
}

/// Write the daemon record to the default cache location atomically, keyed
/// by `rec.port`.
pub fn write(rec: &DaemonRecord) -> Result<()> {
    write_in(&record_base_dir()?, rec)
}

/// Remove the daemon record for `port` from the default cache location
/// (idempotent).
pub fn remove(port: u16) -> Result<()> {
    remove_in(&record_base_dir()?, port)
}

// ---------------------------------------------------------------------------
// Stale launch-record GC (iter-186)
// ---------------------------------------------------------------------------
//
// ## Why the lazy, read-triggered reaper never fired
//
// [`read_in`] does delete a record whose pid is dead — but only as a side
// effect of *reading that same port again*. Ports come from an ephemeral
// `bind(127.0.0.1:0)`, so the chance that a later run asks for the exact port
// a dead record is filed under is negligible. Measured on the dev machine
// 2026-08-23: 4040 `launch-record.*.json` files spanning ten days, with
// essentially no repeated port numbers among them. A lazy reaper keyed on a
// value that never recurs is not a slow reaper; it is not a reaper. The file
// count could only ever go up.
//
// So: do not reintroduce reclamation keyed on the port. Reclamation has to be
// a sweep over the whole directory, driven by an event that actually happens
// (every `launch`) — which is exactly what iter-142 did for
// `daemon.<port>.throttle.json` after the same mistake, and what
// [`gc_stale_launch_records_in`] below does for launch records.
//
// ## What "stale" means here, and the pid-recycling risk
//
// Stale == the recorded pid is not alive. No age gate, matching iter-142's
// precedent ("a dead-owner profile is reclaimed immediately regardless of
// age") and `throttle_state::gc_stale_throttle_states_in`: a record for a
// dead process is dead weight the instant that process exits, and 142 already
// established that an age gate is precisely what stops a same-day workload
// from ever being reclaimed.
//
// Pid recycling can push the liveness check either way, and the two
// directions are not symmetric:
//
// - A recycled pid can make a *stale* record look live. It then survives this
//   sweep and is collected by a later one. Cost: one 212-byte file, for one
//   more launch. Harmless.
// - The dangerous direction — removing a *live* daemon's record — needs
//   `is_process_alive` to report false for a running process, which it does
//   not do (`kill(pid, 0)` on unix, `OpenProcess` on Windows). Pid recycling
//   cannot produce it.
//
// That asymmetry is why pid liveness alone is a safe test here; it is the same
// argument `registry::spawn_lock_is_stale` makes for spawn locks.

/// Parse the Firefox port out of a `launch-record.<PORT>.json` filename.
///
/// Returns `None` for everything else that shares `~/.ff-rdp/` —
/// `daemon.<port>.json`, `daemon.<port>.throttle.json`,
/// `daemon.<port>.spawn.lock`, `daemon.log` — and, importantly, for the
/// `launch-record.<port>.json.tmp` files [`write_in`] renames from: a `.tmp`
/// belongs to a write that is in flight right now and must never be swept.
fn parse_launch_record_port(filename: &str) -> Option<u16> {
    filename
        .strip_prefix("launch-record.")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

/// Sweep `<dir>/launch-record.*.json` files whose recorded pid is no longer
/// alive, so `~/.ff-rdp/` stops growing by one file per launch.
///
/// Mirrors the shape of `throttle_state::gc_stale_throttle_states_in` and is
/// deliberately narrow and defensive:
///
/// - Only exact `launch-record.<PORT>.json` filenames are candidates (see
///   [`parse_launch_record_port`]).
/// - A file that cannot be read or parsed is left alone rather than removed:
///   an I/O or parse error is not proof of staleness.
/// - Every error is swallowed. This is opportunistic housekeeping riding along
///   on the `launch` path; it must never become a reason a launch fails.
///
/// The removal is done here rather than by delegating to [`read_in`] so the
/// sweep reads each file exactly once, by path, and does not depend on
/// `read_in` keeping its stale-entry side effect.
pub(crate) fn gc_stale_launch_records_in(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if parse_launch_record_port(&name).is_none() {
            continue;
        }
        let path = entry.path();
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(rec) = serde_json::from_str::<DaemonRecord>(&contents) else {
            continue;
        };
        if !process::is_process_alive(rec.pid) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// [`gc_stale_launch_records_in`] against the real record directory.
///
/// Uses [`record_base_dir`], so the sweep honours `FF_RDP_HOME` exactly the
/// way the writer does and a test can point both at a tempdir. (Note that
/// `util::profile_dir::secure_profile_root` does *not* honour `FF_RDP_HOME` —
/// an inconsistency recorded by iteration 188, not changed here.)
pub(crate) fn gc_stale_launch_records() {
    if let Ok(dir) = record_base_dir() {
        gc_stale_launch_records_in(&dir);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> DaemonRecord {
        DaemonRecord {
            // Use the current process PID so the liveness check passes.
            pid: std::process::id(),
            port: 6000,
            headless: true,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/ff-rdp-test-profile"),
        }
    }

    /// Serialize → deserialize round-trip preserves all fields.
    #[test]
    fn unit_daemon_record_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rec = sample_record();
        let original_launched_at = rec.launched_at;

        write_in(dir.path(), &rec).expect("write_in");

        let read_back = read_in(dir.path(), rec.port)
            .expect("read_in ok")
            .expect("should be Some");

        assert_eq!(read_back.pid, rec.pid, "pid round-trip");
        assert_eq!(read_back.port, rec.port, "port round-trip");
        assert_eq!(read_back.headless, rec.headless, "headless round-trip");
        assert_eq!(
            read_back.launched_at, original_launched_at,
            "launched_at round-trip"
        );
        assert_eq!(
            read_back.profile_dir, rec.profile_dir,
            "profile_dir round-trip"
        );
    }

    /// A record with a dead PID returns None and removes the file.
    #[test]
    fn unit_daemon_record_stale_pid_returns_none_and_removes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rec = DaemonRecord {
            // PID 999_999_999 is astronomically unlikely to exist.
            pid: 999_999_999,
            port: 6001,
            headless: false,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/stale"),
        };

        // Bypass the normal write_in (which doesn't check PID) by writing JSON directly.
        let path = dir.path().join(record_filename(rec.port));
        let json = serde_json::to_string_pretty(&rec).unwrap();
        fs::write(&path, json).expect("write stale record");
        assert!(path.exists(), "file must exist before read_in");

        // read_in should detect the dead PID and return None.
        let result = read_in(dir.path(), rec.port).expect("read_in ok");
        assert!(result.is_none(), "stale PID must return None");

        // The file must have been removed.
        assert!(!path.exists(), "stale record file must be removed");
    }

    /// No `.tmp` file left behind after a successful write.
    #[test]
    fn unit_daemon_record_atomic_write_no_tmp_left_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rec = sample_record();
        write_in(dir.path(), &rec).expect("write_in");

        let tmp = dir
            .path()
            .join(format!("{}.tmp", record_filename(rec.port)));
        assert!(
            !tmp.exists(),
            ".tmp file must not remain after atomic write"
        );
    }

    /// `read_in` returns None when the directory is empty.
    #[test]
    fn unit_daemon_record_read_absent_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = read_in(dir.path(), 6000).expect("read_in ok");
        assert!(result.is_none());
    }

    /// `remove_in` is idempotent (does not error on missing file).
    #[test]
    fn unit_daemon_record_remove_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        remove_in(dir.path(), 6000).expect("remove when absent must not error");

        let rec = sample_record();
        write_in(dir.path(), &rec).expect("write");
        remove_in(dir.path(), rec.port).expect("remove when present must not error");
        assert!(!dir.path().join(record_filename(rec.port)).exists());
    }

    /// Overwriting the same port with a second write replaces the first.
    #[test]
    fn unit_daemon_record_overwrite_replaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_in(dir.path(), &sample_record()).expect("first write");

        let updated = DaemonRecord {
            pid: std::process::id(),
            port: sample_record().port,
            headless: false,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/updated"),
        };
        write_in(dir.path(), &updated).expect("second write");

        let read_back = read_in(dir.path(), updated.port)
            .expect("read_in ok")
            .expect("Some");
        assert_eq!(read_back.profile_dir, PathBuf::from("/tmp/updated"));
    }

    /// AC `unit_daemon_record_per_port_isolation` (iter-142 Theme A): two
    /// records for different ports must not clobber each other — this is
    /// the exact defect dogfooding session 63 reproduced 3/3 with parallel
    /// agents on separate ports (a single global `launch-record.json` meant
    /// the second `launch` silently overwrote the first's entry).
    #[test]
    fn unit_daemon_record_per_port_isolation() {
        let dir = tempfile::tempdir().expect("tempdir");

        let rec_a = DaemonRecord {
            pid: std::process::id(),
            port: 6100,
            headless: true,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/profile-a"),
        };
        let rec_b = DaemonRecord {
            pid: std::process::id(),
            port: 6101,
            headless: true,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/profile-b"),
        };

        write_in(dir.path(), &rec_a).expect("write a");
        write_in(dir.path(), &rec_b).expect("write b");

        let read_a = read_in(dir.path(), 6100)
            .expect("read a ok")
            .expect("a present");
        let read_b = read_in(dir.path(), 6101)
            .expect("read b ok")
            .expect("b present");

        assert_eq!(read_a.profile_dir, PathBuf::from("/tmp/profile-a"));
        assert_eq!(read_b.profile_dir, PathBuf::from("/tmp/profile-b"));

        // Removing port 6100's record must leave port 6101's untouched.
        remove_in(dir.path(), 6100).expect("remove a");
        assert!(
            read_in(dir.path(), 6100)
                .expect("read a after remove")
                .is_none(),
            "port 6100 record must be gone"
        );
        assert!(
            read_in(dir.path(), 6101)
                .expect("read b after remove a")
                .is_some(),
            "port 6101 record must survive removing port 6100's record"
        );
    }

    // -----------------------------------------------------------------------
    // Stale launch-record GC (iter-186)
    // -----------------------------------------------------------------------

    /// A PID astronomically unlikely to exist — the same sentinel the
    /// stale-read test above uses.
    const DEAD_PID: u32 = 999_999_999;

    /// Write a launch record straight to disk, bypassing [`write_in`]'s
    /// (nonexistent) liveness check — needed to plant a record for a pid that
    /// is already dead.
    fn plant_record(dir: &Path, port: u16, pid: u32) {
        let rec = DaemonRecord {
            pid,
            port,
            headless: true,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/ff-rdp-planted"),
        };
        fs::write(
            dir.join(record_filename(port)),
            serde_json::to_string_pretty(&rec).expect("serialize planted record"),
        )
        .expect("write planted record");
    }

    fn launch_record_count(dir: &Path) -> usize {
        fs::read_dir(dir)
            .expect("read_dir")
            .flatten()
            .filter(|e| {
                e.file_name()
                    .into_string()
                    .ok()
                    .and_then(|n| parse_launch_record_port(&n))
                    .is_some()
            })
            .count()
    }

    /// Task A: the sweep removes a record whose owning pid is dead.
    /// Task B: a record whose pid is alive (this very test process) is never
    /// removed — the failure mode that would break `daemon stop` for a
    /// running instance.
    #[test]
    fn unit_gc_stale_launch_records_removes_dead_keeps_live() {
        let dir = tempfile::tempdir().expect("tempdir");

        plant_record(dir.path(), 6000, DEAD_PID);
        plant_record(dir.path(), 6001, std::process::id());

        gc_stale_launch_records_in(dir.path());

        assert!(
            !dir.path().join(record_filename(6000)).exists(),
            "a launch record whose pid is dead must be collected by the sweep"
        );
        assert!(
            dir.path().join(record_filename(6001)).exists(),
            "a launch record whose pid is alive must survive the sweep"
        );
    }

    /// The sweep's filename match is scoped exactly to
    /// `launch-record.<PORT>.json`. Everything else sharing `~/.ff-rdp/` is
    /// left untouched — including `write_in`'s `.tmp`, which belongs to a
    /// write that may be in flight right now.
    #[test]
    fn unit_gc_stale_launch_records_ignores_other_files() {
        let dir = tempfile::tempdir().expect("tempdir");

        fs::write(dir.path().join("daemon.6000.json"), b"{}").expect("registry file");
        fs::write(dir.path().join("daemon.6000.throttle.json"), b"{}").expect("throttle file");
        fs::write(dir.path().join("daemon.6000.spawn.lock"), []).expect("lock file");
        fs::write(dir.path().join("daemon.log"), b"log").expect("log file");
        // The pre-iter-142 port-less name: not a `<PORT>` shape, so it is not
        // a candidate. Left for a human rather than silently deleted.
        fs::write(dir.path().join("launch-record.json"), b"{}").expect("legacy record");
        fs::write(
            dir.path().join(format!("{}.tmp", record_filename(6000))),
            b"{}",
        )
        .expect("tmp file");

        gc_stale_launch_records_in(dir.path());

        for name in [
            "daemon.6000.json",
            "daemon.6000.throttle.json",
            "daemon.6000.spawn.lock",
            "daemon.log",
            "launch-record.json",
            "launch-record.6000.json.tmp",
        ] {
            assert!(
                dir.path().join(name).exists(),
                "{name} must survive the launch-record sweep untouched"
            );
        }
    }

    /// A record that cannot be parsed is left alone: an I/O or parse error is
    /// not proof of staleness, and deleting on "I couldn't read it" would make
    /// the sweep destructive under conditions it cannot distinguish.
    #[test]
    fn unit_gc_stale_launch_records_leaves_corrupt_record_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(record_filename(6000));
        fs::write(&path, b"{ this is not json").expect("write corrupt record");

        gc_stale_launch_records_in(dir.path());

        assert!(
            path.exists(),
            "an unparsable launch record must be left alone, not deleted"
        );
    }

    /// Housekeeping must never fail its caller: a missing directory is a
    /// silent no-op, not an error.
    #[test]
    fn unit_gc_stale_launch_records_tolerates_missing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        gc_stale_launch_records_in(&dir.path().join("does-not-exist"));
    }

    /// Filename parsing table — the sweep's entire safety boundary.
    #[test]
    fn unit_parse_launch_record_port_matches_only_exact_shape() {
        assert_eq!(
            parse_launch_record_port("launch-record.6000.json"),
            Some(6000)
        );
        assert_eq!(parse_launch_record_port("launch-record.1.json"), Some(1));
        assert_eq!(
            parse_launch_record_port("launch-record.65535.json"),
            Some(65535)
        );

        // Out of u16 range, non-numeric, wrong prefix/suffix, in-flight tmp.
        assert_eq!(parse_launch_record_port("launch-record.65536.json"), None);
        assert_eq!(parse_launch_record_port("launch-record.abc.json"), None);
        assert_eq!(parse_launch_record_port("launch-record.json"), None);
        assert_eq!(
            parse_launch_record_port("launch-record.6000.json.tmp"),
            None
        );
        assert_eq!(parse_launch_record_port("daemon.6000.json"), None);
        assert_eq!(parse_launch_record_port("daemon.6000.throttle.json"), None);
        assert_eq!(parse_launch_record_port("daemon.log"), None);
    }

    /// Task C / AC 1 (unit half): the record count does not grow with the
    /// launch count.
    ///
    /// Each iteration models one `launch`: sweep first (as
    /// `commands::launch::run` does), then write the record for the instance
    /// this launch starts. The instance's pid is dead by the time of the next
    /// sweep — the killed/crashed/harness-abandoned case that `remove_in`
    /// never covers, and that made `~/.ff-rdp/` reach 4040 files.
    ///
    /// Crucially, every port is distinct, exactly as an ephemeral `bind(:0)`
    /// hands them out. That is the condition under which `read_in`'s
    /// lazy-on-read reaping collects *nothing*: without this sweep the
    /// assertion below would see `LAUNCHES + 1` files.
    #[test]
    fn unit_launch_record_count_stays_bounded_across_repeated_launches() {
        const LAUNCHES: u16 = 50;
        let dir = tempfile::tempdir().expect("tempdir");

        // A concurrently-running instance, present for the whole run.
        plant_record(dir.path(), 19000, std::process::id());

        for i in 0..LAUNCHES {
            gc_stale_launch_records_in(dir.path());
            plant_record(dir.path(), 20000 + i, DEAD_PID);
        }

        let count = launch_record_count(dir.path());
        assert_eq!(
            count,
            2,
            "after {LAUNCHES} launches the directory must hold only the live \
             instance's record plus the most recent launch's own (got {count}); \
             unbounded growth would give {}",
            LAUNCHES + 1
        );
        assert!(
            dir.path().join(record_filename(19000)).exists(),
            "the live instance's record must survive all {LAUNCHES} sweeps"
        );
    }
}
