//! Auto-wait's500ms stability budget must also bound diagnostic reads.
//! Run the actual CLI on owned Firefox instances, on direct and daemon routes.
//! daemon-parity: live_272_diagnostics_and_blocked_stability_both_routes exercises both routes.
use std::process::Output;
use std::time::{Duration, Instant};

use crate::common::{LiveFirefox, bounded_command_output, live_tests_enabled};

fn run(port: u16, direct: bool, args: &[&str]) -> Output {
    let mut command = crate::common::action_route::command(port, direct, args);
    bounded_command_output(&mut command, Duration::from_secs(20), "iteration272 CLI")
        .unwrap_or_else(|error| panic!("{args:?}: {error}"))
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn eval(port: u16, direct: bool, expression: &str) -> serde_json::Value {
    let output = run(port, direct, &["eval", expression]);
    assert!(
        output.status.success(),
        "eval {expression}: {}",
        text(&output)
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["results"].clone()
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_272_diagnostics_and_blocked_stability_both_routes() {
    assert!(live_tests_enabled(), "set FF_RDP_LIVE_TESTS=1");
    let firefox = LiveFirefox::headless_on_random_port();
    firefox.with_daemon_or_reason().unwrap();
    let port = firefox.port();
    for direct in [true, false] {
        for case in [
            "absent",
            "hidden",
            "hidden_many",
            "moving",
            "blocked_readiness",
            "blocked_stability",
        ] {
            let mut setup = "document.body.innerHTML='<input id=target><input class=twins style=display:none><input class=twins>';window.__probeCalls=0;".to_owned();
            let selector = match case {
                "absent" => "#missing",
                "hidden_many" => ".twins",
                _ => "#target",
            };
            match case {
                "hidden" => setup.push_str("document.getElementById('target').style.display='none';"),
                "moving" => setup.push_str("document.getElementById('target').getBoundingClientRect=function(){window.__probeCalls++;return {top:window.__probeCalls,left:0,width:100,height:20}};"),
                "blocked_readiness" => setup.push_str("window.__qs=document.querySelector.bind(document);document.querySelector=function(s){if(s==='#target'&&window.__probeCalls++===0){var end=Date.now()+2500;while(Date.now()<end){}}return window.__qs(s)};"),
                "blocked_stability" => setup.push_str("window.__rect=document.getElementById('target').getBoundingClientRect.bind(document.getElementById('target'));document.getElementById('target').getBoundingClientRect=function(){if(++window.__probeCalls===2){var end=Date.now()+2500;while(Date.now()<end){}}return window.__rect()};"),
                _ => {}
            }
            setup.push_str("true");
            assert_eq!(eval(port, direct, &setup), true);
            let start = Instant::now();
            let output = if matches!(case, "absent" | "moving") {
                run(port, direct, &["click", selector])
            } else {
                run(port, direct, &["type", selector, "hello"])
            };
            let elapsed = start.elapsed();
            let message = text(&output);
            let action = if matches!(case, "absent" | "moving") {
                "click"
            } else {
                "type"
            };
            crate::common::action_route::require_action_route(&output.stderr, action, !direct)
                .unwrap_or_else(|error| panic!("{case}: {error}: {message}"));
            eprintln!(
                "ITER272 route={} case={case} wall_ms={} status={} output={message}",
                if direct { "direct" } else { "daemon" },
                elapsed.as_millis(),
                output.status
            );
            // A timed-out console operation can still be running in Firefox.
            // This finite read-back waits for the fixture's2500ms operation,
            // proves it ran, and resets only our page hook.
            let calls = eval(
                port,
                direct,
                "if(window.__qs){document.querySelector=window.__qs;delete window.__qs;}window.__probeCalls",
            );
            if case == "blocked_readiness" {
                assert!(output.status.success(), "{message}");
                assert!(calls.as_u64().is_some_and(|count| count > 0));
                assert_eq!(
                    eval(port, direct, "document.getElementById('target').value"),
                    "hello"
                );
                continue;
            }
            assert!(!output.status.success(), "{case}: {message}");
            let expected = match case {
                "absent" => "0 elements matched (not found)",
                "hidden" => "the 1 matching element is hidden",
                "hidden_many" => "matched 2 elements, chose index 0 which is hidden",
                "moving" => "rect did not stabilise",
                "blocked_stability" => "selector diagnostic",
                _ => unreachable!(),
            };
            assert!(message.contains(expected), "{case}: {message}");
            if case == "blocked_stability" {
                assert!(
                    elapsed < Duration::from_millis(1_800),
                    "blocked probe inherited4000ms socket timeout: {elapsed:?}: {message}"
                );
                assert_eq!(calls, 2, "the intended second rect call must have blocked");
                assert!(
                    !message.contains("rect did not stabilise"),
                    "blocked is not moving: {message}"
                );
                assert_eq!(
                    eval(port, direct, "document.getElementById('target').value"),
                    "",
                    "no type action after failed readiness"
                );
            }
        }
    }
    let stop = run(port, false, &["daemon", "stop"]);
    assert!(stop.status.success(), "{}", text(&stop));
}
