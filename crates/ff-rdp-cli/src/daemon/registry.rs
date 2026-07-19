use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt as _;
use serde::{Deserialize, Serialize};

/// Generate a 32-byte cryptographically-random token and return it as a
/// 64-character lowercase hex string.
pub(crate) fn generate_auth_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| anyhow::anyhow!("generating random auth token: {e}"))?;
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub(crate) pid: u32,
    pub(crate) proxy_port: u16,
    pub(crate) firefox_host: String,
    pub(crate) firefox_port: u16,
    /// ISO 8601 timestamp of when the daemon was started.
    pub(crate) started_at: String,
    /// 32-byte random auth token (hex-encoded, 64 chars).
    ///
    /// Every CLI client must send `{"auth": "<token>"}` as its first frame.
    /// A mismatch causes the daemon to immediately close the connection.
    /// Stored in `daemon.json` (already 0o600) so only the file owner can
    /// connect — defeating DNS-rebinding attacks from browser tabs or
    /// sandboxed processes that can reach localhost TCP but cannot read $HOME.
    pub(crate) auth_token: String,
}

// ---------------------------------------------------------------------------
// Per-port registry file naming (iter-123 Theme B)
// ---------------------------------------------------------------------------
//
// The registry is keyed by the Firefox debugging `port` so that concurrent
// ff-rdp instances driving different Firefox instances (ports 6000/6001/…) do
// not clobber each other's daemon record.  Each daemon writes to
// `daemon.<port>.json` and locks `daemon.<port>.spawn.lock`; `find_running_daemon`
// / `wait_for_registry` already validate `firefox_port`, so once storage is
// port-scoped their lookups need no further change.

/// Return the registry filename for a given Firefox `port`
/// (e.g. `daemon.6000.json`).
fn registry_filename(port: u16) -> String {
    format!("daemon.{port}.json")
}

/// Return the spawn-lock filename for a given Firefox `port`
/// (e.g. `daemon.6000.spawn.lock`).
fn spawn_lock_filename(port: u16) -> String {
    format!("daemon.{port}.spawn.lock")
}

// ---------------------------------------------------------------------------
// Base-dir helpers (accept an explicit directory for testability)
// ---------------------------------------------------------------------------

/// Read `<dir>/daemon.<port>.json`, returning `None` if the file does not exist.
pub(crate) fn read_registry_in(dir: &Path, port: u16) -> Result<Option<DaemonInfo>> {
    let path = dir.join(registry_filename(port));
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

/// Write `info` to `<dir>/daemon.<port>.json` atomically using write-then-rename.
///
/// The file is keyed by `info.firefox_port` so writes for one port never
/// overwrite another port's record (iter-123 Theme B).  An exclusive file lock
/// is acquired on the target path (creating it if necessary) before writing, so
/// concurrent writers do not corrupt the file.
pub(crate) fn write_registry_in(dir: &Path, info: &DaemonInfo) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;

    let filename = registry_filename(info.firefox_port);
    let registry_path = dir.join(&filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

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

/// Remove `<dir>/daemon.<port>.json` if it exists.
pub(crate) fn remove_registry_in(dir: &Path, port: u16) -> Result<()> {
    let path = dir.join(registry_filename(port));
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing registry file {}", path.display()))?;
    }
    Ok(())
}

/// Remove a stale legacy single-slot `daemon.json` if present (iter-123 Theme B).
///
/// Earlier builds wrote one global `~/.ff-rdp/daemon.json`.  Now that the
/// registry is keyed per port, that file is never read again; a lingering copy
/// (from an older ff-rdp that crashed without cleaning up) would only confuse a
/// human inspecting the directory.  Best-effort: a failure to remove is ignored
/// so it never blocks the current, port-scoped code path.
pub(crate) fn remove_legacy_registry_in(dir: &Path) {
    let legacy = dir.join("daemon.json");
    if legacy.exists() {
        let _ = fs::remove_file(&legacy);
    }
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

/// Read and parse `~/.ff-rdp/daemon.<port>.json` for the given Firefox `port`.
/// Returns `Ok(None)` if the file does not exist (iter-123 Theme B).
pub fn read_registry(port: u16) -> Result<Option<DaemonInfo>> {
    read_registry_in(&registry_dir()?, port)
}

/// Write `info` to `~/.ff-rdp/daemon.<port>.json` atomically, keyed by
/// `info.firefox_port` (iter-123 Theme B).
pub fn write_registry(info: &DaemonInfo) -> Result<()> {
    let dir = registry_dir()?;
    // Opportunistically retire any stale legacy single-slot file so the
    // per-port scheme is the only thing left in the directory.
    remove_legacy_registry_in(&dir);
    write_registry_in(&dir, info)
}

/// Remove `~/.ff-rdp/daemon.<port>.json` for the given Firefox `port` if it
/// exists (iter-123 Theme B).
pub fn remove_registry(port: u16) -> Result<()> {
    remove_registry_in(&registry_dir()?, port)
}

/// Return the path to `~/.ff-rdp/daemon.log`.
pub fn log_path() -> Result<PathBuf> {
    Ok(registry_dir()?.join("daemon.log"))
}

// ---------------------------------------------------------------------------
// Spawn serialization lock (iter-100 Theme D)
// ---------------------------------------------------------------------------

/// An exclusive advisory file lock held across the daemon
/// check→spawn→register sequence (iter-100 Theme D).
///
/// The lock is released automatically when this guard is dropped (the
/// underlying file handle is closed).  Holding it for the *whole* sequence —
/// not just the registry write — is what prevents two racing CLI invocations
/// from both observing "no daemon", both spawning one, and orphaning the
/// loser: the second invocation blocks on [`acquire_spawn_lock`] until the
/// first has finished registering, then re-reads the registry and reuses the
/// winner instead of spawning a second daemon.
pub(crate) struct SpawnLock {
    // Kept alive purely for its lock; the `flock`/`LockFile` is released on
    // drop.  Never read directly.
    _file: fs::File,
}

/// Acquire the **per-port** daemon spawn lock, blocking until it is available.
///
/// Uses a dedicated `daemon.<port>.spawn.lock` file (separate from
/// `daemon.<port>.json` so the lock lifetime is independent of registry
/// write/rename churn).  The lock is advisory and cross-process: `fs2`'s
/// `lock_exclusive` maps to `flock` (Unix) / `LockFileEx` (Windows), so it
/// serializes across independent `ff-rdp` processes, which is exactly the
/// auto-start race we must close.
///
/// iter-123 Theme B: the lock is keyed by `port` so concurrent autostarts
/// targeting *different* Firefox instances no longer serialize behind one
/// global lock (or collide on a single record) — a spawn for port 6000 never
/// blocks a spawn for port 6001.
pub(crate) fn acquire_spawn_lock(port: u16) -> Result<SpawnLock> {
    acquire_spawn_lock_in(&registry_dir()?, port)
}

/// [`acquire_spawn_lock`] against an explicit directory (testable).
pub(crate) fn acquire_spawn_lock_in(dir: &Path, port: u16) -> Result<SpawnLock> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;
    let lock_path = dir.join(spawn_lock_filename(port));
    let mut opts = fs::OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts
        .open(&lock_path)
        .with_context(|| format!("opening spawn lock file {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("acquiring spawn lock {}", lock_path.display()))?;
    Ok(SpawnLock { _file: file })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Firefox port used by [`sample_info`]; the registry file is keyed on it.
    const SAMPLE_PORT: u16 = 6000;

    fn sample_info() -> DaemonInfo {
        DaemonInfo {
            pid: 12345,
            proxy_port: 7000,
            firefox_host: "127.0.0.1".to_owned(),
            firefox_port: SAMPLE_PORT,
            started_at: "2026-04-06T12:00:00Z".to_owned(),
            auth_token: "a".repeat(64),
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = sample_info();

        write_registry_in(dir.path(), &info).expect("write");
        let read_back = read_registry_in(dir.path(), SAMPLE_PORT)
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
        let result = read_registry_in(dir.path(), SAMPLE_PORT).expect("read");
        assert!(result.is_none());
    }

    /// AC `unit_registry_per_port_no_clobber`: a write for port A and a write
    /// for port B produce two independent files, and reading each port back
    /// returns that port's own record — neither overwrites the other.
    #[test]
    fn per_port_writes_do_not_clobber() {
        let dir = tempfile::tempdir().expect("tempdir");

        let mut info_a = sample_info();
        info_a.firefox_port = 6000;
        info_a.proxy_port = 7000;
        info_a.pid = 1000;

        let mut info_b = sample_info();
        info_b.firefox_port = 6001;
        info_b.proxy_port = 7001;
        info_b.pid = 2000;

        write_registry_in(dir.path(), &info_a).expect("write A");
        write_registry_in(dir.path(), &info_b).expect("write B");

        // Both files exist side by side.
        assert!(dir.path().join("daemon.6000.json").exists());
        assert!(dir.path().join("daemon.6001.json").exists());

        let read_a = read_registry_in(dir.path(), 6000)
            .expect("read A")
            .expect("A present");
        let read_b = read_registry_in(dir.path(), 6001)
            .expect("read B")
            .expect("B present");

        // Port A's record is intact — not clobbered by B's write.
        assert_eq!(read_a.pid, 1000);
        assert_eq!(read_a.proxy_port, 7000);
        assert_eq!(read_a.firefox_port, 6000);
        // Port B's record is its own.
        assert_eq!(read_b.pid, 2000);
        assert_eq!(read_b.proxy_port, 7001);
        assert_eq!(read_b.firefox_port, 6001);
    }

    #[test]
    fn remove_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &sample_info()).expect("write");

        let registry_file = dir.path().join("daemon.6000.json");
        assert!(registry_file.exists());

        remove_registry_in(dir.path(), SAMPLE_PORT).expect("remove");
        assert!(!registry_file.exists());
    }

    #[test]
    fn remove_nonexistent_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Must not return an error.
        remove_registry_in(dir.path(), SAMPLE_PORT).expect("remove on nonexistent should succeed");
    }

    #[test]
    fn remove_only_affects_the_named_port() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut info_a = sample_info();
        info_a.firefox_port = 6000;
        let mut info_b = sample_info();
        info_b.firefox_port = 6001;
        write_registry_in(dir.path(), &info_a).expect("write A");
        write_registry_in(dir.path(), &info_b).expect("write B");

        remove_registry_in(dir.path(), 6000).expect("remove A");
        assert!(!dir.path().join("daemon.6000.json").exists());
        assert!(
            dir.path().join("daemon.6001.json").exists(),
            "removing port 6000 must not remove port 6001's record"
        );
    }

    #[test]
    fn write_removes_stale_legacy_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Simulate an old single-slot file left behind by a previous build.
        fs::write(dir.path().join("daemon.json"), b"{}").expect("write legacy");
        remove_legacy_registry_in(dir.path());
        assert!(
            !dir.path().join("daemon.json").exists(),
            "stale legacy daemon.json must be retired"
        );
    }

    #[test]
    fn write_is_atomic_tmp_cleaned_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &sample_info()).expect("write");

        // The .tmp file must not remain after a successful write.
        let tmp = dir.path().join("daemon.6000.json.tmp");
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
            firefox_port: SAMPLE_PORT,
            started_at: "2026-04-07T00:00:00Z".to_owned(),
            auth_token: "b".repeat(64),
        };
        write_registry_in(dir.path(), &updated).expect("second write");

        let read_back = read_registry_in(dir.path(), SAMPLE_PORT)
            .expect("read")
            .expect("Some");
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
        let file_perms = fs::metadata(sub.join("daemon.6000.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_perms, 0o600, "registry file should be owner-only");
    }

    #[test]
    fn read_corrupt_json_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("daemon.6000.json"), b"not valid json").expect("write corrupt");
        let result = read_registry_in(dir.path(), SAMPLE_PORT);
        assert!(result.is_err());
    }

    #[test]
    fn read_invalid_port_zero_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":1234,"proxy_port":0,"firefox_host":"127.0.0.1","firefox_port":6000,"started_at":"2026-04-09T00:00:00Z","auth_token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        fs::write(dir.path().join("daemon.6000.json"), json).expect("write");
        let result = read_registry_in(dir.path(), 6000);
        assert!(result.is_err(), "port 0 should fail validation");
    }

    #[test]
    fn read_invalid_firefox_port_zero_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":1234,"proxy_port":7000,"firefox_host":"127.0.0.1","firefox_port":0,"started_at":"2026-04-09T00:00:00Z","auth_token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        // Filename is looked up by the caller-supplied port (6000); the record's
        // internal firefox_port field of 0 is what must fail validation.
        fs::write(dir.path().join("daemon.6000.json"), json).expect("write");
        let result = read_registry_in(dir.path(), 6000);
        assert!(result.is_err(), "firefox_port 0 should fail validation");
    }

    #[test]
    fn read_invalid_pid_zero_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":0,"proxy_port":7000,"firefox_host":"127.0.0.1","firefox_port":6000,"started_at":"2026-04-09T00:00:00Z","auth_token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        fs::write(dir.path().join("daemon.6000.json"), json).expect("write");
        let result = read_registry_in(dir.path(), 6000);
        assert!(result.is_err(), "pid 0 should fail validation");
    }

    #[test]
    fn generate_auth_token_produces_64_hex_chars() {
        let token = generate_auth_token().expect("token generation should succeed");
        assert_eq!(token.len(), 64, "token must be 64 hex chars (32 bytes)");
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "token must be lowercase hex: {token:?}"
        );
    }

    #[test]
    fn generate_auth_token_is_not_all_zeros() {
        // Statistically impossible (2^-256 probability) for a random token to
        // be all zeros — this guards against a broken RNG returning zeroes.
        let token = generate_auth_token().expect("token generation should succeed");
        assert_ne!(token, "0".repeat(64), "token must not be all zeros");
    }

    /// AC `unit_spawn_lock_serializes_check_spawn_register` (lock half):
    /// two threads that both try to acquire the spawn lock against the same
    /// directory are serialized — the second blocks until the first releases,
    /// so at no instant do both hold the lock.  This is the primitive the
    /// check→spawn→register serialization in `resolve_connection_target`
    /// relies on to prevent a double-spawn.
    #[test]
    fn spawn_lock_serializes_two_acquirers() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

        let dir = Arc::new(tempfile::tempdir().expect("tempdir"));
        let concurrent = Arc::new(AtomicBool::new(false));
        let holders = Arc::new(AtomicU32::new(0));

        let spawn_worker =
            |dir: Arc<tempfile::TempDir>, concurrent: Arc<AtomicBool>, holders: Arc<AtomicU32>| {
                std::thread::spawn(move || {
                    let lock = acquire_spawn_lock_in(dir.path(), 6000).expect("acquire spawn lock");
                    // If another thread is inside the critical section at the same
                    // time, `holders` will exceed 1.
                    let now = holders.fetch_add(1, Ordering::SeqCst) + 1;
                    if now > 1 {
                        concurrent.store(true, Ordering::SeqCst);
                    }
                    // Hold the lock briefly so a racing acquirer would overlap if
                    // the lock were not exclusive.
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    holders.fetch_sub(1, Ordering::SeqCst);
                    drop(lock);
                })
            };

        let t1 = spawn_worker(
            Arc::clone(&dir),
            Arc::clone(&concurrent),
            Arc::clone(&holders),
        );
        let t2 = spawn_worker(
            Arc::clone(&dir),
            Arc::clone(&concurrent),
            Arc::clone(&holders),
        );
        t1.join().expect("t1");
        t2.join().expect("t2");

        assert!(
            !concurrent.load(Ordering::SeqCst),
            "the spawn lock must be exclusive — two acquirers must never overlap"
        );
    }

    #[test]
    fn spawn_lock_released_on_drop_allows_reacquire() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let _lock = acquire_spawn_lock_in(dir.path(), 6000).expect("first acquire");
            // dropped at end of scope
        }
        // A second acquire must succeed immediately now that the first is gone.
        let _lock2 = acquire_spawn_lock_in(dir.path(), 6000).expect("second acquire after release");
    }

    /// AC `unit_spawn_lock_per_port_independent`: the spawn lock for port A and
    /// the spawn lock for port B are distinct files, so holding one never blocks
    /// acquiring the other.  Concurrent autostarts on different Firefox ports
    /// must not serialize behind a single global lock (iter-123 Theme B).
    #[test]
    fn spawn_lock_is_per_port_and_does_not_cross_block() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Hold the lock for port 6000 …
        let _lock_a = acquire_spawn_lock_in(dir.path(), 6000).expect("acquire A");
        // … and acquiring the lock for port 6001 must still succeed immediately.
        let _lock_b = acquire_spawn_lock_in(dir.path(), 6001).expect("acquire B while A held");
        // The two lock files are distinct.
        assert!(dir.path().join("daemon.6000.spawn.lock").exists());
        assert!(dir.path().join("daemon.6001.spawn.lock").exists());
    }

    #[test]
    fn read_legacy_registry_without_auth_token_returns_error() {
        // Old daemon.<port>.json files without auth_token must fail to parse,
        // causing the client to fall back to spawning a new daemon that
        // generates a token.
        let dir = tempfile::tempdir().expect("tempdir");
        let json = r#"{"pid":1234,"proxy_port":7000,"firefox_host":"127.0.0.1","firefox_port":6000,"started_at":"2026-04-09T00:00:00Z"}"#;
        fs::write(dir.path().join("daemon.6000.json"), json).expect("write");
        let result = read_registry_in(dir.path(), 6000);
        assert!(
            result.is_err(),
            "legacy registry without auth_token must fail to parse"
        );
    }
}
