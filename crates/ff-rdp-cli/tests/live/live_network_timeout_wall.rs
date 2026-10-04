//! Live test: a one-shot `network` honours `--timeout` as a hard wall on a
//! page that never goes quiet (dogfooding session 64 #6).
//!
//! The capture used to drain until one socket read timed out — until the page
//! stayed silent for a whole `--timeout`. comparis.ch's beacon traffic never
//! does, so `network --timeout 3000` printed nothing for 30 s+. This fixture
//! reproduces that with a local page that `fetch`es every 200 ms.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
//!       live_network_timeout -- --include-ignored --nocapture

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
};

/// A page whose network traffic never stops: one `fetch` every 200 ms.
const BEACON_PAGE: &str = r"<!doctype html><title>beacon</title><body>beacon
<script>
setInterval(function () { fetch('/beacon?t=' + Date.now()).catch(function () {}); }, 200);
</script>
</body>";

/// The `--timeout` under test, and the most the command may take beyond it
/// (process start, connect, envelope).
const TIMEOUT_MS: u64 = 3000;
const SLACK: Duration = Duration::from_millis(1000);

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_network_timeout_is_a_hard_wall_on_a_busy_page() {
    if !live_tests_enabled() {
        eprintln!("live_network_timeout_is_a_hard_wall_on_a_busy_page: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(BEACON_PAGE));
    routes.insert(
        "/beacon".to_owned(),
        FixtureRoute {
            content_type: "text/plain",
            body: b"ok".to_vec(),
            ..FixtureRoute::default()
        },
    );
    let server = FixtureServer::start(routes).expect("bind fixture HTTP server");

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["--timeout", "20000", "navigate", &server.base_url()])
        .output()
        .expect("spawn navigate");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    for args in [
        vec!["network"],
        vec!["network", "--detail", "--headers", "--security", "--all"],
    ] {
        let started = Instant::now();
        let out = Command::new(ff_rdp_bin())
            .args(base_args(port))
            .args(["--timeout", &TIMEOUT_MS.to_string()])
            .args(&args)
            .output()
            .expect("spawn network");
        let elapsed = started.elapsed();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{args:?} failed: stdout={stdout} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            elapsed <= Duration::from_millis(TIMEOUT_MS) + SLACK,
            "{args:?} --timeout {TIMEOUT_MS} took {elapsed:?}: the wall clock must bound the capture"
        );

        let json: Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{args:?} output is not JSON: {e}\n{stdout}"));
        // Summary mode carries the fields under `results`; detail mode at the
        // envelope's top level.
        let fields = if json["results"].is_object() {
            &json["results"]
        } else {
            &json
        };
        let total = fields["total_requests"].as_u64().unwrap_or(0);
        assert!(
            total > 0,
            "{args:?}: a 200 ms beacon page must yield entries within {TIMEOUT_MS} ms: {json}"
        );
        assert_eq!(
            fields["timeout_reached"], true,
            "{args:?}: a capture the wall clock cut short must say so: {json}"
        );
        assert!(
            fields["hint"]
                .as_str()
                .is_some_and(|h| h.contains("--timeout")),
            "{args:?}: the timeout hint must name --timeout, not navigate's --network-timeout: {json}"
        );
        eprintln!("{args:?}: {elapsed:?}, {total} requests, timeout_reached=true");
    }
}
