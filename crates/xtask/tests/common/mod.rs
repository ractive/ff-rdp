//! Shared helpers for `xtask`'s integration tests.
//!
//! `tests/common/mod.rs` is a module, not a test target — Cargo compiles only
//! the top-level `.rs` files under `tests/` as targets — so this file is
//! included with `mod common;` by the tests that need it.

/// Render a finished child process's whole result: exit status, stdout **and**
/// stderr.
///
/// iter-246 Part C: `crates/xtask/tests` is now covered by
/// `ff-rdp-cli`'s `unit_179_no_assertion_reports_stderr_without_stdout` scan,
/// whose rule is that a panic message naming `stderr` must name `stdout` too.
/// The rule is not a formality here: `check_firefox_refs::valid_in_range_ref_passes`
/// failed once during a workspace run with `stderr` empty and nothing else in
/// the message, so the failure carried no evidence at all — the `xtask` binary
/// writes its own progress to stdout, and an `anyhow` chain can surface on
/// either stream depending on the path.
///
/// Deliberately the same shape as the live tier's `common::output_note`, which
/// the scan already recognises by name.
pub fn output_note(out: &std::process::Output) -> String {
    format!(
        "status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}
