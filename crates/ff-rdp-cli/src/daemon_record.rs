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
//! ## Staleness
//!
//! [`read`] / [`read_in`] perform a PID-liveness check on every read.
//! If the recorded PID is no longer running the record is treated as absent
//! and the file is removed so the stale entry does not block a future launch.
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
}
