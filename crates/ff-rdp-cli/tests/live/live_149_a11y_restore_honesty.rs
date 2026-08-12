//! Live tests for iteration 149 — `a11y --native` must report a failed
//! service restore.
//!
//! Follow-up to [[iteration-143-native-a11y-tree]]: `run_native_opt_in`
//! (`crates/ff-rdp-cli/src/commands/a11y.rs`) enables Firefox's platform
//! accessibility service on `--native` when it was off, and restores it to
//! disabled afterward — but until this iteration, a failed restore was only
//! ever reported via `--verbose` stderr, never in the default JSON envelope.
//! That left Firefox's platform accessibility service enabled browser-wide
//! for the rest of the process with no trace in the command's own output.
//!
//! `meta.service_left_enabled` / `meta.service_restore_error` are now always
//! present (iter-128 always-present-nullable-key convention) on every `a11y`
//! response.
//!
//! # Actor-boundary fault injection
//!
//! Forcing a real `disable()` failure against Firefox is otherwise only
//! reachable on Windows with an active screen reader blocking the call
//! (`kb/rdp/actors/accessibility.md`) — not reproducible against the
//! headless macOS/Linux Firefox this suite runs against. Per the iteration
//! plan's Notes, `live_149_restore_failure_reported_in_meta` and
//! `live_149_service_already_on_is_not_touched` set
//! `FF_RDP_A11Y_FORCE_RESTORE_FAILURE=1` on the child process, which makes
//! `run_native_opt_in` target the *restore* call at a deliberately-invalid
//! actor ID. Firefox still genuinely answers with a `noSuchActor`-style wire
//! error — this is a real protocol failure, not a mocked one — while
//! `enable_service` is unaffected, so the service really is left enabled
//! afterward.
//!
//! # Daemon routing
//!
//! `a11y` always calls `connect_direct` (like `screenshot`, `cookies`,
//! `storage`, `sources`, `computed`) — confirmed by reading
//! `crates/ff-rdp-cli/src/commands/a11y.rs` before writing these tests, per
//! the plan's "verify on the wire first" rule. There is no daemon-routed
//! code path for `a11y` to additionally exercise: every invocation here
//! already takes the one and only connection path this command has.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_149_a11y_restore_honesty -- --nocapture

use std::process::{Command, Output};
use std::time::Duration;

use ff_rdp_core::{AccessibilityActor, ActorId, RdpConnection, RootActor};
use serde_json::Value;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

/// Env var `run_native_opt_in`'s restore step reads to corrupt its own
/// disable-target actor ID (see module doc). Mirrors the constant name used
/// in `crates/ff-rdp-cli/src/commands/a11y.rs`.
const FORCE_RESTORE_FAILURE_ENV: &str = "FF_RDP_A11Y_FORCE_RESTORE_FAILURE";

fn parse_json(output: &Output) -> Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_a11y(port: u16, extra: &[&str], force_restore_failure: bool) -> Output {
    let mut args = base_args(port);
    args.push("a11y".to_owned());
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let mut cmd = Command::new(ff_rdp_bin());
    cmd.args(&args);
    if force_restore_failure {
        cmd.env(FORCE_RESTORE_FAILURE_ENV, "1");
    } else {
        cmd.env_remove(FORCE_RESTORE_FAILURE_ENV);
    }
    cmd.output().expect("ff-rdp a11y")
}

fn run_a11y_json(port: u16, extra: &[&str], force_restore_failure: bool) -> Value {
    let out = run_a11y(port, extra, force_restore_failure);
    assert!(
        out.status.success(),
        "ff-rdp a11y {extra:?} (force_restore_failure={force_restore_failure}) failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    parse_json(&out)
}

/// AC: `live_149_restore_failure_reported_in_meta` — when `disable_service`
/// fails after ff-rdp enabled the service, the JSON envelope carries the
/// left-enabled signal and the reason, and the walked tree is still returned
/// in `results`.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_149_restore_failure_reported_in_meta() {
    if !live_tests_enabled() {
        eprintln!("live_149_restore_failure_reported_in_meta: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_149_restore_failure_reported_in_meta: Firefox not available — skipping");
        return;
    };

    let json = run_a11y_json(ff.port(), &["--native"], true);

    assert_eq!(
        json["meta"]["service_left_enabled"], true,
        "a failed restore must set meta.service_left_enabled = true: {json}"
    );
    let error = json["meta"]["service_restore_error"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "meta.service_restore_error must be a non-null string on a failed restore: {json}"
            )
        });
    assert!(
        !error.is_empty(),
        "meta.service_restore_error must not be empty: {json}"
    );
    assert_eq!(
        json["results"]["role"], "document",
        "the walked native tree must still be returned even though the restore failed: {json}"
    );
}

/// AC: `live_149_successful_restore_reports_clean` — a normal `--native` run
/// that restores the service reports no left-enabled signal, and the service
/// is observably disabled afterwards.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_149_successful_restore_reports_clean() {
    if !live_tests_enabled() {
        eprintln!("live_149_successful_restore_reports_clean: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_149_successful_restore_reports_clean: Firefox not available — skipping");
        return;
    };

    let opted_in = run_a11y_json(ff.port(), &["--native"], false);
    assert_eq!(
        opted_in["meta"]["service_left_enabled"], false,
        "a successful restore must report service_left_enabled = false: {opted_in}"
    );
    assert!(
        opted_in["meta"]["service_restore_error"].is_null(),
        "a successful restore must not carry a restore error: {opted_in}"
    );

    // Observably disabled afterwards: a plain (non-`--native`) call must take
    // the JS-fallback path again, same technique as live_143's restore check.
    let after = run_a11y_json(ff.port(), &[], false);
    assert_eq!(
        after["meta"]["source"], "js-fallback",
        "the accessibility service must be restored to disabled after a clean \
         --native restore: {after}"
    );
}

/// Open an independent RDP connection, enable the platform accessibility
/// service via `parentAccessibilityActor`, and **keep the connection alive**
/// by returning it (the caller must hold it until the "already enabled"
/// precondition is no longer needed).
///
/// This suite initially tried to establish "already enabled" by leaving a
/// *single* `ff-rdp` CLI invocation's own connection enabled via the
/// actor-boundary injection (same technique as
/// `live_149_restore_failure_reported_in_meta`) and then checking a second,
/// separate CLI invocation. That failed: verified on the wire, Firefox
/// re-disables the platform accessibility service once the connection that
/// enabled it disconnects, regardless of whether an explicit `disable()`
/// call ran — a *new* connection always observes the service off again. So
/// "already enabled, from another consumer's point of view" only holds while
/// some connection genuinely keeps it open, which is what this helper does.
fn hold_service_enabled(port: u16) -> RdpConnection {
    let mut conn = RdpConnection::connect("127.0.0.1", port, Duration::from_secs(10))
        .expect("raw connect to hold the accessibility service open");
    let root_form = RootActor::get_root(conn.transport_mut()).expect("getRoot");
    let parent_actor: ActorId = root_form
        .get("parentAccessibilityActor")
        .and_then(Value::as_str)
        .expect("root form must expose parentAccessibilityActor")
        .into();
    AccessibilityActor::enable_service(conn.transport_mut(), &parent_actor)
        .expect("enable_service on the held connection");
    conn
}

/// AC: `live_149_service_already_on_is_not_touched` — when the service was
/// already enabled before the command, ff-rdp neither disables it nor claims
/// to have left it enabled.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_149_service_already_on_is_not_touched() {
    if !live_tests_enabled() {
        eprintln!("live_149_service_already_on_is_not_touched: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_149_service_already_on_is_not_touched: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // Hold a second, independent connection that enables the service and
    // stays connected — the "already on, from some other consumer" state
    // this test needs must outlive any single CLI invocation below.
    let holder = hold_service_enabled(port);

    // A --native call now finds the service already on (was_enabled=true):
    // it must not call enable() or disable() at all — RestoreOutcome::NotNeeded.
    let native = run_a11y_json(port, &["--native"], false);
    assert_eq!(
        native["meta"]["service_left_enabled"], false,
        "ff-rdp did not enable the service this call, so it must not claim to \
         have left it enabled: {native}"
    );
    assert!(
        native["meta"]["service_restore_error"].is_null(),
        "there is nothing to restore when the service was already on: {native}"
    );

    // Confirm the service is still enabled afterward (not disabled by the
    // call above) — a plain a11y call takes the native path only while the
    // service is on.
    let after = run_a11y_json(port, &[], false);
    assert_eq!(
        after["meta"]["source"], "native",
        "the service must still be enabled after a --native call that found it \
         already on: {after}"
    );

    drop(holder);
}
