//! iter-164 defect 2 (harness half) — `LiveFirefox::with_daemon` must *poll*
//! for the daemon's registry entry instead of sleeping a fixed 500 ms.
//!
//! iter-158's `live-sweep` ran at load average 18.6 and produced:
//!
//! ```text
//! ---- live_141_output_hygiene::live_141_text_empty_result_keeps_metadata stdout ----
//! panicked at crates/ff-rdp-cli/tests/live/live_141_output_hygiene.rs:59:5:
//!   live_141_text_empty_result_keeps_metadata: the proxy daemon did not start
//!   for Firefox on port 61670
//! ```
//!
//! The daemon *did* start — just not inside the harness's fixed 500 ms sleep.
//! These tests pin the polling contract against a stub registry, so the helper
//! is verifiable with no Firefox and no daemon anywhere in sight. (Firefox-free
//! and ungated, so it runs on every `cargo test`.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

#[path = "common/mod.rs"]
mod common;

use common::{daemon_port_from_status, daemon_ready_timeout, poll_for_daemon_port};

/// AC `unit_164_with_daemon_polls_instead_of_sleeping`: a daemon that registers
/// *after* the old 500 ms sleep would have elapsed is still found, and is found
/// as soon as it appears rather than after the full budget.
#[test]
fn unit_164_with_daemon_polls_instead_of_sleeping() {
    let attempts = AtomicUsize::new(0);
    let started = Instant::now();

    // Stub registry: "not running" for the first 8 probes (~800 ms at the
    // helper's 100 ms cadence — comfortably past the old fixed 500 ms sleep),
    // then a registered daemon on proxy port 54321.
    let found = poll_for_daemon_port(Duration::from_secs(10), || {
        let n = attempts.fetch_add(1, Ordering::SeqCst);
        let status = if n < 8 {
            json!({"results": {"running": false}})
        } else {
            json!({"results": {"running": true, "port": 54321}})
        };
        daemon_port_from_status(&status)
    });

    assert_eq!(
        found,
        Some(54321),
        "a daemon that registers after 500 ms must still be found"
    );
    assert!(
        attempts.load(Ordering::SeqCst) >= 9,
        "the helper must actually re-probe, not sleep once and give up (attempts={})",
        attempts.load(Ordering::SeqCst)
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the poll must return as soon as the daemon appears, not burn the whole budget"
    );
}

/// The poll is *bounded*: a daemon that never registers yields `None` at the
/// deadline rather than hanging the suite.
#[test]
fn unit_164_daemon_poll_is_bounded() {
    let attempts = AtomicUsize::new(0);
    let started = Instant::now();
    let found = poll_for_daemon_port(Duration::from_millis(400), || {
        attempts.fetch_add(1, Ordering::SeqCst);
        daemon_port_from_status(&json!({"results": {"running": false}}))
    });
    assert_eq!(found, None, "a daemon that never starts must report None");
    assert!(
        attempts.load(Ordering::SeqCst) >= 2,
        "a 400 ms budget at a 100 ms cadence must allow several probes"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the bound must be honoured; took {:?}",
        started.elapsed()
    );
}

/// A zero budget still probes once — "no time left" must not degrade to "never
/// checked", which would report a perfectly healthy daemon as absent.
#[test]
fn unit_164_zero_budget_still_probes_once() {
    let attempts = AtomicUsize::new(0);
    let found = poll_for_daemon_port(Duration::ZERO, || {
        attempts.fetch_add(1, Ordering::SeqCst);
        daemon_port_from_status(&json!({"results": {"running": true, "port": 7000}}))
    });
    assert_eq!(found, Some(7000));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

/// `daemon status` shapes the helper must read correctly.
#[test]
fn unit_164_daemon_port_from_status_shapes() {
    assert_eq!(
        daemon_port_from_status(&json!({"results": {"running": true, "port": 6001}})),
        Some(6001)
    );
    // Not running -> None even when a stale port is present.
    assert_eq!(
        daemon_port_from_status(&json!({"results": {"running": false, "port": 6001}})),
        None
    );
    // Running but no usable port -> None (never fabricate one).
    assert_eq!(
        daemon_port_from_status(&json!({"results": {"running": true}})),
        None
    );
    assert_eq!(
        daemon_port_from_status(&json!({"results": {"running": true, "port": 999_999}})),
        None,
        "an out-of-range port must not be truncated into a plausible one"
    );
    // An error envelope (no `results`) is simply "not running".
    assert_eq!(
        daemon_port_from_status(&json!({"error": "no daemon", "error_type": "Connection"})),
        None
    );
}

/// The harness budget must comfortably exceed the product's own autostart
/// budget, so a harness timeout can never be mistaken for a product failure.
#[test]
fn unit_164_harness_budget_exceeds_product_autostart_budget() {
    // Product default is 20 s (`DEFAULT_REGISTRY_WAIT_MS` in
    // `src/daemon/client.rs`); duplicated here because this crate ships no
    // `[lib]` target for an integration test to import from.
    let product_budget = Duration::from_secs(20);
    assert!(
        daemon_ready_timeout() > product_budget,
        "harness budget {:?} must exceed the product's autostart budget {product_budget:?}",
        daemon_ready_timeout()
    );
}
