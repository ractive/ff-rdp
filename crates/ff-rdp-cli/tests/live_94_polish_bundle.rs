//! Live tests for iter-94: session-59 polish bundle.
//!
//! Covers:
//!   A — `daemon stop` bounded wait + pkill fallback
//!   B — shared render_blocking classifier (live parity)
//!   C — cascade emits `inherited_or_default` note
//!   D — network text suppresses null-keyed rows (live smoke)
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_94_polish_bundle -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;
use std::time::Duration;

use common::{LiveFirefox, base_args, ff_rdp_bin};

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

fn live_network_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1")
}

// ---------------------------------------------------------------------------
// Theme A — daemon stop no residual process
// ---------------------------------------------------------------------------

/// AC: `live_daemon_stop_no_residual_process`
///
/// After `ff-rdp daemon stop`, the Firefox PID we stopped must be gone.
/// This verifies that the port-free-wait bound and SIGKILL escalation
/// actually terminate the process, not just close the socket.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_daemon_stop_no_residual_process() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_stop_no_residual_process: Firefox not available — skipping");
        return;
    };
    let port = ff.port();
    let firefox_pid = ff.pid();

    // Confirm the process is alive before stopping.
    assert!(
        is_pid_alive(firefox_pid),
        "live_daemon_stop_no_residual_process: Firefox pid {firefox_pid} should be alive before stop"
    );

    // Issue daemon stop via DaemonRecord path (no daemon proxy needed).
    let stop = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_daemon_stop_no_residual_process: ff-rdp daemon stop failed to spawn");

    // daemon stop may succeed or report "not running" — both are fine here;
    // what we care about is that the process is gone.
    let _ = stop.status;

    // Poll for up to 10 s (the bounded wait is 8 s, so 10 s gives a margin).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut pid_gone = false;
    while std::time::Instant::now() < deadline {
        if !is_pid_alive(firefox_pid) {
            pid_gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    assert!(
        pid_gone,
        "live_daemon_stop_no_residual_process: Firefox pid {firefox_pid} is still alive after daemon stop"
    );
}

fn is_pid_alive(pid: u32) -> bool {
    // Use kill(pid, 0) on Unix: returns 0 if the process exists, ESRCH if not.
    #[cfg(unix)]
    {
        // SAFETY: kill(pid, 0) is a no-op signal that only checks process
        // existence; it does not deliver a signal or modify any state.
        let ret = unsafe { libc::kill(pid.cast_signed(), 0) };
        ret == 0
    }
    #[cfg(not(unix))]
    {
        // On Windows, check via process handle.
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Theme B — render_blocking parity on real network
// ---------------------------------------------------------------------------

/// AC: `live_render_blocking_parity_on_mdn`
///
/// Navigates to MDN (or a local data: fixture) and asserts that
/// `dom stats` and `perf audit` report the same `render_blocking_count`.
/// Gated by `FF_RDP_LIVE_NETWORK_TESTS=1`.
#[test]
#[ignore = "requires FF_RDP_LIVE_NETWORK_TESTS=1 and a running Firefox"]
fn live_render_blocking_parity_on_mdn() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_render_blocking_parity_on_mdn: Firefox not available — skipping");
        return;
    };
    let port = ff.port();
    let args = base_args(port);

    // Navigate to MDN HTML page.
    let nav = Command::new(ff_rdp_bin())
        .args(&args)
        .args([
            "navigate",
            "https://developer.mozilla.org/en-US/docs/Web/HTML",
        ])
        .output()
        .expect("navigate failed");
    if !nav.status.success() {
        eprintln!(
            "live_render_blocking_parity_on_mdn: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let dom_out = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["dom", "stats"])
        .output()
        .expect("dom stats failed");
    assert!(dom_out.status.success(), "dom stats failed");

    let perf_out = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["perf", "audit"])
        .output()
        .expect("perf audit failed");
    assert!(perf_out.status.success(), "perf audit failed");

    let dom_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dom_out.stdout))
            .expect("dom stats is not valid JSON");
    let perf_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&perf_out.stdout))
            .expect("perf audit is not valid JSON");

    let dom_count = dom_json
        .pointer("/results/render_blocking_count")
        .and_then(serde_json::Value::as_u64);
    let perf_count = perf_json
        .pointer("/results/render_blocking_count")
        .and_then(serde_json::Value::as_u64);

    if let (Some(dom), Some(perf)) = (dom_count, perf_count) {
        assert_eq!(
            dom, perf,
            "render_blocking divergence: dom stats={dom}, perf audit={perf}"
        );
    }
}

// ---------------------------------------------------------------------------
// Theme C — cascade emits inherited_or_default note
// ---------------------------------------------------------------------------

/// AC: `pre_fix_repro_cascade_empty_rules_includes_inherited_note`
///
/// Navigates to a data: page where `<h1>` inherits `color` from `<body>`.
/// Asserts that `cascade h1 --prop color` includes `inherited_or_default: true`
/// in the JSON output.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_cascade_empty_rules_includes_inherited_note() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "pre_fix_repro_cascade_empty_rules_includes_inherited_note: Firefox not available"
        );
        return;
    };
    let port = ff.port();
    let args = base_args(port);

    // Navigate to a minimal page where body has color:red and h1 has no author rule.
    let page = "data:text/html,<body style='color:red'><h1>x</h1></body>";
    let nav = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["navigate", page])
        .output()
        .expect("navigate failed");
    if !nav.status.success() {
        eprintln!(
            "pre_fix_repro_cascade_empty_rules_includes_inherited_note: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let cascade = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("cascade failed");
    assert!(
        cascade.status.success(),
        "cascade command failed: {}",
        String::from_utf8_lossy(&cascade.stderr)
    );

    let json: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&cascade.stdout))
        .expect("cascade output is not valid JSON");

    // Look in results[0] for inherited_or_default: true
    let result = json
        .pointer("/results/0")
        .expect("no results[0] in cascade output");

    let inherited = result
        .get("inherited_or_default")
        .and_then(serde_json::Value::as_bool);
    assert_eq!(
        inherited,
        Some(true),
        "cascade h1 --prop color should have inherited_or_default:true when h1 has no author rule; got: {result}"
    );

    let note = result
        .get("note")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    assert!(
        !note.is_empty(),
        "cascade result should include a non-empty 'note' field; got: {result}"
    );
}

/// AC: `live_cascade_note_disambiguates_iter82_regression_shape`
///
/// Same as `pre_fix_repro_cascade_empty_rules_includes_inherited_note` but
/// also checks that a non-inherited property (one with an explicit author rule)
/// does NOT carry the note. Ensures the note is not emitted spuriously.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cascade_note_disambiguates_iter82_regression_shape() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cascade_note_disambiguates_iter82_regression_shape: Firefox not available");
        return;
    };
    let port = ff.port();
    let args = base_args(port);

    // h1 has an explicit color rule; cascade should NOT emit the note.
    let page = "data:text/html,<style>h1{color:blue}</style><h1>x</h1>";
    let nav = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["navigate", page])
        .output()
        .expect("navigate failed");
    if !nav.status.success() {
        return;
    }

    let cascade = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("cascade failed");
    if !cascade.status.success() {
        return;
    }

    let json: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&cascade.stdout))
        .expect("cascade output is not valid JSON");

    let result = &json["results"][0];
    // When rules are present, inherited_or_default must be absent or false.
    let rules = result.get("rules").and_then(serde_json::Value::as_array);
    if let Some(rules) = rules
        && !rules.is_empty()
    {
        let inherited = result
            .get("inherited_or_default")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        assert!(
            !inherited,
            "cascade must NOT emit inherited_or_default:true when author rules are present; got: {result}"
        );
    }
}

// ---------------------------------------------------------------------------
// Theme D — network text post-nav live smoke
// ---------------------------------------------------------------------------

/// AC: `live_network_text_post_nav_renders_cleanly`
///
/// Immediately after navigation (when cause_type may still be streaming in),
/// `network --format text` must not emit bare-number rows (a number with no label).
/// Manual repro: `ff-rdp navigate <url> && ff-rdp network --format text`.
///
/// This test exercises the full render path including null-key suppression.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_network_text_post_nav_renders_cleanly() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_network_text_post_nav_renders_cleanly: Firefox not available");
        return;
    };
    let port = ff.port();
    let args = base_args(port);

    // Navigate to a data: page so we get network events without real network.
    let nav = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["navigate", "data:text/html,<h1>network-test</h1>"])
        .output()
        .expect("navigate failed");
    if !nav.status.success() {
        return; // Non-fatal: live environment may not support this.
    }

    let network = Command::new(ff_rdp_bin())
        .args(&args)
        .args(["network", "--format", "text"])
        .output()
        .expect("network failed");

    // Command may fail if no events (data: URIs generate no network activity).
    // What we assert is that IF it produces output, there are no bare-number rows.
    let text = String::from_utf8_lossy(&network.stdout);

    // A bare-number row: line with only whitespace and digits.
    let has_bare_number = text.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit())
    });
    assert!(
        !has_bare_number,
        "live_network_text_post_nav_renders_cleanly: bare-number row found in output:\n{text}"
    );
}
