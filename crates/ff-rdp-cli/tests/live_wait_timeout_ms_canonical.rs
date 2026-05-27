/// Live test for Theme K (iter-84): `--timeout-ms` is the canonical flag for
/// the `wait` subcommand and `--wait-timeout` is kept as a hidden alias.
///
/// AC: live_wait_timeout_ms_canonical — wait --timeout-ms 2000 exits 0;
///     wait --wait-timeout 2000 (legacy alias) exits 0
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

fn live_network_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1")
}

/// Theme K: `wait --timeout-ms` is the canonical flag; `--wait-timeout` is
/// accepted as a hidden alias for backwards compatibility. The global
/// `--timeout` flag still sets the connection timeout as before.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`,
///               tab already at a stable page (no pending navigation).
/// Post-condition: exit 0 for both `--timeout-ms` and `--wait-timeout` flags.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_wait_timeout_ms_canonical_flag() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.com/"])
        .output()
        .expect("navigate failed");
    assert!(nav.status.success());

    // Canonical flag should work without any deprecation warning.
    let out_new = Command::new(ff_rdp_bin())
        .args(["wait", "--timeout-ms", "2000"])
        .output()
        .expect("wait --timeout-ms failed");

    assert!(
        out_new.status.success(),
        "wait --timeout-ms failed: {}",
        String::from_utf8_lossy(&out_new.stderr)
    );

    // The old spelling --wait-timeout must also work (hidden alias).
    let out_old = Command::new(ff_rdp_bin())
        .args(["wait", "--selector", "body", "--wait-timeout", "2000"])
        .output()
        .expect("wait --wait-timeout failed");

    assert!(
        out_old.status.success(),
        "wait --wait-timeout (legacy alias) failed: {}",
        String::from_utf8_lossy(&out_old.stderr)
    );
}
