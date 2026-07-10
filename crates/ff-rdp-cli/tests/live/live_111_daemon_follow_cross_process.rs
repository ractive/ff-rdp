//! Live test for iter-111 Theme A — daemon `--follow` stream survives a
//! cross-process (top-level target) navigation.
//!
//! This is the live end-to-end proof for the iter-101 top-level-switch purge
//! path: a long-running follow stream, served through the daemon proxy, must
//! keep delivering events that originate on the **post-nav** page after the
//! browsing context is switched out from under it.  If the iter-101 purge left
//! dead-target state behind, the daemon's watcher subscription would be
//! stranded on the destroyed target and no post-nav event would reach the
//! still-open stream.
//!
//! # Why `network --follow` (not `console --follow`)
//!
//! The daemon's follow streams read the watcher `resources-available-array`
//! stream.  On the tested Firefox versions ordinary `console.log` calls are
//! delivered as a direct console-actor push and are **not** routed through the
//! watcher `console-message` resource stream (iter-71 Theme C research), so a
//! `console --follow` stream is not a dependable post-nav signal.  Network /
//! navigation resources, by contrast, flow through the watcher reliably (the
//! daemon's whole network-buffering feature is built on them).  A
//! `network --follow` stream therefore emits a `navigation` event whose `url`
//! is the post-nav page — an event unambiguously *sourced from the post-nav
//! page* — which is exactly the AC's post-condition.
//!
//! # Why the driving navigation uses `--no-daemon`
//!
//! The follow stream holds the daemon's single RPC-writer slot for its whole
//! lifetime (iter-101 Theme B: at most one RPC client at a time).  A second
//! *daemon-routed* command would be refused with `daemon_busy`, so the page is
//! driven with a direct (`--no-daemon`) navigation.  The daemon's own watcher —
//! subscribed on its persistent Firefox connection — still observes the
//! resulting top-level target switch and forwards the new page's navigation
//! event to the follow stream.  This mirrors the real dogfooding flow (a
//! follow running in the background while the page is driven separately).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_111_daemon_follow_cross_process -- --nocapture
//!
//! The genuine cross-*process* (Fission) phase additionally requires
//! `FF_RDP_LIVE_NETWORK_TESTS=1` (it navigates to real remote sites).

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{LiveFirefox, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled};

/// Spawn a reader thread that pushes every line from `child`'s stdout into a
/// shared buffer.  Returns the shared buffer so the test can poll it.
fn collect_stdout_lines(child: &mut Child) -> Arc<Mutex<Vec<String>>> {
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take().expect("follow child stdout piped");
    let sink = Arc::clone(&lines);
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => sink.lock().expect("stdout sink lock").push(l),
                Err(_) => break,
            }
        }
    });
    lines
}

/// True if any collected NDJSON line references `needle` (checked against the
/// parsed `url` field first, then the raw line as a fallback).
fn saw_needle(lines: &Arc<Mutex<Vec<String>>>, needle: &str) -> bool {
    let guard = lines.lock().expect("stdout lines lock");
    guard.iter().any(|line| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(url) = v.get("url").and_then(serde_json::Value::as_str)
        {
            return url.contains(needle);
        }
        line.contains(needle)
    })
}

/// Stop the daemon for `port` (best-effort).
fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output();
}

/// AC: `live_daemon_follow_survives_cross_process_nav` — after a top-level
/// target switch, at least one event whose source is the post-nav page is
/// delivered on the still-open daemon `--follow` stream.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_daemon_follow_survives_cross_process_nav() {
    if !live_tests_enabled() {
        eprintln!("live_daemon_follow_survives_cross_process_nav: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_daemon_follow_survives_cross_process_nav: Firefox not available — skipping"
        );
        return;
    };
    let port = ff.port();

    // Auto-start the daemon for this Firefox instance.  `with_daemon` triggers
    // startup via an `eval` (which genuinely routes through the daemon) and
    // confirms `daemon status.running == true`.
    if ff.with_daemon().is_none() {
        eprintln!("live_daemon_follow_survives_cross_process_nav: daemon did not start — skipping");
        stop_daemon(port);
        return;
    }

    // Establish an initial page (page A) via a direct navigation so the daemon
    // watcher has a live target before the follow stream starts.
    let nav_a = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "--timeout",
            "10000",
            "navigate",
            "data:text/html,<h1>page-a</h1>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("navigate page A");
    if !nav_a.status.success() {
        eprintln!(
            "live_daemon_follow_survives_cross_process_nav: nav A failed — {}",
            String::from_utf8_lossy(&nav_a.stderr)
        );
        stop_daemon(port);
        return;
    }

    // Start the long-running network follow stream through the daemon (NO
    // --no-daemon: this is the daemon-proxied stream under test).
    let mut follow = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "60000",
            "network",
            "--follow",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn network --follow");
    let follow_lines = collect_stdout_lines(&mut follow);

    // Give the follow stream a moment to subscribe before navigating.
    std::thread::sleep(Duration::from_secs(1));

    // Navigate cross-origin to page B, whose URL embeds a unique sentinel.  The
    // resulting `navigation` event on the follow stream carries that URL, so
    // finding the sentinel proves a post-nav-sourced event reached the still-
    // open stream after the top-level target switch.  Driven with --no-daemon
    // so it doesn't contend for the daemon RPC slot the follow stream holds.
    let sentinel = format!("iter111postnav{}", std::process::id());
    let page_b = format!(
        "data:text/html;charset=utf-8,\
         <!DOCTYPE html><html><head><title>{sentinel}</title></head>\
         <body><h1>{sentinel}</h1></body></html>"
    );
    let nav_b = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "--timeout",
            "15000",
            "navigate",
            &page_b,
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("navigate page B");
    if !nav_b.status.success() {
        eprintln!(
            "live_daemon_follow_survives_cross_process_nav: nav B failed — {}",
            String::from_utf8_lossy(&nav_b.stderr)
        );
        let _ = follow.kill();
        let _ = follow.wait();
        stop_daemon(port);
        return;
    }

    // Poll the follow stream for the post-nav sentinel.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got_sentinel = false;
    while Instant::now() < deadline {
        if saw_needle(&follow_lines, &sentinel) {
            got_sentinel = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Optional cross-*process* phase: a genuine Fission process switch
    // (example.com → wikipedia.org, distinct eTLD+1).  We assert the follow
    // stream both stays alive AND delivers a post-nav-sourced event (the
    // wikipedia navigation) across the real process switch.
    let mut cross_process_ok = true;
    if live_network_tests_enabled() {
        let direct = |url: &str| {
            Command::new(ff_rdp_bin())
                .args([
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port.to_string(),
                    "--no-daemon",
                    "--timeout",
                    "30000",
                    "navigate",
                    url,
                ])
                .output()
                .expect("navigate remote site")
        };
        let nav_example = direct("https://example.com/");
        let nav_wiki = direct("https://en.wikipedia.org/wiki/Firefox");

        // Wait for a wikipedia-sourced navigation event on the still-open stream.
        let cp_deadline = Instant::now() + Duration::from_secs(15);
        let mut saw_wiki = false;
        while Instant::now() < cp_deadline {
            if saw_needle(&follow_lines, "wikipedia.org") {
                saw_wiki = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        let follow_alive = matches!(follow.try_wait(), Ok(None));
        cross_process_ok =
            nav_example.status.success() && nav_wiki.status.success() && follow_alive && saw_wiki;
        eprintln!(
            "live_daemon_follow_survives_cross_process_nav: cross-process phase \
             example_ok={} wiki_ok={} follow_alive={} saw_wiki_event={}",
            nav_example.status.success(),
            nav_wiki.status.success(),
            follow_alive,
            saw_wiki,
        );
    } else {
        eprintln!(
            "live_daemon_follow_survives_cross_process_nav: skipping cross-process \
             (network) phase — set FF_RDP_LIVE_NETWORK_TESTS=1 to enable"
        );
    }

    // Tear down the follow child and daemon before asserting so cleanup always
    // runs, even on a panic below.
    let _ = follow.kill();
    let follow_out = follow.wait_with_output();
    stop_daemon(port);

    let collected = follow_lines.lock().expect("stdout lines lock").clone();
    assert!(
        got_sentinel,
        "post-navigation event (sentinel {sentinel:?} in a follow-stream url) was not delivered \
         on the still-open daemon --follow stream after the top-level target switch; the iter-101 \
         purge path may have stranded the watcher subscription.\ncollected {} line(s): {:#?}\n\
         follow stderr: {}",
        collected.len(),
        collected,
        follow_out
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
            .unwrap_or_default(),
    );

    assert!(
        cross_process_ok,
        "cross-process (example.com → wikipedia) phase failed: the daemon --follow stream did not \
         survive the Fission process switch or did not deliver a wikipedia-sourced event.\n\
         collected {} line(s): {:#?}",
        collected.len(),
        collected,
    );

    eprintln!(
        "live_daemon_follow_survives_cross_process_nav: PASS — post-nav sentinel delivered on the \
         daemon follow stream ({} line(s) collected)",
        collected.len()
    );
}
