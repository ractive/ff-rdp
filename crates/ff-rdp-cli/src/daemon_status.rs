//! Process-global recorder for daemon-lifecycle warnings surfaced in the
//! command output envelope (iter-100 Theme E).
//!
//! When auto-start does not yield a usable daemon and the CLI transparently
//! falls back to a *direct* Firefox connection, the command still succeeds —
//! but the caller (a test, a script, or a human) previously had no way to
//! tell "used the daemon" apart from "quietly went direct". That silent
//! degradation hid the daemon-registration race the deep review found.
//!
//! `resolve_connection_target` records a [`record_autostart_failed`] warning
//! here when it takes the fallback path. [`crate::output_pipeline`] drains the
//! recorded warnings once, at `finalize` time, and injects them as a top-level
//! `"warnings"` array on the envelope — so every command that ran through the
//! daemon-resolution path surfaces the diagnostic without each command needing
//! to plumb it through by hand.
//!
//! The design mirrors [`crate::connection_meta`]'s `OnceLock<Mutex<…>>`
//! remembered-version pattern: a single process-global slot, poison-tolerant,
//! populated on the resolve path and read on the output path.

use std::sync::{Mutex, OnceLock};

/// The `error_type`-style tag emitted for an auto-start failure warning.
///
/// Kept as a named constant so tests and any future consumers assert against
/// one source of truth rather than a scattered string literal.
pub(crate) const AUTOSTART_FAILED_TYPE: &str = "daemon_autostart_failed";

/// A single recorded daemon-lifecycle warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DaemonWarning {
    /// Machine-readable tag (e.g. [`AUTOSTART_FAILED_TYPE`]).
    pub warning_type: String,
    /// Short human-readable reason.
    pub reason: String,
}

static WARNINGS: OnceLock<Mutex<Vec<DaemonWarning>>> = OnceLock::new();

fn slot() -> &'static Mutex<Vec<DaemonWarning>> {
    WARNINGS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Record that auto-start did not yield a usable daemon and the CLI fell back
/// to a direct connection (iter-100 Theme E).
///
/// `reason` is a short explanation (e.g. "daemon started but registry not
/// found within 5s"). Never a hard error — direct mode still works — so this
/// only annotates the envelope, it never aborts the command.
pub(crate) fn record_autostart_failed(reason: impl Into<String>) {
    let warning = DaemonWarning {
        warning_type: AUTOSTART_FAILED_TYPE.to_owned(),
        reason: reason.into(),
    };
    let mut guard = slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // De-duplicate identical warnings so a retry loop does not emit the same
    // line repeatedly.
    if !guard.contains(&warning) {
        guard.push(warning);
    }
}

/// Return (and clear) all warnings recorded so far.
///
/// Called once by the output pipeline while building the final envelope.
/// Draining (rather than cloning) keeps a long-lived process — e.g. a future
/// REPL — from re-emitting stale warnings on the next command.
pub(crate) fn take_warnings() -> Vec<DaemonWarning> {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::mem::take(&mut *guard)
}

/// Serialize the currently-recorded warnings into a JSON array, draining them.
///
/// Returns `None` when there are no warnings, so the caller can omit the
/// `"warnings"` key entirely on the happy path (keeping default output
/// compact).
pub(crate) fn take_warnings_json() -> Option<serde_json::Value> {
    let warnings = take_warnings();
    if warnings.is_empty() {
        return None;
    }
    let arr: Vec<serde_json::Value> = warnings
        .into_iter()
        .map(|w| {
            serde_json::json!({
                "type": w.warning_type,
                "reason": w.reason,
            })
        })
        .collect();
    Some(serde_json::Value::Array(arr))
}

/// Serialization lock for tests that exercise the process-global [`WARNINGS`]
/// slot (iter-123).
///
/// The recorder is a single process-wide slot, so any two tests that run a
/// `record → take`/`assert-exact-count` sequence concurrently can observe each
/// other's writes and flake.  Every such test — here *and* in
/// `daemon/client.rs` — must hold this lock for the duration of its
/// record/assert sequence, which serializes them across the whole test binary.
/// Poison-tolerant: a panicking test still releases a usable guard to the next.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests share a process-global slot; each holds `test_lock()` for its
    // whole record/assert sequence so they never observe each other's writes,
    // then drains at the end to leave the slot clean for the next holder.

    #[test]
    fn record_then_take_roundtrips() {
        let _guard = test_lock();
        let _ = take_warnings(); // clear any residue
        record_autostart_failed("registry not found within 5s");
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].warning_type, AUTOSTART_FAILED_TYPE);
        assert!(warnings[0].reason.contains("registry not found"));
        // Second take is empty — the first drained it.
        assert!(take_warnings().is_empty());
    }

    #[test]
    fn duplicate_warnings_are_deduped() {
        let _guard = test_lock();
        let _ = take_warnings();
        record_autostart_failed("same reason");
        record_autostart_failed("same reason");
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1, "identical warnings must be deduped");
    }

    #[test]
    fn take_json_is_none_when_empty() {
        let _guard = test_lock();
        let _ = take_warnings();
        assert!(
            take_warnings_json().is_none(),
            "no warnings -> None so the envelope omits the key"
        );
    }

    #[test]
    fn take_json_shapes_type_and_reason() {
        let _guard = test_lock();
        let _ = take_warnings();
        record_autostart_failed("spawn died before registry write");
        let json = take_warnings_json().expect("some warnings");
        let arr = json.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], AUTOSTART_FAILED_TYPE);
        assert_eq!(arr[0]["reason"], "spawn died before registry write");
    }
}
