//! Live tests that require a running Firefox instance.
//!
//! Skipped by default. To run:
//! 1. Start Firefox with the remote debugger enabled:
//!    ```sh
//!    firefox --start-debugger-server 6000
//!    ```
//! 2. Run the ignored tests:
//!    ```sh
//!    FF_RDP_LIVE_TESTS=1 cargo test --package ff-rdp-core -- --ignored
//!    ```
//!
//! Optionally set `FF_RDP_PORT` to override the default port (6000).

use std::time::Duration;

use ff_rdp_core::{RdpConnection, RootActor};

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").is_ok()
}

fn firefox_port() -> u16 {
    std::env::var("FF_RDP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6000)
}

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1 and start Firefox with --start-debugger-server 6000"]
fn live_connect_and_list_tabs() {
    if !live_tests_enabled() {
        return;
    }

    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT)
        .expect("failed to connect to Firefox RDP server");

    let tabs = RootActor::list_tabs(conn.transport_mut()).expect("listTabs failed");

    assert!(
        !tabs.is_empty(),
        "expected at least one tab to be open in Firefox"
    );

    println!("Found {} tab(s):", tabs.len());
    for tab in &tabs {
        println!(
            "  [{sel}] {title} — {url}  (actor={actor})",
            sel = if tab.selected { "x" } else { " " },
            title = tab.title,
            url = tab.url,
            actor = tab.actor,
        );
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1 and start Firefox with --start-debugger-server 6000"]
fn live_selected_tab_is_marked() {
    if !live_tests_enabled() {
        return;
    }

    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT)
        .expect("failed to connect to Firefox RDP server");

    let tabs = RootActor::list_tabs(conn.transport_mut()).expect("listTabs failed");

    let selected_count = tabs.iter().filter(|t| t.selected).count();
    assert!(
        selected_count <= 1,
        "at most one tab should be selected, found {selected_count}"
    );
}
