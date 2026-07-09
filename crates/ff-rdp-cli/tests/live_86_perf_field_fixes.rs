//! Live tests for iter-86: perf field-report fixes.
//!
//! Covers all five themes:
//!   A — `daemon stop` frees the Firefox RDP port (kills process group)
//!   B — `lcp_note` does NOT mention "headless" (verified under headless launch;
//!         the message is constructed without consulting launch mode, so this
//!         covers the regardless-of-mode guarantee in the only mode CI can run)
//!   C — render-blocking filter matches spec (stylesheets + sync scripts only)
//!   D — `--jq` missing path: silent by default, non-zero with `--jq-strict`
//!   E — `perf audit --help` mentions Lighthouse
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_86_perf_field_fixes -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;
use std::time::Duration;

use common::{LiveFirefox, base_args, ff_rdp_bin};

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

// ---------------------------------------------------------------------------
// Theme A — daemon stop frees port
// ---------------------------------------------------------------------------

/// `live_daemon_stop_frees_port`:
/// After `ff-rdp daemon stop`, the Firefox RDP port 6000 must be closed
/// (i.e. no TCP listener) within 3 seconds.
///
/// Pre-condition: Firefox running (launched by this test), daemon started.
/// Post-condition: `TcpStream::connect("127.0.0.1:<port>")` fails after stop.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_daemon_stop_frees_port() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_stop_frees_port: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // Start the daemon.
    let Some(_daemon_port) = ff.with_daemon() else {
        eprintln!("live_daemon_stop_frees_port: daemon did not start — skipping");
        return;
    };

    // Confirm the port is in use before we stop.
    assert!(
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok(),
        "live_daemon_stop_frees_port: expected Firefox RDP port {port} to be open before stop"
    );

    // Issue daemon stop.  The daemon kills the Firefox process group and waits
    // for the port to close (Theme A).
    let stop = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_daemon_stop_frees_port: ff-rdp daemon stop failed to spawn");

    assert!(
        stop.status.success(),
        "live_daemon_stop_frees_port: daemon stop returned non-zero — stderr={}",
        String::from_utf8_lossy(&stop.stderr)
    );

    // The port must be closed within 3 s (daemon already waited up to 3 s).
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let mut port_closed = false;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_err() {
            port_closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // LiveFirefox Drop will attempt kill_pid — fine if process is already gone.
    assert!(
        port_closed,
        "live_daemon_stop_frees_port: FAIL — Firefox RDP port {port} still \
         listening after daemon stop (Theme A regression)"
    );

    eprintln!("live_daemon_stop_frees_port: PASS — port {port} closed after daemon stop");
}

/// `live_launch_replace_handles_stuck_prior`:
/// When a prior Firefox instance holds the default port, `ff-rdp launch --replace`
/// must stop the prior instance and succeed.
///
/// Pre-condition: Firefox running on port 6000 (launched by test).
/// Post-condition: `launch --replace` exits 0.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_launch_replace_handles_stuck_prior() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_launch_replace_handles_stuck_prior: Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // Confirm the port is occupied.
    assert!(
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok(),
        "live_launch_replace_handles_stuck_prior: port {port} not open before replace"
    );

    // Launch with --replace targeting the same port.
    let out = Command::new(ff_rdp_bin())
        .args([
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ])
        .output()
        .expect("live_launch_replace_handles_stuck_prior: failed to spawn ff-rdp launch");

    assert!(
        out.status.success(),
        "live_launch_replace_handles_stuck_prior: FAIL — launch --replace returned non-zero\n\
         stderr={}\nstdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    eprintln!(
        "live_launch_replace_handles_stuck_prior: PASS — launch --replace succeeded on port {port}"
    );
}

// ---------------------------------------------------------------------------
// Theme B — lcp_note does not mention "headless"
// ---------------------------------------------------------------------------

/// `live_lcp_note_no_headless_text_in_vitals`:
/// `ff-rdp perf vitals` must NOT contain the word "headless" in any lcp_note
/// field, regardless of whether Firefox was launched in headless mode.
///
/// Pre-condition: Firefox running (launched headless by this test).
/// Post-condition: `lcp_note` (if present) does not contain "headless".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_lcp_note_no_headless_text_in_vitals() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_lcp_note_no_headless_text_in_vitals: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_lcp_note_no_headless_text_in_vitals: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "vitals"])
        .output()
        .expect("ff-rdp perf vitals");
    assert!(
        out.status.success(),
        "live_lcp_note_no_headless_text_in_vitals: perf vitals failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("live_lcp_note_no_headless_text_in_vitals: not valid JSON: {e}\n{stdout}")
    });

    if let Some(note) = json["results"]["lcp_note"].as_str() {
        assert!(
            !note.to_lowercase().contains("headless"),
            "live_lcp_note_no_headless_text_in_vitals: FAIL — lcp_note mentions 'headless': {note:?}"
        );
        assert!(
            note.contains("Firefox"),
            "live_lcp_note_no_headless_text_in_vitals: FAIL — lcp_note should mention Firefox: {note:?}"
        );
        eprintln!("live_lcp_note_no_headless_text_in_vitals: PASS — lcp_note={note:?}");
    } else {
        eprintln!("live_lcp_note_no_headless_text_in_vitals: lcp_note absent — nothing to check");
    }
}

/// `live_lcp_note_mentions_firefox_limitation_in_audit`:
/// `ff-rdp perf audit` lcp_note must NOT contain "headless" and MUST mention
/// "Firefox" and "limitation".
///
/// Pre-condition: Firefox running headless.
/// Post-condition: lcp_note mentions "Firefox" and "limitation", not "headless".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_lcp_note_mentions_firefox_limitation_in_audit() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_lcp_note_mentions_firefox_limitation_in_audit: Firefox not available — skipping"
        );
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_lcp_note_mentions_firefox_limitation_in_audit: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit");
    assert!(
        out.status.success(),
        "live_lcp_note_mentions_firefox_limitation_in_audit: perf audit failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("live_lcp_note_mentions_firefox_limitation_in_audit: not valid JSON: {e}\n{stdout}")
    });

    if let Some(note) = json["results"]["lcp_note"].as_str() {
        assert!(
            !note.to_lowercase().contains("headless"),
            "live_lcp_note_mentions_firefox_limitation_in_audit: FAIL — lcp_note mentions 'headless': {note:?}"
        );
        assert!(
            note.contains("Firefox"),
            "live_lcp_note_mentions_firefox_limitation_in_audit: FAIL — lcp_note must mention 'Firefox': {note:?}"
        );
        assert!(
            note.to_lowercase().contains("limitation"),
            "live_lcp_note_mentions_firefox_limitation_in_audit: FAIL — lcp_note must mention 'limitation': {note:?}"
        );
        eprintln!("live_lcp_note_mentions_firefox_limitation_in_audit: PASS — lcp_note={note:?}");
    } else {
        eprintln!(
            "live_lcp_note_mentions_firefox_limitation_in_audit: lcp_note absent — nothing to check"
        );
    }
}

// ---------------------------------------------------------------------------
// Theme C — render-blocking filter matches spec
// ---------------------------------------------------------------------------

/// `live_render_blocking_excludes_favicon`:
/// `ff-rdp perf audit` must NOT include favicon/icon/preload `<link>` tags in
/// `results.render_blocking` — only `rel=stylesheet` (media matches, not disabled)
/// and synchronous `<script src>` tags block rendering.
///
/// Pre-condition: Firefox navigated to about:blank (no external stylesheets).
/// Post-condition: `results.render_blocking` is an array (may be empty);
///                each element has a `url` field.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_render_blocking_excludes_favicon() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_render_blocking_excludes_favicon: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(nav.status.success(), "navigate failed");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit");
    assert!(
        out.status.success(),
        "live_render_blocking_excludes_favicon: perf audit failed — {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("live_render_blocking_excludes_favicon: not valid JSON: {e}\n{stdout}")
    });

    // results.render_blocking must be an array (Theme C).
    let rb = &json["results"]["render_blocking"];
    assert!(
        rb.is_array(),
        "live_render_blocking_excludes_favicon: FAIL — results.render_blocking is not an array: {rb}"
    );

    let rb_arr = rb.as_array().unwrap();

    // Each entry must have a `url` field.
    for entry in rb_arr {
        assert!(
            entry["url"].is_string(),
            "live_render_blocking_excludes_favicon: FAIL — render_blocking entry has no url: {entry}"
        );
        // Must NOT be a favicon / icon URL.
        let url = entry["url"].as_str().unwrap_or("");
        assert!(
            !url.contains("favicon") && !url.contains(".ico"),
            "live_render_blocking_excludes_favicon: FAIL — favicon found in render_blocking: {url}"
        );
    }

    eprintln!(
        "live_render_blocking_excludes_favicon: PASS — render_blocking has {} entries, all valid",
        rb_arr.len()
    );
}

// ---------------------------------------------------------------------------
// Theme D — --jq missing path behavior
// ---------------------------------------------------------------------------

/// `live_jq_missing_path_silent_default`:
/// By default (no `--jq-strict`), a `--jq` filter that selects a non-existent
/// path produces no output (not `null`).
///
/// Pre-condition: Firefox running.
/// Post-condition: stdout is empty (not "null").
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_jq_missing_path_silent_default() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_jq_missing_path_silent_default: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(nav.status.success(), "navigate failed");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "--jq",
            ".results.this_field_does_not_exist",
            "perf",
            "audit",
        ])
        .output()
        .expect("ff-rdp perf audit --jq");

    // Must exit 0 (no error).
    assert!(
        out.status.success(),
        "live_jq_missing_path_silent_default: FAIL — exited non-zero\nstderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();

    // Must NOT print "null".
    assert!(
        trimmed != "null" && !trimmed.contains("null"),
        "live_jq_missing_path_silent_default: FAIL — output contains 'null': {trimmed:?}"
    );

    eprintln!(
        "live_jq_missing_path_silent_default: PASS — stdout is empty/absent for missing path"
    );
}

/// `live_jq_missing_path_strict_exits_nonzero`:
/// With `--jq-strict`, a `--jq` filter that selects a non-existent path must
/// exit non-zero and print a JSON error envelope containing "not found".
///
/// Pre-condition: Firefox running.
/// Post-condition: exit code != 0, stdout JSON error contains "not found".
///
/// NOTE: this codebase's error contract prints JSON errors to **stdout**
/// (so a script piping stdout as NDJSON still sees the error), not stderr —
/// see `error_shapes.rs`'s `parse_stdout_json` for the established
/// convention. This test previously (incorrectly, since it was first
/// written — predates iter-100) checked `stderr`, which the CLI never
/// writes to for this error path, so the assertion always failed once
/// actually reached. Found while un-masking `live_86_perf_field_fixes.rs`
/// during iter-100 PR review (a separate, since-fixed bug in
/// `LiveFirefox::with_daemon` meant CI's `live-tests` job never previously
/// ran a test binary late enough alphabetically to reach this one).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_jq_missing_path_strict_exits_nonzero() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_jq_missing_path_strict_exits_nonzero: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(nav.status.success(), "navigate failed");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args([
            "--jq",
            ".results.this_field_does_not_exist",
            "--jq-strict",
            "perf",
            "audit",
        ])
        .output()
        .expect("ff-rdp perf audit --jq --jq-strict");

    // Must exit non-zero.
    assert!(
        !out.status.success(),
        "live_jq_missing_path_strict_exits_nonzero: FAIL — expected non-zero exit but got 0"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("not found"),
        "live_jq_missing_path_strict_exits_nonzero: FAIL — stdout does not contain 'not found': \
         {stdout:?}\nstderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );

    eprintln!(
        "live_jq_missing_path_strict_exits_nonzero: PASS — exit non-zero + 'not found' in stdout"
    );
}

// ---------------------------------------------------------------------------
// Theme E — perf audit --help mentions Lighthouse
// ---------------------------------------------------------------------------

/// `live_perf_audit_help_mentions_lighthouse`:
/// `ff-rdp perf audit --help` must contain the word "Lighthouse".
///
/// Pre-condition: none (no Firefox required — help is static).
/// Post-condition: stdout/stderr contains "Lighthouse".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_perf_audit_help_mentions_lighthouse() {
    if !live_tests_enabled() {
        return;
    }

    // This test doesn't actually need Firefox but follows the live-test gate
    // convention so it doesn't run in default CI.
    let out = Command::new(ff_rdp_bin())
        .args(["perf", "audit", "--help"])
        .output()
        .expect("ff-rdp perf audit --help");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        combined.contains("Lighthouse"),
        "live_perf_audit_help_mentions_lighthouse: FAIL — 'Lighthouse' not found in help output:\n{combined}"
    );

    eprintln!("live_perf_audit_help_mentions_lighthouse: PASS — 'Lighthouse' found in help output");
}
