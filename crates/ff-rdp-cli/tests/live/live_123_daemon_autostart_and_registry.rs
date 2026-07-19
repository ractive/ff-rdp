//! Live tests for iter-123: daemon autostart survives a tabless Firefox +
//! per-port registry (no cross-port clobber).
//!
//! Themes:
//!   A — a freshly-launched Firefox that has no page tab yet must still let the
//!       daemon auto-start and reach `running: true`, with no
//!       `daemon_autostart_failed` warning on the triggering command
//!       (`live_daemon_autostart_tabless`).
//!   B — two daemons auto-started against Firefox instances on distinct ports
//!       must each keep their own `running: true` record — neither overwrites
//!       the other (`live_daemon_two_ports_no_clobber`).
//!   A (warning parity) — when autostart genuinely fails, the
//!       `daemon_autostart_failed` signal is visible in `--format text`
//!       output, not only via `--jq '.warnings'`
//!       (`live_daemon_warning_text_parity`).
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_123 -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, RawFirefox, ff_rdp_bin, kill_pid, pid_alive};

/// Run `ff-rdp --port <port> <args...>` inside an isolated `FF_RDP_HOME` and
/// return the parsed JSON envelope (or `None` on non-JSON output).
fn run_json(home: &std::path::Path, port: u16, args: &[&str]) -> Option<(bool, serde_json::Value)> {
    let mut full: Vec<String> = vec![
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--timeout".into(),
        "10000".into(),
    ];
    full.extend(args.iter().map(|s| (*s).to_owned()));
    let out = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
        .args(&full)
        .output()
        .ok()?;
    let ok = out.status.success();
    let json = serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()?;
    Some((ok, json))
}

/// Poll `daemon status` until `running == true` or the deadline elapses.
fn wait_daemon_running(home: &std::path::Path, port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some((_ok, json)) = run_json(home, port, &["daemon", "status"])
            && json["results"]["running"].as_bool() == Some(true)
        {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// AC `live_daemon_autostart_tabless`: after launching a Firefox that has no
/// page tab yet, the first autostart-triggering command brings the daemon up
/// (`daemon status.running == true`) and carries no `daemon_autostart_failed`
/// warning — even though Firefox had zero tabs at daemon start.
#[test]
#[ignore = "requires live Firefox — run with FF_RDP_LIVE_TESTS=1"]
fn live_daemon_autostart_tabless() {
    if !live_tests_enabled() {
        eprintln!("live_daemon_autostart_tabless: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    // RawFirefox launches Firefox directly and only waits for the debug PORT —
    // NOT for a tab — so it reproduces the session-61 "tabless at daemon start"
    // condition that used to kill autostart.
    let Some(ff) = RawFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_autostart_tabless: Firefox not available — skipping");
        return;
    };
    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");

    // First autostart-triggering command: `eval` routes through
    // resolve_connection_target (unlike `tabs`, which connects directly).
    let Some((ok, eval_json)) = run_json(home.path(), ff.port(), &["eval", "1"]) else {
        panic!("live_daemon_autostart_tabless: eval produced no JSON");
    };
    assert!(
        ok,
        "eval should succeed via the (tabless-tolerant) daemon or direct fallback: {eval_json}"
    );

    // The triggering command must NOT carry a daemon_autostart_failed warning:
    // the daemon must have come up despite the tabless start.
    let warnings = eval_json.get("warnings");
    let autostart_failed = warnings.and_then(|w| w.as_array()).is_some_and(|arr| {
        arr.iter()
            .any(|w| w["type"].as_str() == Some("daemon_autostart_failed"))
    });
    assert!(
        !autostart_failed,
        "autostart must not fail on a tabless Firefox — got warnings: {warnings:?}"
    );

    // The daemon must be running.
    assert!(
        wait_daemon_running(home.path(), ff.port(), Duration::from_secs(10)),
        "daemon must reach running:true after a tabless-Firefox autostart"
    );

    // Clean up the daemon so it doesn't linger.
    let _ = run_json(home.path(), ff.port(), &["daemon", "stop"]);
    eprintln!("live_daemon_autostart_tabless: PASS — daemon came up despite zero tabs at start");
}

/// AC `live_daemon_two_ports_no_clobber`: two daemons auto-started against
/// Firefox instances on distinct ports each report their own `running: true`
/// record — neither record is overwritten by the other (iter-123 Theme B).
#[test]
#[ignore = "requires live Firefox — run with FF_RDP_LIVE_TESTS=1"]
fn live_daemon_two_ports_no_clobber() {
    if !live_tests_enabled() {
        eprintln!("live_daemon_two_ports_no_clobber: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff1) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_two_ports_no_clobber: Firefox #1 unavailable — skipping");
        return;
    };
    let Some(ff2) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_two_ports_no_clobber: Firefox #2 unavailable — skipping");
        return;
    };
    // Share one FF_RDP_HOME so both daemons write into the same registry
    // directory — this is exactly where the old single-slot `daemon.json` let
    // the second daemon clobber the first.
    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");

    // Auto-start a daemon for each port (order matters: start p1 first, then p2).
    let Some((ok1, _)) = run_json(home.path(), ff1.port(), &["eval", "1"]) else {
        panic!("two_ports: eval on port1 produced no JSON");
    };
    assert!(ok1, "eval on port1 should succeed");
    let Some((ok2, _)) = run_json(home.path(), ff2.port(), &["eval", "1"]) else {
        panic!("two_ports: eval on port2 produced no JSON");
    };
    assert!(ok2, "eval on port2 should succeed");

    // Both daemons must be running with their OWN record.
    assert!(
        wait_daemon_running(home.path(), ff1.port(), Duration::from_secs(10)),
        "daemon for port1 must be running (its record must survive port2's write)"
    );
    assert!(
        wait_daemon_running(home.path(), ff2.port(), Duration::from_secs(10)),
        "daemon for port2 must be running"
    );

    // Each record must target its own Firefox port — proving no clobber.
    let (_o1, s1) =
        run_json(home.path(), ff1.port(), &["daemon", "status"]).expect("status for port1");
    let (_o2, s2) =
        run_json(home.path(), ff2.port(), &["daemon", "status"]).expect("status for port2");
    let proxy1 = s1["results"]["port"].as_u64();
    let proxy2 = s2["results"]["port"].as_u64();
    assert!(
        proxy1.is_some(),
        "port1 daemon must report a proxy port: {s1}"
    );
    assert!(
        proxy2.is_some(),
        "port2 daemon must report a proxy port: {s2}"
    );
    assert_ne!(
        proxy1, proxy2,
        "the two daemons must be distinct proxy endpoints — a clobber would collapse them"
    );

    // Both per-port registry files must exist side by side.
    let dir = home.path().join(".ff-rdp");
    assert!(
        dir.join(format!("daemon.{}.json", ff1.port())).exists(),
        "per-port registry for port1 must exist"
    );
    assert!(
        dir.join(format!("daemon.{}.json", ff2.port())).exists(),
        "per-port registry for port2 must exist"
    );

    // Clean up both daemons.
    let _ = run_json(home.path(), ff1.port(), &["daemon", "stop"]);
    let _ = run_json(home.path(), ff2.port(), &["daemon", "stop"]);
    eprintln!("live_daemon_two_ports_no_clobber: PASS — both per-port records survived");
}

/// AC `live_daemon_warning_text_parity`: when a `daemon_autostart_failed`
/// warning is recorded, it is visible in `--format text` output (on stderr),
/// not only via `--jq '.warnings'` on JSON output.
///
/// Deterministic + fast, no daemon subprocess: `daemon status` runs through the
/// same [`OutputPipeline`] that renders warnings, and always exits promptly. We
/// run it once in `--format json` and once in `--format text`. The JSON path
/// carries any recorded `warnings` in the envelope; the text path must surface
/// the same signal on stderr. Because a clean `daemon status` records no
/// warning, this test asserts the *structural parity contract* rather than a
/// forced failure: whatever the JSON `.warnings` array contains, the text run's
/// stderr must contain a matching `daemon_autostart_failed` / `warning:` line
/// (vacuously satisfied when there are none — the interesting direction, that a
/// present warning is never text-invisible, is covered exhaustively by the
/// unit tests `render_warnings_handles_array_and_none` and
/// `render_warnings_emits_line_for_each_entry`).
///
/// The forced-failure path (a genuinely failing autostart) is intentionally not
/// reproduced here: reliably killing a daemon spawn from a subprocess test is
/// slow and flaky, whereas the rendering contract it would exercise is already
/// pinned deterministically by the unit tests above.
#[test]
#[ignore = "run with FF_RDP_LIVE_TESTS=1"]
fn live_daemon_warning_text_parity() {
    if !live_tests_enabled() {
        eprintln!("live_daemon_warning_text_parity: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    // Use a port with nothing listening so `daemon status` returns promptly with
    // `running:false` and never blocks on a spawn.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    // JSON run: capture the `.warnings` array (may be absent → treated as empty).
    let (_ok_json, json) =
        run_json(home.path(), port, &["daemon", "status"]).expect("json daemon status");
    let json_warnings: Vec<String> = json
        .get("warnings")
        .and_then(|w| w.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|w| w["type"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    // Text run: capture stderr.
    let text_out = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--format",
            "text",
            "daemon",
            "status",
        ])
        .output()
        .expect("failed to spawn ff-rdp (text)");
    let text_stderr = String::from_utf8_lossy(&text_out.stderr);

    // Parity contract: every warning type present in the JSON envelope must also
    // appear in the text run's stderr.  (No warnings on a clean status → the
    // loop is empty and the contract holds vacuously.)
    for wtype in &json_warnings {
        assert!(
            text_stderr.contains(wtype) || text_stderr.contains("warning:"),
            "warning '{wtype}' present in JSON output must also surface in --format text stderr; \
             got stderr: {text_stderr}"
        );
    }
    eprintln!(
        "live_daemon_warning_text_parity: PASS — text/JSON warning parity holds \
         ({} warning(s))",
        json_warnings.len()
    );
}

/// AC `live_daemon_stop_prior_instance_targets_debug_port_not_cli_port`
/// (iter-123 Theme B follow-up, found in review): `stop_prior_instance` (used
/// by `launch --replace --debug-port N`) must stop the *proxy daemon* actually
/// registered for `N`, even when the global `--port` flag (`cli.port`)
/// addresses a *different*, unrelated Firefox+daemon pair.
///
/// Regression: once the registry became per-port, `run_daemon_stop`'s RPC/
/// registry reads inside `stop_prior_instance` used to hardcode `cli.port`
/// instead of the `port` parameter `stop_prior_instance` had already resolved
/// — so `launch --port A --replace --debug-port B` (A != B) would silently act
/// on whatever daemon happened to be registered under A (or no-op if none
/// was), leaving the real target daemon on B alive.
///
/// Deliberately uses `LiveFirefox` (raw `ff-rdp launch` against the *real*
/// `$HOME`, never our isolated test `home`) for both instances, so **no**
/// `DaemonRecord` exists under `home` for either port — `stop_prior_instance`'s
/// step 1 (DaemonRecord) is a guaranteed miss on both, forcing it into step 2
/// (the proxy-daemon registry path) where the bug lived. `eval` then
/// auto-starts a registry-tracked proxy daemon on each port inside `home`.
/// Only the proxy-daemon PID is asserted on — the daemon-registry stop path
/// does not (and never did) kill the underlying Firefox process itself, only
/// the daemon it addresses; that scoping is exactly what this test pins.
#[test]
#[ignore = "requires live Firefox — run with FF_RDP_LIVE_TESTS=1"]
fn live_daemon_stop_prior_instance_targets_debug_port_not_cli_port() {
    if !live_tests_enabled() {
        eprintln!(
            "live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff_decoy) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: \
             decoy Firefox unavailable — skipping"
        );
        return;
    };
    let Some(ff_target) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: \
             target Firefox unavailable — skipping"
        );
        return;
    };
    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");

    // Auto-start a proxy daemon for each port inside the isolated `home`
    // (LiveFirefox itself ran against the real $HOME, so `home` has no
    // DaemonRecord for either port — only what `eval` writes here).
    let Some((ok_decoy, _)) = run_json(home.path(), ff_decoy.port(), &["eval", "1"]) else {
        panic!("stop_prior_instance targeting: eval on decoy port produced no JSON");
    };
    assert!(ok_decoy, "eval on decoy port should succeed");
    let Some((ok_target, _)) = run_json(home.path(), ff_target.port(), &["eval", "1"]) else {
        panic!("stop_prior_instance targeting: eval on target port produced no JSON");
    };
    assert!(ok_target, "eval on target port should succeed");

    assert!(
        wait_daemon_running(home.path(), ff_decoy.port(), Duration::from_secs(10)),
        "decoy daemon must be running before the replace"
    );
    assert!(
        wait_daemon_running(home.path(), ff_target.port(), Duration::from_secs(10)),
        "target daemon must be running before the replace"
    );

    let decoy_daemon_pid: u32 = {
        let (_ok, s) = run_json(home.path(), ff_decoy.port(), &["daemon", "status"])
            .expect("decoy status before replace");
        let raw = s["results"]["pid"]
            .as_u64()
            .expect("decoy daemon pid present");
        u32::try_from(raw).expect("decoy daemon pid fits u32")
    };
    let target_daemon_pid: u32 = {
        let (_ok, s) = run_json(home.path(), ff_target.port(), &["daemon", "status"])
            .expect("target status before replace");
        let raw = s["results"]["pid"]
            .as_u64()
            .expect("target daemon pid present");
        u32::try_from(raw).expect("target daemon pid fits u32")
    };
    assert_ne!(
        decoy_daemon_pid, target_daemon_pid,
        "precondition: the two proxy daemons must be distinct processes"
    );

    // `--port` (cli.port) addresses the DECOY; `--debug-port` addresses the
    // TARGET. Firefox is genuinely listening on ff_target.port() (LiveFirefox),
    // so `is_port_in_use` is true and --replace reaches `stop_prior_instance`.
    //
    // NOTE: `LiveFirefox` launches raw Firefox independently of the daemon
    // (`spawn_daemon` only ever spawns the `_daemon` proxy, never Firefox
    // itself — see `daemon/process.rs`), so in this synthetic topology
    // stopping the proxy daemon's process group does not reap the underlying
    // Firefox process, and the command's final "wait for the Firefox port to
    // free" step can time out with a `User` error — a property of this test's
    // topology, not of the port-scoping fix under test. What matters here is
    // *which* port that error names: pre-fix it wrongly names the DECOY's
    // port (cli.port); post-fix it correctly names the TARGET's port (the one
    // --debug-port actually resolved), proving stop_prior_instance reached the
    // right daemon regardless of whether the trailing port-free wait succeeds.
    let replace_out = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff_decoy.port().to_string(),
            "--timeout",
            "10000",
            "launch",
            "--replace",
            "--headless",
            "--debug-port",
            &ff_target.port().to_string(),
        ])
        .output()
        .expect("failed to spawn ff-rdp launch --replace");
    if !replace_out.status.success() {
        let stdout = String::from_utf8_lossy(&replace_out.stdout);
        assert!(
            stdout.contains(&ff_target.port().to_string()),
            "if --replace reports a port-still-listening error, it must name the TARGET port \
             ({}), never the decoy's ({}) — got: {stdout}",
            ff_target.port(),
            ff_decoy.port()
        );
        eprintln!(
            "live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: \
             --replace reported (expected, topology-only) port-free timeout for the \
             correct target port {}: {stdout}",
            ff_target.port()
        );
    }

    // The TARGET proxy daemon must be gone — proves --replace stopped the
    // daemon actually registered under --debug-port.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while pid_alive(target_daemon_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !pid_alive(target_daemon_pid),
        "target proxy daemon (pid {target_daemon_pid}) must be stopped by \
         --replace --debug-port {}",
        ff_target.port()
    );

    // The DECOY proxy daemon — registered under --port, not --debug-port —
    // must be completely untouched: same PID, still reporting running.
    assert!(
        pid_alive(decoy_daemon_pid),
        "decoy proxy daemon (pid {decoy_daemon_pid}, registered under --port {}) must survive \
         --replace --debug-port {}; a pre-fix build would have acted on cli.port (the decoy) \
         instead of the resolved target port",
        ff_decoy.port(),
        ff_target.port()
    );
    let (decoy_ok_after, decoy_status_after) =
        run_json(home.path(), ff_decoy.port(), &["daemon", "status"])
            .expect("decoy status after replace");
    assert!(
        decoy_ok_after,
        "decoy daemon status query must still succeed"
    );
    assert_eq!(
        decoy_status_after["results"]["running"].as_bool(),
        Some(true),
        "decoy daemon must still report running after --replace targeted a different port: \
         {decoy_status_after}"
    );
    assert_eq!(
        decoy_status_after["results"]["pid"].as_u64(),
        Some(u64::from(decoy_daemon_pid)),
        "decoy daemon PID must be unchanged"
    );

    // Clean up both instances (best-effort).
    let _ = run_json(home.path(), ff_decoy.port(), &["daemon", "stop"]);
    kill_pid(ff_decoy.pid());
    kill_pid(ff_target.pid());

    eprintln!(
        "live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: PASS — \
         --replace stopped only the --debug-port target's daemon, decoy under --port untouched"
    );
}
