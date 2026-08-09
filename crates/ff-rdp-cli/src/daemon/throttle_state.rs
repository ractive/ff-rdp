//! Client-side bookkeeping for the last `throttle` profile applied via the
//! daemon (iter-131 Theme D).
//!
//! Firefox's network-parent actor exposes `setNetworkThrottling` but no
//! getter — there is no RDP call that answers "what throttling is currently
//! active" (see the doc comment on
//! [`ff_rdp_core::NetworkParentFront::set_network_throttling`]). `throttle
//! status` therefore cannot query Firefox; it recalls the last profile *this
//! daemon* successfully applied, written by `commands::throttle::run` on a
//! successful `throttle <profile>` call.
//!
//! Scoped to the daemon (keyed by the Firefox debug `port`, the same key
//! [`super::registry`] uses) because the daemon is the only session that
//! outlives a single CLI invocation. A `--no-daemon` one-shot connection
//! disconnects immediately and Firefox discards the setting with it (see
//! `commands::throttle::ONE_SHOT_LIFETIME_WARNING`), so nothing is persisted
//! for that case — `throttle status` has nothing truthful to report without a
//! running daemon. If the daemon restarts (new pid), Firefox itself restarts
//! throttling-free, so a throttle-state file tagged with the old pid is
//! stale and must not be reported as active.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::registry;

/// Recorded outcome of the last `throttle <profile>` call this daemon applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleState {
    /// `"slow-3g"` / `"fast-3g"`, or `None` when cleared (`throttle off`).
    pub(crate) profile: Option<String>,
    /// ISO 8601 timestamp of when this state was recorded.
    pub(crate) set_at: String,
    /// PID of the daemon process that recorded this state. Compared against
    /// the current registry entry's pid on read — a mismatch means the
    /// daemon restarted since, and the recorded profile no longer reflects
    /// reality.
    pub(crate) daemon_pid: u32,
}

fn state_filename(port: u16) -> String {
    format!("daemon.{port}.throttle.json")
}

/// Write `state` to `<dir>/daemon.<port>.throttle.json` atomically
/// (write-then-rename), mirroring [`super::registry::write_registry_in`].
pub(crate) fn write_throttle_state_in(dir: &Path, port: u16, state: &ThrottleState) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating registry directory {}", dir.display()))?;

    let filename = state_filename(port);
    let path = dir.join(&filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    let json = serde_json::to_string_pretty(state).context("serializing ThrottleState to JSON")?;
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
    {
        use std::io::Write as _;
        tmp_file
            .write_all(json.as_bytes())
            .with_context(|| format!("writing to tmp file {}", tmp_path.display()))?;
        tmp_file
            .flush()
            .with_context(|| format!("flushing tmp file {}", tmp_path.display()))?;
    }
    drop(tmp_file);

    fs::rename(&tmp_path, &path)
        .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Read `<dir>/daemon.<port>.throttle.json`, returning `None` if absent.
pub(crate) fn read_throttle_state_in(dir: &Path, port: u16) -> Result<Option<ThrottleState>> {
    let path = dir.join(state_filename(port));
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading throttle state at {}", path.display()))?;
    let state: ThrottleState = serde_json::from_str(&contents)
        .with_context(|| format!("parsing throttle state at {}", path.display()))?;
    Ok(Some(state))
}

// ---------------------------------------------------------------------------
// Public convenience wrappers that use the real `~/.ff-rdp/` directory.
// ---------------------------------------------------------------------------

/// Record `state` for `port` in the real registry directory.
pub fn write_throttle_state(port: u16, state: &ThrottleState) -> Result<()> {
    write_throttle_state_in(&registry::registry_dir()?, port, state)
}

/// Read the recorded throttle state for `port` from the real registry
/// directory.
pub fn read_throttle_state(port: u16) -> Result<Option<ThrottleState>> {
    read_throttle_state_in(&registry::registry_dir()?, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state(pid: u32) -> ThrottleState {
        ThrottleState {
            profile: Some("slow-3g".to_owned()),
            set_at: "2026-08-09T00:00:00Z".to_owned(),
            daemon_pid: pid,
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = sample_state(1234);
        write_throttle_state_in(dir.path(), 6000, &state).expect("write");
        let read = read_throttle_state_in(dir.path(), 6000)
            .expect("read")
            .expect("some");
        assert_eq!(read.profile, Some("slow-3g".to_owned()));
        assert_eq!(read.daemon_pid, 1234);
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            read_throttle_state_in(dir.path(), 6000)
                .expect("read")
                .is_none()
        );
    }

    #[test]
    fn per_port_writes_do_not_clobber() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_throttle_state_in(dir.path(), 6000, &sample_state(1)).expect("write 6000");
        write_throttle_state_in(dir.path(), 6001, &sample_state(2)).expect("write 6001");
        let a = read_throttle_state_in(dir.path(), 6000)
            .expect("read")
            .expect("some");
        let b = read_throttle_state_in(dir.path(), 6001)
            .expect("read")
            .expect("some");
        assert_eq!(a.daemon_pid, 1);
        assert_eq!(b.daemon_pid, 2);
    }

    #[test]
    fn overwrite_updates_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_throttle_state_in(dir.path(), 6000, &sample_state(1)).expect("write");
        let mut cleared = sample_state(1);
        cleared.profile = None;
        write_throttle_state_in(dir.path(), 6000, &cleared).expect("overwrite");
        let read = read_throttle_state_in(dir.path(), 6000)
            .expect("read")
            .expect("some");
        assert_eq!(read.profile, None);
    }

    #[test]
    fn read_corrupt_json_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(state_filename(6000)), b"not json").expect("write garbage");
        assert!(read_throttle_state_in(dir.path(), 6000).is_err());
    }
}
