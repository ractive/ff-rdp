/// Live test: `wait --timeout-ms` sets the condition timeout and a
/// satisfied condition exits 0. (The legacy `--wait-timeout` alias was
/// removed in the 2026-10 reset.)
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use std::process::Command;

const FIXTURE_HTML: &str =
    "data:text/html;charset=utf-8,<!DOCTYPE html><html><body><p>ready</p></body></html>";

/// Post-condition: `wait --selector body --timeout-ms 2000` exits 0.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_wait_timeout_ms_canonical_flag() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_wait_timeout_ms_canonical_flag: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        // data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        crate::common::output_note(&nav)
    );

    let out_new = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["wait", "--selector", "body", "--timeout-ms", "2000"])
        .output()
        .expect("wait --timeout-ms failed");

    assert!(
        out_new.status.success(),
        "wait --timeout-ms failed: {}",
        crate::common::output_note(&out_new)
    );

    eprintln!("live_wait_timeout_ms_canonical_flag: PASS");
}
