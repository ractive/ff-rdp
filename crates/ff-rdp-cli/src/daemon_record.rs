//! Lightweight single-file record shared by `launch` and `daemon stop`.
//!
//! Every Firefox instance spawned by ff-rdp (whether via `launch` or
//! `daemon start`) writes one [`DaemonRecord`] to a well-known cache file.
//! `daemon stop` reads that record to find the PID and port regardless of
//! which subcommand launched Firefox.
//!
//! ## File location
//!
//! `~/.ff-rdp/launch-record.json` on all platforms — shares the same parent
//! directory as the proxy-daemon registry (`~/.ff-rdp/daemon.json`) so a single
//! `FF_RDP_HOME` cleanup wipes all ff-rdp state. The file name differs to
//! avoid colliding with the existing registry file.
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

/// Filename used for the launch record, sharing `~/.ff-rdp/` with the
/// proxy-daemon registry's `daemon.json`.
const RECORD_FILENAME: &str = "launch-record.json";

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

/// Read the daemon record from `<dir>/launch-record.json`.
///
/// Returns `None` if the file is absent or if the recorded PID is no longer
/// alive (stale entry). When a stale entry is detected the file is removed
/// so it cannot interfere with future launches.
pub fn read_in(dir: &Path) -> Result<Option<DaemonRecord>> {
    let path = dir.join(RECORD_FILENAME);
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

/// Write `rec` to `<dir>/launch-record.json` atomically (write-to-tmp + rename).
pub fn write_in(dir: &Path, rec: &DaemonRecord) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating daemon record directory {}", dir.display()))?;

    let record_path = dir.join(RECORD_FILENAME);
    let tmp_path = dir.join(format!("{RECORD_FILENAME}.tmp"));

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

/// Remove `<dir>/launch-record.json` if it exists (idempotent).
pub fn remove_in(dir: &Path) -> Result<()> {
    let path = dir.join(RECORD_FILENAME);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing daemon record {}", path.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public convenience wrappers using the real cache directory
// ---------------------------------------------------------------------------

/// Read the daemon record from the default cache location.
///
/// Returns `None` if absent or if the recorded PID is dead (stale).
pub fn read() -> Result<Option<DaemonRecord>> {
    read_in(&record_base_dir()?)
}

/// Write the daemon record to the default cache location atomically.
pub fn write(rec: &DaemonRecord) -> Result<()> {
    write_in(&record_base_dir()?, rec)
}

/// Remove the daemon record from the default cache location (idempotent).
pub fn remove() -> Result<()> {
    remove_in(&record_base_dir()?)
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

        let read_back = read_in(dir.path())
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
        let path = dir.path().join(RECORD_FILENAME);
        let json = serde_json::to_string_pretty(&rec).unwrap();
        fs::write(&path, json).expect("write stale record");
        assert!(path.exists(), "file must exist before read_in");

        // read_in should detect the dead PID and return None.
        let result = read_in(dir.path()).expect("read_in ok");
        assert!(result.is_none(), "stale PID must return None");

        // The file must have been removed.
        assert!(!path.exists(), "stale record file must be removed");
    }

    /// No `.tmp` file left behind after a successful write.
    #[test]
    fn unit_daemon_record_atomic_write_no_tmp_left_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_in(dir.path(), &sample_record()).expect("write_in");

        let tmp = dir.path().join(format!("{RECORD_FILENAME}.tmp"));
        assert!(
            !tmp.exists(),
            ".tmp file must not remain after atomic write"
        );
    }

    /// `read_in` returns None when the directory is empty.
    #[test]
    fn unit_daemon_record_read_absent_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = read_in(dir.path()).expect("read_in ok");
        assert!(result.is_none());
    }

    /// `remove_in` is idempotent (does not error on missing file).
    #[test]
    fn unit_daemon_record_remove_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        remove_in(dir.path()).expect("remove when absent must not error");

        write_in(dir.path(), &sample_record()).expect("write");
        remove_in(dir.path()).expect("remove when present must not error");
        assert!(!dir.path().join(RECORD_FILENAME).exists());
    }

    /// Overwriting with a second write replaces the first.
    #[test]
    fn unit_daemon_record_overwrite_replaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_in(dir.path(), &sample_record()).expect("first write");

        let updated = DaemonRecord {
            pid: std::process::id(),
            port: 7777,
            headless: false,
            launched_at: Utc::now(),
            profile_dir: PathBuf::from("/tmp/updated"),
        };
        write_in(dir.path(), &updated).expect("second write");

        let read_back = read_in(dir.path()).expect("read_in ok").expect("Some");
        assert_eq!(read_back.port, 7777);
    }
}
