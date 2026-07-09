//! Live test: `live_eval_in_frame` (iter-78 AC2).
//!
//! Verifies iter-77 Theme B (`EvaluateScope` + `--frame` CLI flag).
//!
//! The test:
//! 1. Navigates to a data URL containing an `<iframe srcdoc="…">` element.
//! 2. Uses the core `RdpTransport` + `WatcherActor::watchTargets("frame")` to
//!    discover the iframe's frame actor ID from `target-available-form` events.
//! 3. Invokes `ff-rdp eval --frame <actor> 'location.href'` and asserts the
//!    returned string contains `"srcdoc"` or `"about:srcdoc"` — Firefox's URL
//!    for srcdoc iframes.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_eval_scope -- --nocapture

use std::process::{Command, Output};
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use ff_rdp_core::{RdpTransport, RootActor, TabActor, WatcherActor, WatcherEvent};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_eval_in_frame`:
/// Navigate to a page with an srcdoc iframe, discover the iframe's frame actor
/// via `watchTargets("frame")`, then invoke `ff-rdp eval --frame <actor>
/// 'location.href'` and assert the result contains `"srcdoc"`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_in_frame() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_in_frame: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_eval_in_frame: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to a data URL containing an srcdoc iframe.
    // --allow-unsafe-urls is a top-level flag, placed before the subcommand.
    let data_url = "data:text/html,<title>OuterFrame</title><iframe srcdoc='<title>InnerFrame</title><p>inner</p>'></iframe>";

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .arg("--allow-unsafe-urls")
        .args(["navigate", data_url])
        .output()
        .expect("navigate to data URL with srcdoc iframe");

    if !nav.status.success() {
        eprintln!(
            "live_eval_in_frame: navigate failed — skipping\n{}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // Give Firefox a moment to settle the iframe target.
    std::thread::sleep(Duration::from_millis(300));

    // Use the core library to discover the iframe's frame actor via
    // watchTargets("frame").  We open a separate transport connection to
    // subscribe to target events without interfering with the CLI invocations.
    let iframe_actor = discover_iframe_actor(ff.port());

    let Some(iframe_actor) = iframe_actor else {
        eprintln!(
            "live_eval_in_frame: could not discover iframe frame actor \
             (watchTargets returned no non-top-level frame targets) — skipping"
        );
        return;
    };

    eprintln!("live_eval_in_frame: iframe actor = {iframe_actor}");

    // Invoke the CLI: eval --frame <actor> 'location.href'
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "--frame", &iframe_actor, "location.href"])
        .output()
        .expect("eval --frame location.href");

    assert!(
        out.status.success(),
        "live_eval_in_frame: eval --frame exited non-zero — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out);

    // The iframe's location.href in Firefox for srcdoc iframes is
    // "about:srcdoc".  The results field should be a JSON string; fall back to
    // the JSON repr when it is something else (e.g. a grip object).
    let href_str = if json["results"].is_string() {
        json["results"].as_str().unwrap_or("").to_owned()
    } else {
        json["results"].to_string()
    };

    assert!(
        href_str.contains("srcdoc"),
        "live_eval_in_frame: expected location.href to contain 'srcdoc' \
         (Firefox srcdoc iframe URL), got: {href_str}\nfull output: {json}"
    );

    eprintln!("live_eval_in_frame: PASS — location.href = {href_str:?}");
}

/// Open a short-lived RDP transport, call `watchTargets("frame")`, drain
/// `target-available-form` events for up to 2 s, and return the actor ID of
/// the first non-top-level frame target.
///
/// Returns `None` if no such target appears within the timeout.
fn discover_iframe_actor(port: u16) -> Option<String> {
    let mut transport = RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).ok()?;

    let tabs = RootActor::list_tabs(&mut transport).ok()?;
    let tab_actor = tabs.first()?.actor.clone();

    let watcher_actor = TabActor::get_watcher(&mut transport, &tab_actor).ok()?;
    WatcherActor::watch_targets(&mut transport, &watcher_actor, "frame").ok()?;

    transport
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok()?;

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let Ok(msg) = transport.recv() else {
            continue;
        };

        if let Some(WatcherEvent::TargetAvailable { target }) = WatcherEvent::from_packet(&msg)
            && !target.is_top_level
        {
            eprintln!(
                "discover_iframe_actor: found non-top-level target actor={} url={:?}",
                target.actor.as_ref(),
                target.url
            );
            return Some(target.actor.as_ref().to_owned());
        }
    }

    None
}
