use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt as _;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub(crate) pid: u32,
    pub(crate) proxy_port: u16,
    pub(crate) firefox_host: String,
    pub(crate) firefox_port: u16,
    /// ISO 8601 timestamp of when the daemon was started.
    pub(crate) started_at: String,
}

// ---------------------------------------------------------------------------
// Base-dir helpers (accept an explicit directory for testability)
// ---------------------------------------------------------------------------

/// Read `<dir>/daemon.json`, returning `None` if the file does not exist.
pub(crate) fn read_registry_in(dir: &Path) -> Result<Option<DaemonInfo>> {
    let path = dir.join("daemon.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading registry at {}", path.display()))?;
    let info: DaemonInfo = serde_json::from_str(&contents)
        .with_context(|| format!("parsing registry at {}", path.display()))?;
    validate_registry(&info)
        .with_context(|| format!("validating registry at {}", path.display()))?;
    Ok(Some(info))
}

/// Validate that a deserialized [`DaemonInfo`] contains sane values.
///
/// Guards against corrupted or maliciously crafted registry files that
/// could cause confusing downstream errors (e.g. connecting to port 0).
fn validate_registry(info: &DaemonInfo) -> Result<()> {
    anyhow::ensure!(
        info.proxy_port > 0,
        "proxy_port must be > 0, got {}",
        info.proxy_port
    );
    anyhow::ensure!(
        info.firefox_port > 0,
        "firefox_port must be > 0, got {}",
        info.firefox_port
    );
    anyhow::ensure!(info.pid > 0, "pid must be > 0, got {}", info.pid);
    Ok(())
}

/// Write `info` to `<dir>/daemon.json` atomically using write-then-rename.
///
/// An exclusive file lock is acquired on the target path (creating it if
/// necessary) before writing, so concurrent writers do not corrupt the file.
pub(crate) fn write_registry_in(dir: &Path, info: &DaemonInfo) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;

    let registry_path = dir.join("daemon.json");
    let tmp_path = dir.join("daemon.json.tmp");

    // Acquire an exclusive lock on the registry file (creates it if absent).
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&registry_path)
        .with_context(|| format!("opening lock file {}", registry_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("locking registry file {}", registry_path.display()))?;

    // Write to a .tmp file then rename for atomicity.
    let json = serde_json::to_string_pretty(info).context("serializing DaemonInfo to JSON")?;
    let mut opts = fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
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

    fs::rename(&tmp_path, &registry_path).with_context(|| {
        format!(
            "renaming {} -> {}",
            tmp_path.display(),
            registry_path.display()
        )
    })?;

    // Lock is released when `lock_file` is dropped here.
    Ok(())
}

/// Remove `<dir>/daemon.json` if it exists.
pub(crate) fn remove_registry_in(dir: &Path) -> Result<()> {
    let path = dir.join("daemon.json");
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing registry file {}", path.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public convenience wrappers that use the real `~/.ff-rdp/` directory
// ---------------------------------------------------------------------------

/// Return the `~/.ff-rdp/` directory, creating it if it does not exist.
///
/// Respects `FF_RDP_HOME` env var as an override (useful for testing on
/// Windows where `dirs::home_dir()` uses the Windows API and ignores
/// `HOME`/`USERPROFILE` overrides).
pub fn registry_dir() -> Result<PathBuf> {
    let home = match std::env::var_os("FF_RDP_HOME") {
        Some(h) => PathBuf::from(h),
        None => dirs::home_dir().context("could not determine home directory")?,
    };
    let dir = home.join(".ff-rdp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Idempotent hardening: reset to 0o700 on every call in case
                // permissions were widened externally.
                let perms = std::fs::Permissions::from_mode(0o700);
                fs::set_permissions(&dir, perms)
                    .with_context(|| format!("setting permissions on {}", dir.display()))?;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("creating ff-rdp directory {}", dir.display()));
            }
        }
    }
    #[cfg(not(unix))]
    fs::create_dir_all(&dir)
        .with_context(|| format!("creating ff-rdp directory {}", dir.display()))?;
    Ok(dir)
}

/// Read and parse `~/.ff-rdp/daemon.json`.  Returns `Ok(None)` if the file
/// does not exist.
pub fn read_registry() -> Result<Option<DaemonInfo>> {
    read_registry_in(&registry_dir()?)
}

/// Write `info` to `~/.ff-rdp/daemon.json` atomically.
pub fn write_registry(info: &DaemonInfo) -> Result<()> {
    write_registry_in(&registry_dir()?, info)
}

/// Remove `~/.ff-rdp/daemon.json` if it exists.
pub fn remove_registry() -> Result<()> {
    remove_registry_in(&registry_dir()?)
}

/// Return the path to `~/.ff-rdp/daemon.log`.
pub fn log_path() -> Result<PathBuf> {
    Ok(registry_dir()?.join("daemon.log"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn sample_info() -> DaemonInfo {
        DaemonInfo {
            pid: 12345,
            proxy_port: 7000,
            firefox_host: "127.0.0.1".to_owned(),
            firefox_port: 6000,
            started_at: "2026-04-06T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = sample_info();

        write_registry_in(dir.path(), &info).expect("write");
        let read_back = read_registry_in(dir.path())
            .expect("read")
            .expect("should be Some");

        assert_eq!(read_back.pid, info.pid);
        assert_eq!(read_back.proxy_port, info.proxy_port);
        assert_eq!(read_back.firefox_host, info.firefox_host);
        assert_eq!(read_back.firefox_port, info.firefox_port);
        assert_eq!(read_back.started_at, info.started_at);
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = read_registry_in(dir.path()).expect("read");
        assert!(result.is_none());
    }

    #[test]
    fn remove_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &sample_info()).expect("write");

        let registry_file = dir.path().join("daemon.json");
        assert!(registry_file.exists());

        remove_registry_in(dir.path()).expect("remove");
        assert!(!registry_file.exists());
    }

    #[test]
    fn remove_nonexistent_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Must not return an error.
        remove_registry_in(dir.path()).expect("remove on nonexistent should succeed");
    }

    #[test]
    fn write_is_atomic_tmp_cleaned_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &sample_info()).expect("write");

        // The .tmp file must not remain after a successful write.
        let tmp = dir.path().join("daemon.json.tmp");
        assert!(
            !tmp.exists(),
            ".tmp file should be gone after atomic rename"
        );
    }

    #[test]
    fn overwrite_updates_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &sample_info()).expect("first write");

        let updated = DaemonInfo {
            pid: 99999,
            proxy_port: 8080,
            firefox_host: "localhost".to_owned(),
            firefox_port: 6001,
            started_at: "2026-04-07T00:00:00Z".to_owned(),
        };
        write_registry_in(dir.path(), &updated).expect("second write");

        let read_back = read_registry_in(dir.path()).expect("read").expect("Some");
        assert_eq!(read_back.pid, 99999);
        assert_eq!(read_back.proxy_port, 8080);
    }

    #[cfg(unix)]
    #[test]
    fn registry_file_has_restricted_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("sub");
        write_registry_in(&sub, &sample_info()).expect("write");
        let file_perms = fs::metadata(sub.join("daemon.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_perms, 0o600, "registry file should be owner-only");
    }

    #[test]
    fn read_corrupt_json_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("daemon.json"), b"not valid json").expect("write corrupt");
        let result = read_registry_in(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn read_invalid_port_zero_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":1234,"proxy_port":0,"firefox_host":"127.0.0.1","firefox_port":6000,"started_at":"2026-04-09T00:00:00Z"}"#;
        fs::write(dir.path().join("daemon.json"), json).expect("write");
        let result = read_registry_in(dir.path());
        assert!(result.is_err(), "port 0 should fail validation");
    }

    #[test]
    fn read_invalid_pid_zero_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":0,"proxy_port":7000,"firefox_host":"127.0.0.1","firefox_port":6000,"started_at":"2026-04-09T00:00:00Z"}"#;
        fs::write(dir.path().join("daemon.json"), json).expect("write");
        let result = read_registry_in(dir.path());
        assert!(result.is_err(), "pid 0 should fail validation");
    }
}
