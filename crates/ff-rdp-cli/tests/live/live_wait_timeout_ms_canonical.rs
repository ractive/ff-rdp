/// Live test for Theme K (iter-84): `--timeout-ms` is the canonical flag for
/// the `wait` subcommand and `--wait-timeout` is kept as a hidden alias.
///
/// iter-114 Theme B: ported to the self-launch harness using a local `data:`
/// URL fixture. Both `wait` invocations now pass `--selector body` — the
/// `condition` arg group (`--selector`/`--text`/`--eval`/`--ref`) is
/// `required(true)` in clap (see `WaitArgs` in `cli/args.rs`), so the
/// original port-6000 test's first call (`--timeout-ms 2000` with no
/// condition flag) would already have failed argument parsing with exit
/// code 2, not 0 — a pre-existing bug in the legacy test that predates this
/// port. Adding `--selector body` preserves the original intent (exercise
/// `--timeout-ms` and expect exit 0) while actually being valid.
///
/// AC: live_wait_timeout_ms_canonical — wait --timeout-ms 2000 exits 0;
///     wait --wait-timeout 2000 (legacy alias) exits 0
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use std::process::Command;

const FIXTURE_HTML: &str =
    "data:text/html;charset=utf-8,<!DOCTYPE html><html><body><p>ready</p></body></html>";

/// Theme K: `wait --timeout-ms` is the canonical flag; `--wait-timeout` is
/// accepted as a hidden alias for backwards compatibility. The global
/// `--timeout` flag still sets the connection timeout as before.
///
/// Post-condition: exit 0 for both `--timeout-ms` and `--wait-timeout` flags.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_wait_timeout_ms_canonical_flag() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_wait_timeout_ms_canonical_flag: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_wait_timeout_ms_canonical_flag: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        // data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Canonical flag should work without any deprecation warning.
    let out_new = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["wait", "--selector", "body", "--timeout-ms", "2000"])
        .output()
        .expect("wait --timeout-ms failed");

    assert!(
        out_new.status.success(),
        "wait --timeout-ms failed: {}",
        String::from_utf8_lossy(&out_new.stderr)
    );

    // The old spelling --wait-timeout must also work (hidden alias).
    let out_old = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["wait", "--selector", "body", "--wait-timeout", "2000"])
        .output()
        .expect("wait --wait-timeout failed");

    assert!(
        out_old.status.success(),
        "wait --wait-timeout (legacy alias) failed: {}",
        String::from_utf8_lossy(&out_old.stderr)
    );

    eprintln!("live_wait_timeout_ms_canonical_flag: PASS");
}
