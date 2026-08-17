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

/// Return the registry *write*-lock filename for a given Firefox `port`
/// (e.g. `daemon.6000.write.lock`) — iter-172.
///
/// A sibling of the published record, never the record itself. See
/// [`acquire_registry_write_lock_in`] for why that distinction is the whole
/// point of this file existing.
fn write_lock_filename(port: u16) -> String {
    format!("daemon.{port}.write.lock")
}

// ---------------------------------------------------------------------------
// Base-dir helpers (accept an explicit directory for testability)
// ---------------------------------------------------------------------------

/// Read `<dir>/daemon.<port>.json`, returning `None` if the file does not
/// exist **or is empty**.
///
/// iter-172: a zero-byte record is *absence of a record*, not corruption, and
/// must read as `Ok(None)` rather than
/// `EOF while parsing a value at line 1 column 0`. Two producers of such a
/// file exist:
///
/// * pre-iter-172 builds of ff-rdp, whose [`write_registry_in`] took its
///   exclusive lock by opening the **published** path with `create(true)` —
///   so an empty `daemon.<port>.json` existed from lock-open until the
///   `rename`. That window is closed by the writer fix below, but a file left
///   behind by an older build (or by an older build killed mid-write) sits in
///   `~/.ff-rdp/` forever, and while it did, *every* invocation on that port
///   failed the daemon lookup and silently degraded to `route: "direct"`.
/// * any external truncation (`: > daemon.<port>.json`, a filesystem that
///   surfaced a zero-length file after an unclean shutdown).
///
/// Deliberately narrow: only a *zero-length* file is absence. Non-empty bytes
/// that do not parse are still an error — see
/// `read_corrupt_json_returns_error` — because those really are corruption
/// and silently ignoring them would hide a genuine problem.
pub(crate) fn read_registry_in(dir: &Path, port: u16) -> Result<Option<DaemonInfo>> {
    let path = dir.join(registry_filename(port));
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading registry at {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(None);
    }
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
/// overwrite another port's record (iter-123 Theme B).  Writers are serialized
/// against each other by an exclusive lock on the **sibling**
/// `daemon.<port>.write.lock` — see [`acquire_registry_write_lock_in`] for why
/// the lock must not live on the published path (iter-172).
///
/// The published record therefore only ever comes into existence via the
/// `rename` below, so a concurrent reader sees either no file at all or a
/// complete one — never a zero-byte record.
pub(crate) fn write_registry_in(dir: &Path, info: &DaemonInfo) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;

    let filename = registry_filename(info.firefox_port);
    let registry_path = dir.join(&filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    // Serialize concurrent writers on a sibling lock file. Held until the end
    // of this function, i.e. across the tmp write *and* the rename.
    let _write_lock = acquire_registry_write_lock_in(dir, info.firefox_port)?;

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

    // Lock is released when `_write_lock` is dropped here.
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

/// An exclusive advisory file lock held for the lifetime of this guard
/// (iter-100 Theme D; generalized over two lock files in iter-172).
///
/// The lock is released automatically when this guard is dropped (the
/// underlying file handle is closed).  Two distinct lock files use it:
/// `daemon.<port>.spawn.lock` ([`acquire_spawn_lock`]) and
/// `daemon.<port>.write.lock` ([`acquire_registry_write_lock_in`]).
pub(crate) struct FileLock {
    // Kept alive purely for its lock; the `flock`/`LockFile` is released on
    // drop.  Never read directly.
    _file: fs::File,
}

/// Open `path` (creating it if absent, owner-only on Unix) and take a blocking
/// exclusive advisory lock on it.
///
/// `what` names the lock in error context, e.g. `"spawn lock"`.
fn acquire_file_lock(path: &Path, what: &str) -> Result<FileLock> {
    let mut opts = fs::OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts
        .open(path)
        .with_context(|| format!("opening {what} file {}", path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("acquiring {what} {}", path.display()))?;
    Ok(FileLock { _file: file })
}

/// Acquire the **per-port registry write lock**, blocking until it is
/// available (iter-172).
///
/// Serializes concurrent [`write_registry_in`] calls without ever touching the
/// published `daemon.<port>.json`.
///
/// **Why a sibling file.** Through iter-171 the writer took its lock by
/// opening the published record itself with `create(true)`. That published a
/// **zero-byte** `daemon.<port>.json` the instant a write started, and content
/// only appeared at the closing `rename`. Any reader polling in that window
/// parsed zero bytes and got
/// `EOF while parsing a value at line 1 column 0`, which the autostart path
/// treated as a terminal failure: it abandoned the wait and silently ran the
/// command over a *direct* connection after the caller had explicitly asked
/// for the daemon. That reddened `live_128_meta_route`,
/// `live_134_meta_route_all_commands` and `live_123_daemon_autostart_tabless`
/// across three separate live sweeps, and it also mis-*classified* the
/// failure: `classify_registry_wait_failure` re-reads the registry, hit the
/// same empty file, and reported "spawn died before the registry write" for a
/// daemon that was perfectly alive.
///
/// [`acquire_spawn_lock_in`] had already reached the same conclusion for the
/// spawn lock ("so the lock lifetime is independent of registry write/rename
/// churn"); this is that treatment applied to the writer, which is where the
/// zero-byte file actually came from.
pub(crate) fn acquire_registry_write_lock_in(dir: &Path, port: u16) -> Result<FileLock> {
    acquire_file_lock(&dir.join(write_lock_filename(port)), "registry write lock")
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
pub(crate) fn acquire_spawn_lock(port: u16) -> Result<FileLock> {
    acquire_spawn_lock_in(&registry_dir()?, port)
}

/// [`acquire_spawn_lock`] against an explicit directory (testable).
///
/// Holding it for the *whole* check→spawn→register sequence — not just the
/// registry write — is what prevents two racing CLI invocations from both
/// observing "no daemon", both spawning one, and orphaning the loser: the
/// second invocation blocks here until the first has finished registering,
/// then re-reads the registry and reuses the winner.
pub(crate) fn acquire_spawn_lock_in(dir: &Path, port: u16) -> Result<FileLock> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;
    acquire_file_lock(&dir.join(spawn_lock_filename(port)), "spawn lock")
}

// ---------------------------------------------------------------------------
// Stale spawn-lock GC (iter-132 Theme E)
// ---------------------------------------------------------------------------

/// Parse the Firefox port out of a `daemon.<PORT>.spawn.lock` filename.
///
/// Returns `None` for anything else that lives in the registry directory —
/// `daemon.<port>.json`, `daemon.<port>.throttle.json` (iter-131),
/// `daemon.log`, `.tmp` write-ahead files, etc. The exact-suffix match
/// (`.spawn.lock`, not a general "starts with daemon." glob) is what keeps
/// this GC scoped to spawn locks only — matching [`spawn_lock_filename`].
fn parse_spawn_lock_port(filename: &str) -> Option<u16> {
    filename
        .strip_prefix("daemon.")?
        .strip_suffix(".spawn.lock")?
        .parse()
        .ok()
}

/// Parse the Firefox port out of a `daemon.<PORT>.write.lock` filename
/// (iter-172's registry write lock — see [`acquire_registry_write_lock_in`]).
///
/// Same exact-suffix discipline as [`parse_spawn_lock_port`]: the published
/// record `daemon.<port>.json`, the iter-131 throttle file and the `.tmp`
/// write-ahead file must never match.
fn parse_write_lock_port(filename: &str) -> Option<u16> {
    filename
        .strip_prefix("daemon.")?
        .strip_suffix(".write.lock")?
        .parse()
        .ok()
}

/// Parse the Firefox port out of either per-port lock filename.
///
/// The GC below sweeps both; a `.write.lock` left behind by a daemon that has
/// since exited is exactly as much dead weight in `~/.ff-rdp/` as a stale
/// `.spawn.lock`, and iter-172 would otherwise have introduced a second class
/// of file that accumulates forever (dogfood-62 #9 for the first class).
fn parse_lock_port(filename: &str) -> Option<u16> {
    parse_spawn_lock_port(filename).or_else(|| parse_write_lock_port(filename))
}

/// Whether the spawn lock for `port` is stale: no live process currently
/// claims it via `daemon.<port>.json`.
///
/// Conservative in the "unknown" direction — an absent or unparsable
/// registry is treated as stale (nothing proves the lock is still needed).
/// This is safe even though pid liveness alone can be wrong (e.g. a PID was
/// reused after the daemon exited): [`gc_stale_spawn_locks_in`] only ever
/// deletes a lock file it can *also* acquire itself, so a lock any other
/// process still actually holds is never removed regardless of what this
/// function returns.
fn spawn_lock_is_stale(dir: &Path, port: u16) -> bool {
    match read_registry_in(dir, port) {
        Ok(Some(info)) => !super::process::is_process_alive(info.pid),
        Ok(None) | Err(_) => true,
    }
}

/// Sweep `<dir>/daemon.*.spawn.lock` and `<dir>/daemon.*.write.lock` files
/// whose owning daemon is provably gone, so `~/.ff-rdp/` doesn't accumulate
/// zero-byte lock files forever (dogfood-62 #9: ~50 stale locks observed,
/// never cleaned up before this).
///
/// Deliberately narrow and defensive:
/// - Only filenames matching the exact `daemon.<PORT>.spawn.lock` or
///   `daemon.<PORT>.write.lock` pattern are touched (see [`parse_lock_port`])
///   — `daemon.<port>.json` and `daemon.<port>.throttle.json` (iter-131) are
///   never candidates.
/// - "Stale" (per [`spawn_lock_is_stale`]) is only a pre-filter. Before
///   actually deleting, this takes a non-blocking `try_lock_exclusive()` on
///   the candidate file. If that fails, some other process currently holds
///   the flock on it (e.g. it is mid-acquire *right now*, which registry
///   pid-liveness alone cannot see) and the file is left alone — avoids an
///   unlink-while-locked race with a legitimate concurrent acquirer.
/// - Every I/O error (unreadable directory entry, permission failure, race
///   where the file vanished between listing and opening) is swallowed.
///   This is opportunistic housekeeping riding along on the daemon spawn
///   path; it must never turn into a reason the actual spawn attempt fails.
pub(crate) fn gc_stale_spawn_locks_in(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(port) = parse_lock_port(&name) else {
            continue;
        };
        if !spawn_lock_is_stale(dir, port) {
            continue;
        }
        let path = entry.path();
        let Ok(file) = fs::OpenOptions::new().write(true).open(&path) else {
            continue;
        };
        // Only delete if we can also acquire the lock ourselves — i.e.
        // nobody else currently holds it.
        if file.try_lock_exclusive().is_ok() {
            let _ = fs::remove_file(&path);
            let _ = fs2::FileExt::unlock(&file);
        }
    }
}

/// [`gc_stale_spawn_locks_in`] against the real `~/.ff-rdp/` directory.
pub(crate) fn gc_stale_spawn_locks() {
    if let Ok(dir) = registry_dir() {
        gc_stale_spawn_locks_in(&dir);
    }
}

// ---------------------------------------------------------------------------
// Legacy (port-less) spawn-lock GC (iter-142 Theme B)
// ---------------------------------------------------------------------------

/// The port-less spawn-lock name a pre-iter-123 ff-rdp build wrote, before
/// the registry was scoped per Firefox port. [`parse_spawn_lock_port`]
/// requires a `daemon.<PORT>.spawn.lock` shape, so this exact filename can
/// never match it and [`gc_stale_spawn_locks_in`] never touches it — it sits
/// forever on any host that ever ran a pre-123 build (dogfooding session 63,
/// item 30). No current build writes this file, so unlike the per-port
/// locks there is no registry entry to consult for liveness; the
/// `try_lock_exclusive` gate below is the only safety check available, and
/// is sufficient — nothing in the current codebase acquires this name, so
/// the only way the lock is exclusive-lockable-but-still-needed is if some
/// other pre-123 process is mid-flight, which the flock check catches.
const LEGACY_SPAWN_LOCK_FILENAME: &str = "daemon.spawn.lock";

/// Remove `<dir>/daemon.spawn.lock` if present and not currently
/// flock-held by another process. Idempotent; every error is swallowed —
/// this is opportunistic housekeeping riding along the same path as
/// [`gc_stale_spawn_locks_in`].
pub(crate) fn gc_legacy_spawn_lock_in(dir: &Path) {
    let path = dir.join(LEGACY_SPAWN_LOCK_FILENAME);
    let Ok(file) = fs::OpenOptions::new().write(true).open(&path) else {
        return;
    };
    if file.try_lock_exclusive().is_ok() {
        let _ = fs::remove_file(&path);
        let _ = fs2::FileExt::unlock(&file);
    }
}

/// [`gc_legacy_spawn_lock_in`] against the real `~/.ff-rdp/` directory.
pub(crate) fn gc_legacy_spawn_lock() {
    if let Ok(dir) = registry_dir() {
        gc_legacy_spawn_lock_in(&dir);
    }
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

    // ── iter-132 Theme E: stale spawn-lock GC ────────────────────────────────

    /// A pid known to never correspond to a live process — same convention
    /// `daemon::process`'s own tests use for "definitely dead".
    const DEAD_PID: u32 = 999_999_999;

    fn info_with_pid(port: u16, pid: u32) -> DaemonInfo {
        DaemonInfo {
            pid,
            proxy_port: 7000,
            firefox_host: "127.0.0.1".to_owned(),
            firefox_port: port,
            started_at: "2026-04-06T12:00:00Z".to_owned(),
            auth_token: "a".repeat(64),
        }
    }

    #[test]
    fn parse_spawn_lock_port_matches_exact_suffix_only() {
        assert_eq!(parse_spawn_lock_port("daemon.6000.spawn.lock"), Some(6000));
        // Must NOT match the registry file itself, the iter-131 throttle
        // state file, or the daemon log — same directory, different suffix.
        assert_eq!(parse_spawn_lock_port("daemon.6000.json"), None);
        assert_eq!(parse_spawn_lock_port("daemon.6000.throttle.json"), None);
        assert_eq!(parse_spawn_lock_port("daemon.log"), None);
        assert_eq!(parse_spawn_lock_port("daemon.6000.spawn.lock.tmp"), None);
        assert_eq!(parse_spawn_lock_port("not-a-daemon-file"), None);
    }

    /// AC `unit_spawn_lock_gc`: a spawn lock whose registry entry's pid is
    /// dead is removed; a spawn lock whose registry entry's pid is alive is
    /// kept; an unrelated `daemon.<port>.throttle.json` (iter-131, same
    /// directory) is left completely untouched — negative-case assertion
    /// that the GC's filename match is scoped exactly to `*.spawn.lock`.
    #[test]
    fn gc_stale_spawn_locks_removes_dead_keeps_live_and_ignores_throttle_file() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Port 6000: registry pid is dead — lock must be removed.
        write_registry_in(dir.path(), &info_with_pid(6000, DEAD_PID)).expect("write dead");
        fs::write(dir.path().join("daemon.6000.spawn.lock"), []).expect("write dead lock");

        // Port 6001: registry pid is this test process itself — alive — lock
        // must survive.
        let my_pid = std::process::id();
        write_registry_in(dir.path(), &info_with_pid(6001, my_pid)).expect("write live");
        fs::write(dir.path().join("daemon.6001.spawn.lock"), []).expect("write live lock");

        // Port 6002: no registry at all — orphaned lock, must be removed.
        fs::write(dir.path().join("daemon.6002.spawn.lock"), []).expect("write orphan lock");

        // iter-131 throttle-state file living in the same directory — GC
        // must never touch it regardless of any pid logic.
        fs::write(dir.path().join("daemon.6003.throttle.json"), b"{}").expect("write throttle");

        gc_stale_spawn_locks_in(dir.path());

        assert!(
            !dir.path().join("daemon.6000.spawn.lock").exists(),
            "dead-pid lock must be removed"
        );
        assert!(
            dir.path().join("daemon.6001.spawn.lock").exists(),
            "live-pid lock must survive"
        );
        assert!(
            !dir.path().join("daemon.6002.spawn.lock").exists(),
            "orphaned (no-registry) lock must be removed"
        );
        assert!(
            dir.path().join("daemon.6003.throttle.json").exists(),
            "unrelated *.throttle.json file must survive the GC untouched"
        );
    }

    /// A spawn lock currently held by another process (simulated here by
    /// holding it ourselves via a second file handle) must survive the GC
    /// even though its registry says the pid is dead — the non-blocking
    /// try-lock guard takes precedence over the pid heuristic.
    #[test]
    fn gc_stale_spawn_locks_never_removes_a_currently_held_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_registry_in(dir.path(), &info_with_pid(6000, DEAD_PID)).expect("write dead");
        // Acquire (and hold) the lock ourselves — simulates a concurrent
        // acquirer whose flock the GC must not steal out from under it.
        let _held = acquire_spawn_lock_in(dir.path(), 6000).expect("acquire");

        gc_stale_spawn_locks_in(dir.path());

        assert!(
            dir.path().join("daemon.6000.spawn.lock").exists(),
            "a lock currently held by someone else must never be deleted"
        );
    }

    /// GC on a directory with no spawn-lock files at all must be a silent
    /// no-op (not an error, not a panic).
    #[test]
    fn gc_stale_spawn_locks_noop_on_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        gc_stale_spawn_locks_in(dir.path());
    }

    /// GC against a nonexistent directory must not panic — the daemon spawn
    /// path calls this best-effort and must never fail the real spawn
    /// attempt over a housekeeping sweep.
    #[test]
    fn gc_stale_spawn_locks_tolerates_missing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        gc_stale_spawn_locks_in(&missing);
    }

    // -----------------------------------------------------------------
    // iter-142 Theme B: legacy (port-less) spawn-lock GC
    // -----------------------------------------------------------------

    /// AC `unit_legacy_spawn_lock_collected`: the pre-iter-123 port-less
    /// `daemon.spawn.lock` name — which `parse_spawn_lock_port` can never
    /// match, so `gc_stale_spawn_locks_in` never touches it — is removed by
    /// the dedicated legacy sweep.
    #[test]
    fn unit_legacy_spawn_lock_collected() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(LEGACY_SPAWN_LOCK_FILENAME), []).expect("write legacy lock");

        // Confirm the per-port GC really does leave it alone — that's the
        // defect this dedicated sweep exists to close.
        gc_stale_spawn_locks_in(dir.path());
        assert!(
            dir.path().join(LEGACY_SPAWN_LOCK_FILENAME).exists(),
            "sanity: the per-port GC must not collect the legacy name"
        );

        gc_legacy_spawn_lock_in(dir.path());
        assert!(
            !dir.path().join(LEGACY_SPAWN_LOCK_FILENAME).exists(),
            "the legacy port-less lock must be collected"
        );
    }

    /// A legacy lock file currently flock-held by another process must
    /// survive the sweep — same non-negotiable safety property as the
    /// per-port GC.
    #[test]
    fn unit_legacy_spawn_lock_held_lock_survives() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(LEGACY_SPAWN_LOCK_FILENAME);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .expect("create legacy lock");
        file.lock_exclusive().expect("hold the lock");

        gc_legacy_spawn_lock_in(dir.path());

        assert!(
            path.exists(),
            "a legacy lock currently held by someone else must never be deleted"
        );
    }

    /// GC against a directory with no legacy lock, or a nonexistent
    /// directory, must be a silent no-op.
    #[test]
    fn unit_legacy_spawn_lock_noop_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        gc_legacy_spawn_lock_in(dir.path());
        gc_legacy_spawn_lock_in(&dir.path().join("does-not-exist"));
    }
}
