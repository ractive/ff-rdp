//! `ff-rdp launch --headless <URL>` — the positional shorthand for `--url` —
//! lands the launched Firefox on that page (the form the `home` hint teaches).
//!
//! Uses `about:robots` so the test needs no network (never a `file:` URL). A
//! `data:` startup URL is not used: Firefox 158 blocks it as a top-level load
//! and leaves the tab on about:blank, for `--url` and the positional alike.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_launch_positional -- --include-ignored

use std::time::{Duration, Instant};

use crate::common::{ff_rdp_launch_command, guard_launched_firefox, live_tests_enabled};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_launch_positional_url_lands_on_the_page() {
    if !live_tests_enabled() {
        eprintln!("live_launch_positional_url_lands_on_the_page: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let url = "about:robots";
    let port = free_port();
    let out = ff_rdp_launch_command()
        .args(["launch", "--headless", url])
        .args(["--debug-port", &port.to_string()])
        .output()
        .expect("run ff-rdp launch");
    let _guard = guard_launched_firefox(&out);
    assert!(
        out.status.success(),
        "launch failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("launch JSON");
    assert_eq!(json["results"]["requested_url"], url, "{json}");

    // Launch waits for the debug port, not the document: poll until the
    // startup page is the one in the tab.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = String::new();
    while Instant::now() < deadline {
        let eval = ff_rdp_launch_command()
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["eval", "location.href", "--jq", ".results"])
            .output()
            .expect("run ff-rdp eval");
        last = String::from_utf8_lossy(&eval.stdout).trim().to_owned();
        if eval.status.success() && last.contains(url) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("launched Firefox never showed {url}; last eval: {last}");
}
