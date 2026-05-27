/// Live test for Theme C (iter-84): `navigate` with default `--wait both`
/// strategy completes without "no remaining budget" timeout errors.
///
/// Root cause: the events phase consumed the full timeout budget leaving 0ms
/// for the readystate fallback. Fixed by splitting the budget 70/30.
///
/// AC: live_navigate_default_fast — completes in ≤ timeout_ms with status:ok
use std::process::Command;
use std::time::Instant;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

fn live_network_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1")
}

/// Theme C: navigate with default `--wait both` strategy does not exhaust
/// its budget before the readystate fallback fires.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: exit 0 within 10 s; no "no remaining budget" in stderr.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_navigate_default_fast_no_budget_exhaustion() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let start = Instant::now();
    let out = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.com/", "--timeout-ms", "8000"])
        .output()
        .expect("ff-rdp navigate failed");

    let elapsed = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "navigate failed: {stderr}");
    assert!(
        !stderr.contains("no remaining budget"),
        "Theme C regression: 'no remaining budget' appeared in stderr: {stderr}"
    );
    assert!(
        elapsed < 10_000,
        "navigate took too long: {elapsed}ms (expected < 10000ms)"
    );
}

/// Theme K: `--timeout` flag (deprecated) is accepted as alias for
/// `--timeout-ms` and emits a deprecation warning.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_navigate_legacy_timeout_flag_accepted() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let out = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.com/", "--timeout", "5000"])
        .output()
        .expect("ff-rdp navigate failed");

    // Should succeed (deprecated flag is still accepted).
    assert!(
        out.status.success(),
        "navigate with --timeout (deprecated) failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
