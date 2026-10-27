//! `navigate`'s default wait (`--wait auto`) returns a usable document instead
//! of timing out while sub-resources are still loading.
//!
//! `navigate --help` promises the default wait ends when readyState reaches
//! 'interactive' or 'complete'. Until 2026-10-27 the default was a strict
//! `complete`, so comparis.ch/krankenkassen under `--throttle slow-3g` (~73 MB
//! of sub-resources) timed out at any `--timeout`. Here a local page whose one
//! image is held back for [`SLOW_IMAGE`] stands in for that page: its DOM is
//! interactive at once, its load event fires only when the image arrives.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_navigate_wait_auto -- --include-ignored

use std::collections::HashMap;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// How long the fixture holds the page's image back.
const SLOW_IMAGE: Duration = Duration::from_secs(8);

fn run(port: u16, timeout_ms: &str, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            timeout_ms,
        ])
        .args(args)
        .output()
        .expect("ff-rdp command")
}

fn stdout_json(out: &Output, what: &str) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "{what}: output not JSON ({e}): stdout={stdout} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn navigate_ok(port: u16, timeout_ms: &str, args: &[&str]) -> Value {
    let out = run(port, timeout_ms, args);
    let json = stdout_json(&out, &format!("{args:?}"));
    assert!(out.status.success(), "{args:?} failed: {json}");
    json["results"].clone()
}

fn elapsed_ms(results: &Value) -> u64 {
    results["elapsed_ms"].as_u64().unwrap_or(u64::MAX)
}

#[test]
#[ignore = "requires Firefox — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_wait_auto_returns_interactive_before_slow_load_event() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_wait_auto: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let page = |name: &str| {
        FixtureRoute::html(format!(
            "<!doctype html><title>wait-auto-{name}</title><body><p>Ready text</p>\
             <img src=\"/slow.png?{name}\"></body>"
        ))
    };
    let mut routes = HashMap::new();
    for name in ["auto", "interactive", "complete", "text", "strict"] {
        routes.insert(format!("/{name}"), page(name));
    }
    routes.insert(
        "/slow.png".to_owned(),
        FixtureRoute {
            content_type: "image/png",
            body: Vec::new(),
            extra_headers: vec![("Cache-Control".to_owned(), "no-store".to_owned())],
            delay: SLOW_IMAGE,
        },
    );
    let server = FixtureServer::start(routes).expect("fixture server");
    let base = server.base_url();
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let slow_ms = u64::try_from(SLOW_IMAGE.as_millis()).unwrap_or(u64::MAX);

    // Default: `complete` gets a 3 s grace after `interactive`, then the
    // interactive document is returned — long before the image arrives.
    let auto = navigate_ok(port, "30000", &["navigate", &format!("{base}/auto")]);
    assert_eq!(auto["ready_state"], "interactive", "{auto}");
    assert_eq!(auto["status"], 200, "{auto}");
    assert!(
        (3000..slow_ms).contains(&elapsed_ms(&auto)),
        "auto must wait the 3 s grace, then return before the load event: {auto}"
    );

    // `--wait interactive`: no grace.
    let interactive = navigate_ok(
        port,
        "30000",
        &[
            "navigate",
            &format!("{base}/interactive"),
            "--wait",
            "interactive",
        ],
    );
    assert_eq!(interactive["ready_state"], "interactive", "{interactive}");
    assert!(elapsed_ms(&interactive) < 3000, "{interactive}");

    // `--wait complete`: waits for the load event.
    let complete = navigate_ok(
        port,
        "30000",
        &[
            "navigate",
            &format!("{base}/complete"),
            "--wait",
            "complete",
        ],
    );
    assert_eq!(complete["ready_state"], "complete", "{complete}");
    assert!(elapsed_ms(&complete) >= slow_ms, "{complete}");

    // `--wait-text` runs after the default wait and still finds its text.
    let text = navigate_ok(
        port,
        "30000",
        &[
            "navigate",
            &format!("{base}/text"),
            "--wait-text",
            "Ready text",
        ],
    );
    assert_eq!(text["wait"]["waited"], true, "{text}");
    assert!(elapsed_ms(&text) < slow_ms, "{text}");

    // `--wait complete` inside a budget shorter than the load is a timeout,
    // as before; the default within the same budget is not.
    let out = run(
        port,
        "5000",
        &["navigate", &format!("{base}/strict"), "--wait", "complete"],
    );
    let err = stdout_json(&out, "strict");
    assert_eq!(out.status.code(), Some(124), "{err}");
    assert!(
        err["error"]
            .as_str()
            .unwrap_or_default()
            .contains("'complete'"),
        "{err}"
    );
}
