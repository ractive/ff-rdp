//! Lightweight single-file record shared by `launch` and `daemon stop`.
//!
//! Every Firefox instance spawned by ff-rdp (whether via `launch` or
//! `daemon start`) writes one [`DaemonRecord`] to a well-known cache file.
//! `daemon stop` reads that record to find the PID and port regardless of
//! which subcommand launched Firefox.
//!
//! ## File location
//!
//! | Platform       | Path                                     |
//! |----------------|------------------------------------------|
//! | Linux / macOS  | `~/.cache/ff-rdp/daemon.json`            |
//! | Windows        | `%LOCALAPPDATA%\ff-rdp\daemon.json`      |
//!
//! The `FF_RDP_HOME` env-var overrides the base directory (same convention as
//! the existing registry in `daemon/registry.rs`):
//! when set, the file is written to `$FF_RDP_HOME/cache/daemon.json`.
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

/// Return the directory that contains the daemon record file.
///
/// Respects `FF_RDP_HOME` for test isolation:
/// - If set: `$FF_RDP_HOME/cache/`
/// - Otherwise: platform cache dir (`~/.cache/ff-rdp` on Linux/macOS,
///   `%LOCALAPPDATA%\ff-rdp` on Windows) via `dirs::cache_dir()`.
pub fn record_base_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("FF_RDP_HOME") {
        return Ok(PathBuf::from(home).join("cache"));
    }
    let cache = dirs::cache_dir().context("could not determine cache directory")?;
    Ok(cache.join("ff-rdp"))
}

/// Return the full path to the daemon record file in the default cache directory.
///
/// Useful for diagnostics and `daemon status` output.
pub fn record_path() -> Result<PathBuf> {
    Ok(record_base_dir()?.join("daemon.json"))
}

// ---------------------------------------------------------------------------
// Test-injectable base-dir variants
// ---------------------------------------------------------------------------

/// Read the daemon record from `<dir>/daemon.json`.
///
/// Returns `None` if the file is absent or if the recorded PID is no longer
/// alive (stale entry). When a stale entry is detected the file is removed
/// so it cannot interfere with future launches.
pub fn read_in(dir: &Path) -> Result<Option<DaemonRecord>> {
    let path = dir.join("daemon.json");
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

/// Write `rec` to `<dir>/daemon.json` atomically (write-to-tmp + rename).
pub fn write_in(dir: &Path, rec: &DaemonRecord) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating daemon record directory {}", dir.display()))?;

    let record_path = dir.join("daemon.json");
    let tmp_path = dir.join("daemon.json.tmp");

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

/// Remove `<dir>/daemon.json` if it exists (idempotent).
pub fn remove_in(dir: &Path) -> Result<()> {
    let path = dir.join("daemon.json");
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
        let path = dir.path().join("daemon.json");
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

        let tmp = dir.path().join("daemon.json.tmp");
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
        assert!(!dir.path().join("daemon.json").exists());
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
