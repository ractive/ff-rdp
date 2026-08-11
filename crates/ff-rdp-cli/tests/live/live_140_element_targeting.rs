//! Live tests for iteration 140 — element targeting: refs, ambiguous
//! selectors, frame diagnostics.
//!
//! From [[dogfooding-session-63]]: `--ref` was advertised across many
//! commands and broken three ways (round-tripped as an invalid JS
//! expression, single-use registry, 10s burn on an invalid selector);
//! ambiguous selectors silently picked a hidden element and timed out with
//! an undifferentiated message; frame diagnostics produced 65 KB errors and
//! miscounted the tried-frame total.
//!
//! Themes covered:
//! - A: `--ref` resolves to a real, reusable CSS selector; expiry vs
//!   not-registered is reported correctly.
//! - B/C: ambiguous-selector timeouts name the match count and distinguish
//!   hidden from not-found; `--visible`/`--index` recover the right element.
//! - D: frame-scan errors are bounded in size; `--frame` reports the
//!   filtered candidate count, not the total.
//! - F: generated page-map selectors resolve to exactly one element.
//!
//! (Theme E — `--jq '.results.frame_url'` never throws — is covered by the
//! fast e2e tests `click_frame_url_present_in_both_results_and_meta` /
//! `click_jq_results_frame_url_does_not_throw` in `tests/e2e/click.rs`; no
//! live Firefox is needed to prove a JSON-shape regression, so it isn't
//! duplicated here.)
//!
//! Run-guidance rule 1 (do not trust the plan's stated root cause without
//! verifying on the wire): this iteration's own diagnosis needed a
//! correction. The starting WIP treated any `frameUpdate{isTopLevel:true}` as
//! a navigation (fixing the *original* single-use bug, where every
//! `frameUpdate` cleared the ref store). Reproducing `live_140_ref_reusable`'s
//! scenario against real Firefox 153 with `RUST_LOG=debug` showed Fission
//! spawns a fresh `childN/windowGlobalTargetN` actor pair for the SAME
//! committed URL on almost every RDP round-trip, each emitting its own
//! `isTopLevel: true` `frameUpdate` — so the narrowed rule *still* wiped the
//! store between two ref resolutions with zero real navigation involved. The
//! actual fix (`is_navigation_event` in `daemon/server.rs`) additionally
//! compares the frame's URL against the last `tabNavigated`-committed URL and
//! only treats a same-`isTopLevel`-but-different-URL frameUpdate as a
//! navigation.
//!
//! daemon-parity: every test here uses [`daemon_args`] (no `--no-daemon`) —
//! the daemon owns the ref store (`--ref` is daemon-only) and frame-target
//! enumeration through the proxy is exactly what iter-137 fixed for the
//! default connection mode.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_140_element_targeting -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// Args for the **default** connection mode: no `--no-daemon`, so the CLI
/// auto-starts and proxies through the daemon — the path every real
/// invocation uses (see the module-level `daemon-parity` note).
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "20000".to_owned(),
    ]
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Bring up Firefox with a running daemon, or `None` with a printed reason.
fn firefox_with_daemon(test: &str) -> Option<LiveFirefox> {
    let ff = LiveFirefox::headless_on_random_port()?;
    if ff.with_daemon().is_none() {
        eprintln!("{test}: daemon did not start — skipping");
        return None;
    }
    Some(ff)
}

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

/// Run `ff-rdp <args>` over the daemon connection and return the raw output
/// (caller decides success/failure — several tests here assert on errors).
fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

/// Run `ff-rdp <args>`, require success, and parse stdout as JSON.
fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

/// Combined stdout+stderr text of a (possibly failing) command.
fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ---------------------------------------------------------------------------
// Theme A — `--ref`: resolves, reusable, correct expiry message
// ---------------------------------------------------------------------------

/// AC: `live_140_ref_click_resolves` — `click --ref eN` after `dom` clicks
/// the right element. Before iter-140, refs round-tripped as a
/// `document.querySelectorAll(sel)[i]` JS *expression* fed straight into
/// `document.querySelector(...)`, which cannot parse it — every `--ref`
/// click failed with a `SyntaxError`, not a click.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_ref_click_resolves() {
    if !live_tests_enabled() {
        eprintln!("live_140_ref_click_resolves: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_ref_click_resolves") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t140 refs</title><body>\
             <button>One</button><button>Two</button></body>",
        ),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_140_ref_click_resolves: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    navigate(port, &server.base_url());

    let dom = run_json(port, &["dom", "button"]);
    let results = dom["results"]
        .as_array()
        .expect("dom results must be an array for a 2-match selector");
    assert_eq!(results.len(), 2, "expected two <button> matches: {dom}");
    let ref_two = results
        .iter()
        .find(|r| r["name"] == "Two")
        .and_then(|r| r["ref"].as_str())
        .unwrap_or_else(|| panic!("no ref for the 'Two' button in {dom}"))
        .to_owned();

    let click = run_json(port, &["click", "--ref", &ref_two]);
    assert_eq!(
        click["results"]["clicked"], true,
        "click --ref {ref_two} must click, not fail on an invalid selector: {click}"
    );
    assert_eq!(
        click["results"]["text"], "Two",
        "the ref must resolve to the SECOND button, not just any button: {click}"
    );

    stop_daemon(port);
}

/// AC: `live_140_ref_reusable` — resolving the same ref twice in a row
/// succeeds both times. Before iter-140 the registry was effectively
/// single-use (every `frameUpdate` cleared it). The first fix pass narrowed
/// that to `isTopLevel: true` `frameUpdate`s, which — confirmed live, see
/// this module's doc comment — still broke on same-URL process-switch
/// `frameUpdate`s that Fission emits on almost every RDP call.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_ref_reusable() {
    if !live_tests_enabled() {
        eprintln!("live_140_ref_reusable: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_ref_reusable") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>t140 reusable</title><body><h1>hi</h1></body>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_140_ref_reusable: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    navigate(port, &server.base_url());

    let dom = run_json(port, &["dom", "h1"]);
    let ref_id = dom["results"][0]["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("dom result missing 'ref': {dom}"))
        .to_owned();

    for attempt in 1..=3 {
        let styles = run_json(port, &["styles", "--ref", &ref_id]);
        let total = styles["total"].as_u64().unwrap_or(0);
        assert!(
            total > 0,
            "resolve #{attempt} of ref {ref_id} must succeed with a non-empty style list: {styles}"
        );
    }

    stop_daemon(port);
}

/// AC: `live_140_ref_expiry_message` — a ref invalidated by navigation
/// reports expiry, not "not registered". Before iter-140 the daemon compared
/// against `next` (which resets to 1 on every `clear()`), so immediately
/// after a real navigation every previously-valid id looked "never
/// allocated" instead of "expired".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_ref_expiry_message() {
    if !live_tests_enabled() {
        eprintln!("live_140_ref_expiry_message: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_ref_expiry_message") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/a".to_owned(),
        FixtureRoute::html("<!doctype html><title>t140 expiry A</title><body><h1>A</h1></body>"),
    );
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html("<!doctype html><title>t140 expiry B</title><body><h1>B</h1></body>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_140_ref_expiry_message: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    navigate(port, &format!("{}/a", server.base_url()));
    let dom = run_json(port, &["dom", "h1"]);
    let ref_id = dom["results"][0]["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("dom result missing 'ref': {dom}"))
        .to_owned();

    // A genuine navigation — the ref must now be gone.
    navigate(port, &format!("{}/b", server.base_url()));

    let out = run(port, &["styles", "--ref", &ref_id]);
    assert!(
        !out.status.success(),
        "resolving a ref from before navigation must fail"
    );
    let text = combined(&out);
    assert!(
        text.contains("expired"),
        "a ref allocated before navigation must report expiry: {text}"
    );
    assert!(
        !text.contains("not registered"),
        "must not fall back to the generic 'not registered' message: {text}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B/C — ambiguous selectors: match count reported, --visible recovers
// ---------------------------------------------------------------------------

/// A fixture with two `input[name=keywords]` matches: DOM-order index 0 is
/// `display:none` (hidden), index 1 is a plain visible input — the shape the
/// plan's gov.uk repro described (`type` silently taking a hidden `[0]`).
const AMBIGUOUS_INPUT_HTML: &str = "<!doctype html><title>t140 ambiguous</title><body>\
     <input name=\"keywords\" style=\"display:none\" value=\"hidden-slot\">\
     <input name=\"keywords\" value=\"\"></body>";

/// AC: `live_140_ambiguous_selector_reports_count` — error on a 2-match
/// selector names the match count and the chosen index, and distinguishes
/// hidden from not-found. Before iter-140 the timeout was the undifferentiated
/// "not ready (not found / hidden / unstable)" for every cause, with no match
/// count at all.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_ambiguous_selector_reports_count() {
    if !live_tests_enabled() {
        eprintln!("live_140_ambiguous_selector_reports_count: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_ambiguous_selector_reports_count") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(AMBIGUOUS_INPUT_HTML));
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!(
            "live_140_ambiguous_selector_reports_count: could not bind fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };
    navigate(port, &server.base_url());

    // Short --timeout: the display:none match fails the readiness check
    // immediately (no need to wait out a long budget) — see
    // `diagnose_selector_failure` in js_helpers.rs.
    let out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "3000",
        ])
        .args(["type", "input[name=keywords]", "passport renewal"])
        .output()
        .expect("ff-rdp type");
    assert!(
        !out.status.success(),
        "the bare (unqualified) selector must fail — it matches a hidden element at index 0"
    );
    let text = combined(&out);
    assert!(
        text.contains("matched 2 elements") || text.contains("2 elements"),
        "error must name the match count (2): {text}"
    );
    assert!(
        text.contains("hidden"),
        "error must distinguish 'hidden' from a bare not-found: {text}"
    );

    stop_daemon(port);
}

/// AC: `live_140_visible_flag_targets_visible` — `--visible` (or `--index`)
/// reaches the visible match where the bare selector fails, on the exact
/// fixture the previous test proves the bare selector cannot handle.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_visible_flag_targets_visible() {
    if !live_tests_enabled() {
        eprintln!("live_140_visible_flag_targets_visible: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_visible_flag_targets_visible") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(AMBIGUOUS_INPUT_HTML));
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_140_visible_flag_targets_visible: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    navigate(port, &server.base_url());

    let out = run_json(
        port,
        &[
            "type",
            "input[name=keywords]",
            "passport renewal",
            "--visible",
        ],
    );
    assert_eq!(out["results"]["typed"], true, "typed into: {out}");
    assert_eq!(
        out["results"]["match_count"], 2,
        "must report both matches were considered: {out}"
    );
    assert_eq!(
        out["results"]["chosen_index"], 1,
        "must choose the SECOND (visible) match, not DOM-order index 0: {out}"
    );

    // Confirm on the page itself: the value landed on the visible input, the
    // hidden one is untouched.
    let check = run_json(
        port,
        &[
            "eval",
            "JSON.stringify(Array.from(document.querySelectorAll('input[name=keywords]')).map(function(e){return e.value}))",
        ],
    );
    let values_json = check["results"]
        .as_str()
        .expect("eval result must be a JSON string");
    let values: Vec<String> = serde_json::from_str(values_json).expect("valid JSON array");
    assert_eq!(
        values,
        vec!["hidden-slot".to_owned(), "passport renewal".to_owned()],
        "hidden input's original value must be untouched; visible one gets the typed text"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme D — frame diagnostics: bounded errors, accurate --frame count
// ---------------------------------------------------------------------------

/// Build a top-level fixture page embedding `n` same-origin iframes, each
/// with a long junk query string — the same shape (many frames, long URLs)
/// as the plan's 97-frame theguardian.com repro, without needing network
/// access. Same-origin is sufficient: frame-target enumeration tracks
/// BrowsingContexts, not just out-of-process targets (confirmed live — see
/// `live_140_frame_error_bounded`).
fn many_iframes_routes(n: usize) -> HashMap<String, FixtureRoute> {
    use std::fmt::Write as _;

    let mut routes = HashMap::new();
    let junk = "x".repeat(120);
    let mut iframes = String::new();
    for i in 0..n {
        let _ = write!(
            iframes,
            "<iframe src=\"/leaf{i}.html?junk={junk}\"></iframe>"
        );
    }
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>t140 many frames</title><body><h1>top</h1>{iframes}</body>"
        )),
    );
    for i in 0..n {
        routes.insert(
            format!("/leaf{i}.html"),
            FixtureRoute::html(format!("<!doctype html><body><p>leaf {i}</p></body>")),
        );
    }
    routes
}

/// Leaf-iframe count shared by [`live_140_frame_error_bounded`] and
/// [`live_140_frame_filter_count_accurate`]'s fixtures.
const MANY_IFRAMES_N: usize = 14;

/// AC: `live_140_frame_error_bounded` — frame-scan error on a many-frame page
/// is bounded in size. Before iter-140, `click_in_scanned_frame`'s `all_urls`
/// joined every frame URL raw and untruncated — 65 KB on theguardian.com's 97
/// frames.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_frame_error_bounded() {
    if !live_tests_enabled() {
        eprintln!("live_140_frame_error_bounded: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_frame_error_bounded") else {
        return;
    };
    let port = ff.port();

    let Some(server) = FixtureServer::start(many_iframes_routes(MANY_IFRAMES_N)) else {
        eprintln!("live_140_frame_error_bounded: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    navigate(port, &server.base_url());

    // --no-wait skips auto-wait so the missing-selector top-level attempt
    // throws immediately and `do_click` runs its own frame scan directly
    // (see click.rs's `do_click`/`click_in_scanned_frame`).
    let out = run(port, &["click", "--no-wait", "button.nowhere-to-be-found"]);
    assert!(
        !out.status.success(),
        "a selector matching nowhere must fail"
    );
    let text = combined(&out);

    assert!(
        text.len() < 3000,
        "error must be bounded (well under the old 65 KB), got {} bytes: {text}",
        text.len()
    );
    assert!(
        text.contains("more"),
        "error must indicate more frames exist beyond the listed cap: {text}"
    );
    assert!(
        text.contains(&format!("of {} total", MANY_IFRAMES_N + 1)),
        "error must report the true total frame count ({}): {text}",
        MANY_IFRAMES_N + 1
    );

    stop_daemon(port);
}

/// AC: `live_140_frame_filter_count_accurate` — `--frame` reports the
/// filtered candidate count, not the total. Before iter-140,
/// `click_in_scanned_frame`'s zero-match message used `targets.len()`
/// (every frame on the page) even when `--frame` narrowed the scan to a
/// handful of candidates.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_frame_filter_count_accurate() {
    if !live_tests_enabled() {
        eprintln!("live_140_frame_filter_count_accurate: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_frame_filter_count_accurate") else {
        return;
    };
    let port = ff.port();

    // leaf1, leaf10, leaf11, leaf12, leaf13 all contain the substring "leaf1"
    // — 5 of the 14 leaf frames (15 targets total including top).
    let Some(server) = FixtureServer::start(many_iframes_routes(MANY_IFRAMES_N)) else {
        eprintln!("live_140_frame_filter_count_accurate: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    navigate(port, &server.base_url());

    let out = run(
        port,
        &[
            "click",
            "--no-wait",
            "--frame",
            "leaf1",
            "button.nowhere-to-be-found",
        ],
    );
    assert!(
        !out.status.success(),
        "a selector matching nowhere must fail"
    );
    let text = combined(&out);

    assert!(
        text.contains("5 frame(s) tried") || text.contains("of 5"),
        "must report exactly the 5 filtered candidates (leaf1/leaf10/leaf11/leaf12/leaf13), \
         not the full frame count: {text}"
    );
    assert!(
        text.contains(&format!("of {} total", MANY_IFRAMES_N + 1)),
        "must still report the true total ({}) alongside the filtered count: {text}",
        MANY_IFRAMES_N + 1
    );
    assert!(
        !text.contains(&format!("matched in 0 of {} frame", MANY_IFRAMES_N + 1)),
        "must NOT claim every frame was tried when --frame narrowed the scan: {text}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme F — generated page-map selectors are unique
// ---------------------------------------------------------------------------

/// AC: `live_140_page_map_selectors_unique` — generated page-map selectors
/// resolve to exactly one element. Before iter-140, a landmark child element
/// with no `id` fell back to a bare tag name (`"selector": "button"`), which
/// matches every button on the page.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_140_page_map_selectors_unique() {
    if !live_tests_enabled() {
        eprintln!("live_140_page_map_selectors_unique: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_140_page_map_selectors_unique") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t140 page-map</title><body>\
             <nav><a href=\"/a\">A</a><a href=\"/b\">B</a></nav>\
             <main>\
               <form><input name=\"q\"><button type=\"submit\">Go</button></form>\
               <button>Extra 1</button><button>Extra 2</button><button>Extra 3</button>\
             </main></body>",
        ),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_140_page_map_selectors_unique: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let base = server.base_url();
    navigate(port, &base);

    let out_path = std::env::temp_dir().join(format!("t140-page-map-{port}.json"));
    // `index` prints its internal `navigate` call's envelope to stdout ahead
    // of its own result line (a pre-existing, separately-tracked output-
    // hygiene issue — dogfooding-session-63 finding #21 — unrelated to this
    // iteration's Themes A-F), so stdout is not single-object JSON here.
    // Read the written page-map file directly instead, matching
    // `live_62_page_map_index`'s existing pattern for this same command.
    let index_out = run(
        port,
        &[
            "index",
            &base,
            "--max-pages",
            "1",
            "--out",
            out_path.to_str().expect("utf-8 temp path"),
        ],
    );
    assert!(
        index_out.status.success(),
        "ff-rdp index failed: {}",
        combined(&index_out)
    );

    let map_text = std::fs::read_to_string(&out_path)
        .unwrap_or_else(|e| panic!("reading page-map {}: {e}", out_path.display()));
    let map: Value = serde_json::from_str(&map_text).expect("page-map must be valid JSON");
    let _ = std::fs::remove_file(&out_path);

    // Collect every generated selector: landmark regions, landmark child
    // elements, form selectors, field selectors, submit selectors.
    let mut selectors: Vec<String> = Vec::new();
    let page = &map["pages"]["index"];
    if let Some(landmarks) = page["landmarks"].as_array() {
        for lm in landmarks {
            if let Some(s) = lm["region"].as_str() {
                selectors.push(s.to_owned());
            }
            if let Some(elements) = lm["elements"].as_array() {
                for el in elements {
                    if let Some(s) = el["selector"].as_str() {
                        selectors.push(s.to_owned());
                    }
                }
            }
        }
    }
    if let Some(forms) = page["forms"].as_array() {
        for form in forms {
            if let Some(s) = form["selector"].as_str() {
                selectors.push(s.to_owned());
            }
            if let Some(fields) = form["fields"].as_array() {
                for f in fields {
                    if let Some(s) = f["selector"].as_str() {
                        selectors.push(s.to_owned());
                    }
                }
            }
            if let Some(s) = form["submit"]["selector"].as_str() {
                selectors.push(s.to_owned());
            }
        }
    }
    assert!(
        selectors.len() >= 6,
        "expected several generated selectors (landmarks/form/fields/submit), got {}: {map}",
        selectors.len()
    );
    // The page has THREE plain <button>s with no distinguishing attribute —
    // exactly the shape that used to fall back to a bare "button" selector.
    assert!(
        selectors.iter().all(|s| s != "button"),
        "no generated selector may be the bare tag name 'button': {selectors:?}"
    );

    for sel in &selectors {
        let count = run_json(port, &["dom", sel, "--count"]);
        assert_eq!(
            count["results"]["count"], 1,
            "generated selector {sel:?} must resolve to exactly one element: {count}"
        );
    }

    stop_daemon(port);
}
