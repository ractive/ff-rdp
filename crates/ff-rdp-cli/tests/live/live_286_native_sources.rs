//! Native-path regression for iteration 286. Fallback cannot satisfy this test.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::common::{
    FixtureRoute, FixtureServer, IsolatedLiveFirefox, base_args, bounded_command_output,
    ff_rdp_bin, live_tests_enabled, output_note,
};

/// Predicate polling of this session's real `tabs` results. Empty tabs are the
/// only retryable observation; a command/parse failure ends setup immediately.
fn wait_for_debuggable_tab(
    budget: Duration,
    mut elapsed: impl FnMut() -> Duration,
    mut probe: impl FnMut(Duration) -> Result<Value, String>,
    mut observe: impl FnMut(Value),
    mut after_empty: impl FnMut(Duration),
) -> Result<(), String> {
    let deadline = elapsed() + budget;
    loop {
        let remaining = deadline.saturating_sub(elapsed());
        if remaining.is_zero() {
            return Err("tab setup expired without an observed debuggable tab".to_owned());
        }
        let response = match probe(remaining.min(Duration::from_secs(10))) {
            Ok(response) => response,
            Err(error) => {
                observe(
                    json!({"phase":"probe-error","elapsed_ms":elapsed().as_millis(),"error":error}),
                );
                return Err(format!("tab setup probe failed: {error}"));
            }
        };
        let tabs = response["results"]
            .as_array()
            .ok_or_else(|| format!("tab setup returned no results array: {response}"))?;
        let ready = tabs
            .first()
            .is_some_and(|tab| tab["actor"].as_str().is_some_and(|actor| !actor.is_empty()));
        let in_budget = elapsed() < deadline;
        observe(
            json!({"phase":"tabs","elapsed_ms":elapsed().as_millis(),"tabs":tabs,
            "ready":ready,"within_setup_budget":in_budget}),
        );
        if !in_budget {
            return Err("tab setup probe completed after its deadline".to_owned());
        }
        if ready {
            return Ok(());
        }
        if !tabs.is_empty() {
            return Err("tab setup returned an unusable first descriptor".to_owned());
        }
        // This delay follows an actual empty predicate result, never a failed
        // navigate. Charge it to the same fixed setup deadline.
        after_empty(
            deadline
                .saturating_sub(elapsed())
                .min(Duration::from_millis(100)),
        );
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_286_sources_returns_native_script_actor() {
    assert!(live_tests_enabled(), "set FF_RDP_LIVE_TESTS=1");
    let active_deadline = Instant::now() + Duration::from_secs(90);
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><script src='/iteration286.js'></script>"),
    );
    routes.insert(
        "/iteration286.js".to_owned(),
        FixtureRoute {
            content_type: "application/javascript",
            body: b"document.documentElement.dataset.iteration286 = 'ready';".to_vec(),
            ..FixtureRoute::default()
        },
    );
    let server = FixtureServer::start(routes).expect("bind controlled HTTP fixture");
    // The session supplies non-panicking owned cleanup during assertion unwind.
    let session = IsolatedLiveFirefox::launch(&ff_rdp_bin())
        .expect("launch isolated Firefox for native sources");
    let port = session.firefox().port();
    let setup_start = Instant::now();
    let setup_budget =
        Duration::from_secs(30).min(active_deadline.saturating_duration_since(setup_start));
    wait_for_debuggable_tab(
        setup_budget,
        || setup_start.elapsed(),
        |bound| {
            if !crate::common::pid_alive(session.receipt().pid) {
                return Err("owned Firefox exited before tab readiness".to_owned());
            }
            let output = bounded_command_output(
                session.command().args(base_args(port)).arg("tabs"),
                bound,
                "iteration286 owned tab predicate",
            )?;
            if !output.status.success() {
                return Err(output_note(&output));
            }
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
        },
        |observation| {
            eprintln!(
                "ITER286_TAB_READY {}",
                json!({
                    "pid":session.receipt().pid,"port":port,"profile":session.receipt().profile,
                    "observation":observation,
                })
            );
        },
        std::thread::sleep,
    )
    .expect("iteration286 setup failed before its single navigate");
    let command_bound = || {
        let remaining = active_deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "iteration286 active budget exhausted");
        remaining.min(Duration::from_secs(10))
    };
    let url = format!("{}/", server.base_url());
    let expected = format!("{url}iteration286.js");
    let nav = bounded_command_output(
        session
            .command()
            .args(base_args(port))
            .args(["navigate", &url]),
        command_bound(),
        "single navigate to controlled script",
    )
    .expect("navigate to controlled script");
    assert!(nav.status.success(), "{}", output_note(&nav));
    let ready = bounded_command_output(
        session.command().args(base_args(port)).args([
            "wait",
            "--selector",
            "html[data-iteration286='ready']",
        ]),
        command_bound(),
        "controlled script readiness",
    )
    .expect("wait for controlled script execution");
    assert!(ready.status.success(), "{}", output_note(&ready));
    let output = bounded_command_output(
        session
            .command()
            .args(base_args(port))
            .args(["--tab", &url, "sources"]),
        command_bound(),
        "single native sources command",
    )
    .expect("native sources command");
    assert!(output.status.success(), "{}", output_note(&output));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(result["meta"].get("fallback").is_none(), "{result}");
    let source = result["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["url"] == expected)
        .expect("native list contains the executed external script");
    assert!(!source["actor"].as_str().unwrap().is_empty(), "{source}");
    assert!(
        Instant::now() <= active_deadline,
        "iteration286 active phase exceeded 90s"
    );
    session
        .finish()
        .expect("clean up isolated Firefox and its private profile");
}

// allow-ungated-live: Firefox-free scripted tab discovery tests the real setup poller.
#[test]
fn tab_setup_waits_for_observed_descriptor_after_empty_results() {
    use std::cell::Cell;
    let clock = Cell::new(Duration::ZERO);
    let mut replies = [
        json!({"results":[],"total":99}),
        json!({"results":[{"actor":"owned-tab","url":"about:blank"}]}),
    ]
    .into_iter();
    let mut observations = Vec::new();
    let mut bounds = Vec::new();
    wait_for_debuggable_tab(
        Duration::from_secs(30),
        || clock.get(),
        |bound| {
            bounds.push(bound);
            Ok(replies.next().expect("no unnecessary probe"))
        },
        |row| observations.push(row),
        |delay| clock.set(clock.get() + delay),
    )
    .unwrap();
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations[0]["ready"], false,
        "total alone is not tab readiness"
    );
    assert_eq!(observations[1]["tabs"][0]["actor"], "owned-tab");
    assert_eq!(observations[1]["ready"], true);
    assert_eq!(bounds, [Duration::from_secs(10); 2]);
    assert_eq!(clock.get(), Duration::from_millis(100));
}

// allow-ungated-live: no browser; expiration must remain a setup failure, never success.
#[test]
fn tab_setup_expires_without_tab_and_rejects_late_nonempty_reply() {
    use std::cell::Cell;
    let clock = Cell::new(Duration::ZERO);
    let mut observations = Vec::new();
    let mut bounds = Vec::new();
    let error = wait_for_debuggable_tab(
        Duration::from_millis(250),
        || clock.get(),
        |bound| {
            bounds.push(bound);
            Ok(json!({"results":[]}))
        },
        |row| observations.push(row),
        |delay| clock.set(clock.get() + delay),
    )
    .unwrap_err();
    assert!(error.contains("expired"));
    assert_eq!(bounds, [250, 150, 50].map(Duration::from_millis));
    assert_eq!(observations.len(), 3);
    assert!(observations.iter().all(|row| row["ready"] == false));
    let clock = Cell::new(Duration::ZERO);
    let error = wait_for_debuggable_tab(
        Duration::from_millis(250),
        || clock.get(),
        |_| {
            clock.set(Duration::from_millis(251));
            Ok(json!({"results":[{"actor":"late-tab"}]}))
        },
        |_| {},
        |_| panic!("must not poll after a late response"),
    )
    .unwrap_err();
    assert!(error.contains("after its deadline"));
}
