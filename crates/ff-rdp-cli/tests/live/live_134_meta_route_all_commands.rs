//! Live tests for iteration 134 — `meta.route` on every command (carry-over
//! from [[iteration-128-network-hint-always-present]] Theme D).
//!
//! iter-128 wired `connection_meta::merge_route` at the two call sites
//! central to that iteration (`network`, `navigate --with-network`). This
//! iteration rolled the same call out to the remaining ~30 browser-touching
//! commands' meta-building call sites. This test proves the rollout landed
//! on a representative sample — `click`, `eval`, `screenshot`, `dom` — the
//! same four commands the iteration plan's AC names.
//!
//! No external network access is needed (everything runs against Firefox's
//! default `about:blank`, which already has an empty `<body>` to target), so
//! this is gated on `FF_RDP_LIVE_TESTS=1` rather than the network tier —
//! matching `live_128_meta_route`'s gating rationale.
//!
//! daemon-parity: `live_134_meta_route_all_commands` itself runs each
//! command in BOTH modes (`--no-daemon` first, then the daemon-routed
//! default) within the same test — that comparison is the entire point of
//! `meta.route`, so there is no separate suite to point at.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_134_meta_route_all_commands -- --nocapture

use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// Bare `--host`/`--port`/`--timeout` args with NO daemon-mode opinion —
/// deliberately not `common::base_args`, which hardcodes `--no-daemon`. This
/// test needs to control that flag itself to compare both routes.
fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn parse_json(output: &Output) -> Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Run `args` against `port` with the given extra flags (e.g. `--no-daemon`)
/// prepended before the subcommand, and return the parsed JSON envelope.
/// Panics with full stdout/stderr on a non-zero exit so a failure is
/// diagnosable without re-running by hand.
fn run_json(port: u16, extra_flags: &[&str], args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(extra_flags)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "command {args:?} (flags {extra_flags:?}) failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    parse_json(&out)
}

/// `live_134_meta_route_all_commands`: for `click`, `eval`, `screenshot`,
/// and `dom`, `meta.route` is present and correct (`"daemon"` by default,
/// `"direct"` under `--no-daemon`) — without `--verbose`.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_134_meta_route_all_commands() {
    if !live_tests_enabled() {
        eprintln!("live_134_meta_route_all_commands: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    // Each (command, args, default_mode_route) triple targets `body`, which
    // `about:blank` always has — no navigate/fixture-server setup needed,
    // keeping this test network-free per the module doc. `default_mode_route`
    // is what the command reports WITHOUT `--no-daemon`: "daemon" for the
    // three that use `connect_and_get_target` (click/eval/dom), but
    // `screenshot` always uses `connect_direct` regardless of daemon mode
    // (its `run_core` doc comment explains why: the daemon's watcher
    // subscription breaks the two-step capture protocol) — its route is
    // "direct" in BOTH modes, which is itself the behaviour under test:
    // `meta.route` reflects the connection Firefox actually saw, not the
    // flag the caller passed.
    let commands: &[(&str, &[&str], &str)] = &[
        ("click", &["click", "body", "--no-wait"], "daemon"),
        ("eval", &["eval", "1+1"], "daemon"),
        ("screenshot", &["screenshot", "--base64"], "direct"),
        ("dom", &["dom", "body", "--count"], "daemon"),
    ];

    // Direct FIRST: --no-daemon bypasses the daemon proxy entirely, and
    // doing this before any daemon exists sidesteps `daemon stop`'s
    // process-group reap (iter-95 Theme A) taking the directly launched
    // Firefox process down with it — same ordering rationale as
    // `live_128_meta_route`.
    for (name, args, _) in commands {
        let json = run_json(port, &["--no-daemon"], args);
        assert_eq!(
            json["meta"]["route"], "direct",
            "`{name} --no-daemon` must report meta.route == \"direct\" without --verbose, got: {json}"
        );
    }

    // Daemon-routed SECOND: no --no-daemon, so connect_and_get_target
    // resolves (and auto-starts) the daemon proxy against the same Firefox
    // for the commands that actually route through it.
    for (name, args, expected) in commands {
        let json = run_json(port, &[], args);
        assert_eq!(
            json["meta"]["route"], *expected,
            "`{name}` (default mode) must report meta.route == \"{expected}\" without --verbose, got: {json}"
        );
    }
    stop_daemon(port);

    eprintln!(
        "live_134_meta_route_all_commands: PASSED — click/eval/screenshot/dom all report \
         meta.route (\"direct\" under --no-daemon, \"daemon\" by default)"
    );
}
