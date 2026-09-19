//! Live protocol fidelity checks for the typed Watcher front (iteration 265).

mod support;

use std::time::{Duration, Instant};

use ff_rdp_core::{Front, RdpConnection, RootActor, TabActor, WatcherFront};
use serde_json::{Value, json};
use support::recording::{firefox_port, should_run_live};

const TIMEOUT: Duration = Duration::from_secs(10);

fn raw_call(
    conn: &mut RdpConnection,
    watcher: &WatcherFront,
    method: &str,
    mut request: Value,
) -> Value {
    request["to"] = json!(watcher.id().as_ref());
    request["type"] = json!(method);
    conn.transport_mut().send(&request).expect("send raw probe");
    conn.transport_mut()
        .recv()
        .expect("receive raw probe reply")
}

fn selected_tab_and_watcher() -> (RdpConnection, ff_rdp_core::TabInfo, WatcherFront) {
    let mut conn = RdpConnection::connect("127.0.0.1", firefox_port(), TIMEOUT)
        .expect("connect to live Firefox");
    let tabs = RootActor::list_tabs(conn.transport_mut()).expect("listTabs");
    let tab = tabs
        .iter()
        .find(|tab| tab.selected)
        .or_else(|| tabs.first())
        .expect("live Firefox must expose at least one tab")
        .clone();
    let watcher_id = TabActor::get_watcher(conn.transport_mut(), &tab.actor).expect("getWatcher");
    let watcher = WatcherFront::new(
        watcher_id,
        ff_rdp_core::Registry::default(),
        Some(tab.actor.clone()),
    );
    (conn, tab, watcher)
}

#[test]
#[ignore = "requires live Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_unwatch_resources_is_oneway_and_prompt() {
    if !should_run_live() {
        return;
    }
    let (mut conn, _tab, watcher) = selected_tab_and_watcher();
    watcher
        .watch_resources(conn.transport_mut(), &["console-message"])
        .expect("watchResources ACK");

    // A reverted implementation waits for Firefox's nonexistent ACK and hits
    // this bound; the oneway implementation only writes the packet.
    conn.transport_mut()
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("set bounded read timeout");
    let started = Instant::now();
    watcher
        .unwatch_resources(conn.transport_mut(), &["console-message"])
        .expect("oneway unwatchResources");
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "oneway unwatchResources unexpectedly waited {:?}",
        started.elapsed()
    );
}

#[test]
#[ignore = "requires live Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_parent_context_and_named_actor_replies_decode() {
    if !should_run_live() {
        return;
    }
    let (mut conn, tab, watcher) = selected_tab_and_watcher();
    let browsing_context_id = tab
        .browsing_context_id
        .expect("selected tab must expose browsingContextID");

    let parent_reply = raw_call(
        &mut conn,
        &watcher,
        "getParentBrowsingContextID",
        json!({"browsingContextID": browsing_context_id}),
    );
    assert!(parent_reply.get("browsingContextID").is_some());
    println!("getParentBrowsingContextID reply: {parent_reply}");

    // A top-level tab may report either null or the browser-window embedder
    // context. The assertion is that Firefox accepts the requested ID and its
    // nullable reply decodes through the production typed method.
    let _parent = watcher
        .get_parent_browsing_context_id(conn.transport_mut(), browsing_context_id)
        .expect("getParentBrowsingContextID");

    for (method, key) in [
        ("getBlackboxingActor", "blackboxing"),
        ("getBreakpointListActor", "breakpointList"),
        ("getThreadConfigurationActor", "configuration"),
    ] {
        let reply = raw_call(&mut conn, &watcher, method, json!({}));
        assert!(reply[key]["actor"].is_string(), "{method} reply: {reply}");
        println!("{method} reply: {reply}");
    }

    for (name, actor) in [
        (
            "blackboxing",
            watcher
                .get_blackboxing_actor(conn.transport_mut())
                .expect("getBlackboxingActor"),
        ),
        (
            "breakpointList",
            watcher
                .get_breakpoint_list_actor(conn.transport_mut())
                .expect("getBreakpointListActor"),
        ),
        (
            "thread configuration",
            watcher
                .get_thread_configuration_actor(conn.transport_mut())
                .expect("getThreadConfigurationActor"),
        ),
    ] {
        assert!(
            !actor.as_ref().is_empty(),
            "{name} actor ID must be nonempty"
        );
    }
}
