//! Live tests for iteration 137 — daemon-mode parity.
//!
//! Iteration 129 shipped frame-aware `click`, the cross-origin frame scan and
//! `consent accept`; all three worked only with `--no-daemon`, because through
//! the daemon `enumerate_frame_targets` returned zero targets. Every iter-129
//! live test passed `--no-daemon`, so nothing caught it. **These tests run the
//! same features over the default daemon connection** — the one every real
//! invocation uses.
//!
//! Themes covered:
//! - A: frame-target enumeration through the daemon proxy
//! - B: concurrent proxied commands (and the "timed out after 0ms" non-duration)
//! - C: `network` source selection agreeing across connection modes
//!
//! daemon-parity: every test here IS the daemon-mode test. `--no-daemon`
//! appears only as the reference leg of a same-page comparison
//! (live_137_frame_targets_via_daemon, live_137_network_source_parity), never
//! as the only mode exercised.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_137 -- --nocapture
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_137_consent -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin};

/// A `data:` fixture: a top document (unique origin) embedding a genuinely
/// cross-origin `https://example.com` iframe — the same fixture iteration 129
/// used, so the daemon and direct legs are compared on identical content.
const CROSS_ORIGIN_FIXTURE: &str =
    r#"data:text/html,<h1>top</h1><iframe src="https://example.com"></iframe>"#;

/// Global args for the **default** connection mode: no `--no-daemon`, so the
/// CLI auto-starts and proxies through the daemon.
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
}

/// Global args for a direct connection — the leg iteration 129 tested.
fn direct_args(port: u16) -> Vec<String> {
    let mut args = daemon_args(port);
    args.push("--no-daemon".to_owned());
    args
}

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Stop the daemon bound to `port`, so a lingering daemon process cannot
/// outlive the test and hold the Firefox connection open for the next one.
fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Bring up Firefox with a running daemon, or return `None` with a printed
/// reason (Firefox unavailable / daemon refused to start).
/// Launch Firefox and bring up its proxy daemon.
///
/// Panics on either failure (iter-158 Theme D) — the `Option` this used to
/// return made every caller `return` early, which libtest reports as `ok`.
fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

/// `ff-rdp daemon status` output as raw JSON text, for assertion messages.
fn daemon_status(port: u16) -> String {
    let out = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "status"])
        .output()
        .expect("daemon status");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Poll `daemon status` until it reports at least one **live** target.
///
/// `live_target_count` (iter-137) is the number of targets alive right now, as
/// opposed to the cumulative `target_count`. A daemon that restarted mid-test
/// re-establishes its `watchTargets("frame")` subscription on a background
/// thread; until that lands it has recorded nothing, and probing it would
/// measure the restart rather than the feature under test.
fn wait_for_live_targets(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let text = daemon_status(port);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && json["results"]["live_target_count"].as_u64().unwrap_or(0) >= 1
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

/// Run `click --frame <no-match>` and pull the "N frame(s) available" count
/// out of the resulting error. That error is the only place the CLI prints
/// the raw enumeration result, which makes it the sharpest probe for the
/// daemon-vs-direct comparison this iteration is about.
fn frame_count_via(args: &[String]) -> Option<usize> {
    let out = Command::new(ff_rdp_bin())
        .args(args)
        .args(["click", "body", "--frame", "zzz-no-such-frame", "--no-wait"])
        .output()
        .expect("click --frame probe");
    let text = combined(&out);
    let idx = text.find(" frame(s) available")?;
    let prefix = &text[..idx];
    let count: String = prefix
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    count.chars().rev().collect::<String>().parse().ok()
}

/// AC: `live_137_frame_targets_via_daemon` — frame enumeration returns the
/// same non-zero target count through the daemon and with `--no-daemon` on a
/// multi-frame page. Before this iteration the daemon leg reported 0.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_137_frame_targets_via_daemon() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_137_frame_targets_via_daemon: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = firefox_with_daemon("live_137_frame_targets_via_daemon");
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", CROSS_ORIGIN_FIXTURE, "--allow-unsafe-urls"])
        .output()
        .expect("navigate via daemon");
    if !nav.status.success() {
        eprintln!(
            "live_137_frame_targets_via_daemon: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        stop_daemon(port);
        return;
    }

    // The daemon must have its own frame-target subscription live before the
    // probe means anything; a daemon that restarted mid-test re-establishes it
    // on a background thread.
    assert!(
        wait_for_live_targets(port),
        "daemon never reported live frame targets — status: {}",
        daemon_status(port)
    );

    let via_daemon = frame_count_via(&daemon_args(port));
    let direct = frame_count_via(&direct_args(port));
    stop_daemon(port);

    let via_daemon = via_daemon.expect("daemon leg must report a frame count");
    let direct = direct.expect("direct leg must report a frame count");

    assert!(
        via_daemon >= 2,
        "daemon-mode enumeration must see the top-level target AND the \
         cross-origin frame, got {via_daemon} (this is the iter-129 regression: \
         it used to be 0)"
    );
    assert_eq!(
        via_daemon, direct,
        "frame count must not depend on the connection mode: \
         daemon={via_daemon}, --no-daemon={direct}"
    );

    eprintln!("live_137_frame_targets_via_daemon: PASSED — {via_daemon} frames in both modes");
}

/// AC: `live_137_click_cross_origin_via_daemon` — a click on an element that
/// exists only inside the cross-origin frame succeeds in daemon mode, with
/// `meta.frame_url` naming the frame.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_137_click_cross_origin_via_daemon() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_137_click_cross_origin_via_daemon: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = firefox_with_daemon("live_137_click_cross_origin_via_daemon");
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", CROSS_ORIGIN_FIXTURE, "--allow-unsafe-urls"])
        .output()
        .expect("navigate via daemon");
    if !nav.status.success() {
        eprintln!(
            "live_137_click_cross_origin_via_daemon: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        stop_daemon(port);
        return;
    }

    assert!(
        wait_for_live_targets(port),
        "daemon never reported live frame targets — status: {}",
        daemon_status(port)
    );

    // The only <a> on the page lives inside the example.com iframe.
    let click = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["click", "a"])
        .output()
        .expect("click a via daemon");
    stop_daemon(port);

    assert!(
        click.status.success(),
        "click 'a' must succeed through the daemon — {}",
        combined(&click)
    );
    let json = parse_json(&click);
    assert_eq!(json["results"]["clicked"], true, "click result: {json}");
    assert_eq!(
        json["results"]["tag"], "A",
        "click must land on the anchor inside the cross-origin iframe: {json}"
    );
    assert_eq!(
        json["meta"]["frame_url"], "https://example.com/",
        "meta.frame_url must report the frame the click landed in: {json}"
    );
    eprintln!("live_137_click_cross_origin_via_daemon: PASSED — {json}");
}

/// AC: `live_137_consent_accept_via_daemon` — `consent accept` on a
/// Sourcepoint site returns `{"cmp":"sourcepoint","action":"accepted"}`
/// **without** `--no-daemon`. This is iteration 129's own `dogfood_path`,
/// which reported `{"cmp":null,"action":null}` until this iteration.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_137_consent_accept_via_daemon() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_137_consent_accept_via_daemon: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let ff = firefox_with_daemon("live_137_consent_accept_via_daemon");
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", "https://www.theguardian.com"])
        .output()
        .expect("navigate theguardian.com via daemon");
    if !nav.status.success() {
        eprintln!(
            "live_137_consent_accept_via_daemon: navigate failed (site may be unreachable) — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        stop_daemon(port);
        return;
    }

    assert!(
        wait_for_live_targets(port),
        "daemon never reported live frame targets — status: {}",
        daemon_status(port)
    );

    let consent = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["consent", "accept"])
        .output()
        .expect("consent accept via daemon");
    stop_daemon(port);

    assert!(
        consent.status.success(),
        "consent accept must succeed through the daemon — {}",
        combined(&consent)
    );
    let json = parse_json(&consent);
    assert!(
        json["results"].get("cmp").is_some() && json["results"].get("action").is_some(),
        "both keys must always be present: {json}"
    );
    if json["results"]["cmp"] != "sourcepoint" {
        // theguardian.com's CMP configuration is outside our control; the
        // iter-129 suite skips on the same condition rather than failing on a
        // live-site change.
        eprintln!(
            "live_137_consent_accept_via_daemon: theguardian.com did not present a \
             recognised Sourcepoint frame this run ({json}) — skipping"
        );
        return;
    }
    assert_eq!(
        json["results"]["action"], "accepted",
        "Sourcepoint frame detected through the daemon but not accepted: {json}"
    );

    eprintln!("live_137_consent_accept_via_daemon: PASSED — {json}");
}

/// AC: `live_137_concurrent_commands` — four concurrent proxied commands all
/// succeed (they queue for the daemon's single RPC channel instead of being
/// refused), and no error anywhere claims a `0ms` duration.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_137_concurrent_commands() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_137_concurrent_commands: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = firefox_with_daemon("live_137_concurrent_commands");
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args([
            "navigate",
            "data:text/html,<h1>concurrency</h1><p>four at once</p>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("navigate via daemon");
    if !nav.status.success() {
        eprintln!(
            "live_137_concurrent_commands: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        stop_daemon(port);
        return;
    }

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let args = daemon_args(port);
            std::thread::spawn(move || {
                let out = Command::new(ff_rdp_bin())
                    .args(args)
                    .arg("page-text")
                    .output()
                    .expect("page-text via daemon");
                (i, out.status.success(), combined(&out))
            })
        })
        .collect();

    let results: Vec<(usize, bool, String)> = handles
        .into_iter()
        .map(|h| h.join().expect("concurrent page-text thread"))
        .collect();
    stop_daemon(port);

    let failed: Vec<&(usize, bool, String)> = results.iter().filter(|(_, ok, _)| !ok).collect();
    assert!(
        failed.is_empty(),
        "all 4 concurrent proxied commands must succeed (queue, not refuse); failures: {failed:#?}"
    );

    for (i, _, text) in &results {
        assert!(
            !text.contains("after 0ms"),
            "command {i} reported a fabricated 0ms duration — an error must name the \
             deadline that actually elapsed: {text}"
        );
    }

    eprintln!("live_137_concurrent_commands: PASSED — 4/4 succeeded, no 0ms durations");
}

/// AC: `live_137_network_source_parity` — with the source pinned, `network`
/// reports the same `meta.source` and the same row count through the daemon
/// and with `--no-daemon` on a settled page.
///
/// iter-159 note: the `auto` source this test was written around is gone. There
/// is no implicit substitution left, `network` no longer emits
/// `meta.source_reason`, and the default is `watcher`. The parity property the
/// test exists for — an explicitly pinned source returns the same rows in both
/// connection modes — is unchanged, and the absence of `source_reason` is now
/// asserted so the deletion cannot silently regrow.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_137_network_source_parity() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_137_network_source_parity: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<html><head><link rel=\"stylesheet\" href=\"/a.css\"></head>\
             <body><h1>net parity</h1><script src=\"/b.js\"></script></body></html>",
        ),
    );
    routes.insert(
        "/a.css".to_owned(),
        FixtureRoute {
            content_type: "text/css",
            body: b"body{color:#111}".to_vec(),
            extra_headers: Vec::new(),
        },
    );
    routes.insert(
        "/b.js".to_owned(),
        FixtureRoute {
            content_type: "application/javascript",
            body: b"window.__ready = true;".to_vec(),
            extra_headers: Vec::new(),
        },
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_137_network_source_parity: could not bind fixture server — skipping");
        return;
    };
    let url = format!("{}/", server.base_url());

    let ff = firefox_with_daemon("live_137_network_source_parity");
    let port = ff.port();

    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", &url])
        .output()
        .expect("navigate fixture via daemon");
    if !nav.status.success() {
        eprintln!(
            "live_137_network_source_parity: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        stop_daemon(port);
        return;
    }

    let run_network = |args: Vec<String>| -> serde_json::Value {
        let out = Command::new(ff_rdp_bin())
            .args(args)
            .args(["network", "--source", "performance-api", "--detail"])
            .output()
            .expect("network --source performance-api");
        assert!(
            out.status.success(),
            "network --source performance-api must succeed — {}",
            combined(&out)
        );
        parse_json(&out)
    };

    let via_daemon = run_network(daemon_args(port));
    let direct = run_network(direct_args(port));
    stop_daemon(port);

    assert_eq!(
        via_daemon["meta"]["source"], "performance-api",
        "pinned source must be honoured through the daemon: {via_daemon}"
    );
    assert_eq!(
        direct["meta"]["source"], "performance-api",
        "pinned source must be honoured on a direct connection: {direct}"
    );
    // iter-159 deleted `meta.source_reason` from `network`: it existed only to
    // explain how `auto` resolved, and `auto`'s implicit substitution is gone.
    assert!(
        via_daemon["meta"].get("source_reason").is_none(),
        "network no longer emits source_reason (iter-159): {via_daemon}"
    );
    assert!(
        direct["meta"].get("source_reason").is_none(),
        "network no longer emits source_reason (iter-159): {direct}"
    );
    assert_eq!(
        via_daemon["meta"]["route"], "daemon",
        "the daemon leg must actually be proxied: {via_daemon}"
    );
    assert_eq!(
        direct["meta"]["route"], "direct",
        "the direct leg must actually be direct: {direct}"
    );
    assert_eq!(
        via_daemon["total"], direct["total"],
        "row count must not depend on the connection mode once the source is \
         pinned: daemon={} direct={}",
        via_daemon["total"], direct["total"]
    );
    assert!(
        via_daemon["total"].as_u64().unwrap_or(0) >= 2,
        "the fixture page loads a stylesheet and a script — expected at least 2 \
         performance-api rows, got {}",
        via_daemon["total"]
    );

    eprintln!(
        "live_137_network_source_parity: PASSED — {} rows from performance-api in both modes",
        via_daemon["total"]
    );
}
