//! Live tests for iteration 160 — "the JSON envelope asserts more than the
//! command knows".
//!
//! Themes covered:
//! - A/B: `click` hit-tests the element's centre point before dispatching, and
//!   reports `matched`/`reachable` instead of the mislabelled `entered`. An
//!   obscured click exits 1 with `error_type: "click_obscured"`.
//! - C: `type` emits a per-character `keydown`/`keypress`/`keyup` sequence.
//! - D: `consent accept` exits non-zero when nothing was dismissed;
//!   `--allow-no-cmp` restores exit 0.
//! - E: `a11y contrast`'s `capped`/`source` sit at the envelope's top level.
//! - F: `network --jq` no longer switches the results shape.
//! - G: a readiness exception is reported, not replaced by a blind re-probe's
//!   "layout did not stabilise" guess.
//!
//! **Evidence rule for this whole file** (plan §"Test evidence rule"): every
//! behavioural assertion goes through a *separate* `ff-rdp eval` round-trip, not
//! the command's own self-report. `live_140_ref_click_resolves` asserted
//! `click["results"]["clicked"] == true` and nothing else — and that envelope
//! is emitted whether or not anything on the page moved, which is precisely the
//! defect Theme A exists to fix. Instrument a counter on the page, run the
//! command, read the counter back.
//!
//! daemon-parity: these use [`daemon_args`] (no `--no-daemon`) — the default
//! path every real invocation takes.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_160_envelope_honesty -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

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

/// Bring up Firefox with a running daemon, panicking on failure (iter-158
/// Theme D: an `Option` here made every caller `return`, which libtest
/// reports as `ok`).
/// iter-172: report *why* the daemon did not start. This test was the sole
/// failure of iteration-171's live sweep and its message said only "the proxy
/// daemon did not start", which was not enough evidence to attribute it to
/// iteration-172's zero-byte-registry defect or to anything else.
fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    if let Err(reason) = ff.with_daemon_or_reason() {
        panic!(
            "{test}: the proxy daemon did not start for Firefox on port {}: {reason}",
            ff.port()
        );
    }
    ff
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    parse_json(&out, args)
}

fn parse_json(out: &Output, args: &[&str]) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn navigate(port: u16, url: &str) {
    let nav = run(port, &["navigate", url]);
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        combined(&nav)
    );
}

/// **The independent read-back.** Evaluate `expr` in the page through a fresh
/// `ff-rdp eval` invocation and return the value it produced (`eval` puts the
/// value directly in `results`). Nothing in this file trusts a command's own
/// account of what it did.
fn eval_value(port: u16, expr: &str) -> Value {
    let out = run_json(port, &["eval", expr]);
    out["results"].clone()
}

/// Serve a single page at `/` and navigate to it. Returns the live server —
/// the caller must keep it alive for the duration of the test.
fn serve_and_navigate(port: u16, test: &str, html: &str) -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(html.to_owned()));
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("{test}: could not bind fixture HTTP — skipping");
        return None;
    };
    navigate(port, &server.base_url());
    Some(server)
}

// ---------------------------------------------------------------------------
// Themes A/B — click hit-tests before it dispatches
// ---------------------------------------------------------------------------

/// The overlay repro from the plan's `dogfood_path`: a 120x40 button at
/// (100, 100) under a `position:fixed;inset:0` veil that owns the centre point.
const OVERLAY_PAGE: &str = r#"<!doctype html><title>t160 overlay</title><body>
<button id="t" style="position:fixed;left:100px;top:100px;width:120px;height:40px">Hit</button>
<div id="veil" style="position:fixed;inset:0;z-index:9"></div>
<script>
window.__hits = 0;
document.getElementById('t').addEventListener('click', function () { window.__hits++; });
</script>
</body>"#;

/// AC: `live_160_click_obscured_reports_unreachable`.
///
/// Before iter-160 this exact page produced `{"clicked": true, "entered":
/// true, "tag": "BUTTON", "text": "Hit"}` at exit 0 while the button's handler
/// never ran — measured during the 2026-08-13 step-back.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_click_obscured_reports_unreachable() {
    if !live_tests_enabled() {
        eprintln!("live_160_click_obscured_reports_unreachable: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_click_obscured_reports_unreachable");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_click_obscured_reports_unreachable",
        OVERLAY_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    // Sanity: the veil really does own the button's centre point. If this
    // fails the fixture is wrong and every assertion below is meaningless.
    assert_eq!(
        eval_value(port, "document.elementFromPoint(160, 120).id"),
        Value::String("veil".to_owned()),
        "fixture broken: the overlay must own the button's centre point"
    );

    let out = run(port, &["click", "#t"]);
    assert!(
        !out.status.success(),
        "an obscured click must exit non-zero: {}",
        combined(&out)
    );
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));

    let json = parse_json(&out, &["click", "#t"]);
    assert_eq!(json["error_type"], "click_obscured", "{json}");
    assert_eq!(json["obscured_by"], "div#veil", "{json}");
    assert_eq!(json["matched"], true, "{json}");
    assert_eq!(json["reachable"], false, "{json}");

    // The independent read-back: nothing on the page moved.
    assert_eq!(
        eval_value(port, "window.__hits"),
        Value::from(0),
        "no click may have been dispatched through the overlay"
    );

    stop_daemon(port);
}

/// AC: `live_160_click_reachable_fires_handler` — with the overlay removed the
/// click lands, and the page (not the command) says so.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_click_reachable_fires_handler() {
    if !live_tests_enabled() {
        eprintln!("live_160_click_reachable_fires_handler: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_click_reachable_fires_handler");
    let port = ff.port();
    let Some(_server) =
        serve_and_navigate(port, "live_160_click_reachable_fires_handler", OVERLAY_PAGE)
    else {
        stop_daemon(port);
        return;
    };

    assert_eq!(
        eval_value(port, "document.getElementById('veil').remove(); 'gone'"),
        Value::String("gone".to_owned())
    );

    let click = run_json(port, &["click", "#t"]);
    assert_eq!(click["results"]["matched"], true, "{click}");
    assert_eq!(click["results"]["reachable"], true, "{click}");
    assert_eq!(click["results"]["clicked"], true, "{click}");
    assert!(
        click["results"].get("entered").is_none(),
        "`entered` was dropped in iter-160 — it meant 'querySelector matched' \
         while its name claimed the pointer could enter: {click}"
    );

    assert_eq!(
        eval_value(port, "window.__hits"),
        Value::from(1),
        "the click must have reached the button's own handler"
    );

    stop_daemon(port);
}

/// AC: `live_160_click_descendant_hit_counts_as_reachable` — a `<span>` inside
/// the button is the *normal* case and must not be reported as an obstruction.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_click_descendant_hit_counts_as_reachable() {
    if !live_tests_enabled() {
        eprintln!("live_160_click_descendant_hit_counts_as_reachable: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_click_descendant_hit_counts_as_reachable");
    let port = ff.port();
    let html = r#"<!doctype html><title>t160 descendant</title><body>
<button id="b" style="position:fixed;left:40px;top:40px;width:160px;height:48px">
  <span id="inner" style="display:block;width:100%;height:100%">Go</span>
</button>
<script>
window.__hits = 0;
document.getElementById('b').addEventListener('click', function () { window.__hits++; });
</script>
</body>"#;
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_click_descendant_hit_counts_as_reachable",
        html,
    ) else {
        stop_daemon(port);
        return;
    };

    // Sanity: the centre point really does resolve to the inner span, so this
    // test is exercising the descendant branch and not a trivial self-hit.
    assert_eq!(
        eval_value(
            port,
            "(function(){var r=document.getElementById('b').getBoundingClientRect();\
             return document.elementFromPoint(r.left+r.width/2, r.top+r.height/2).id;})()"
        ),
        Value::String("inner".to_owned()),
        "fixture broken: the centre point must resolve to the inner span"
    );

    let click = run_json(port, &["click", "#b"]);
    assert_eq!(click["results"]["reachable"], true, "{click}");
    assert_eq!(
        eval_value(port, "window.__hits"),
        Value::from(1),
        "a descendant hit is a reachable click"
    );

    stop_daemon(port);
}

/// AC: `live_160_ref_click_asserts_handler_effect` — `live_140_ref_click_resolves`
/// asserted only the command's self-report. Same scenario, with the effect read
/// back independently.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_ref_click_asserts_handler_effect() {
    if !live_tests_enabled() {
        eprintln!("live_160_ref_click_asserts_handler_effect: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_ref_click_asserts_handler_effect");
    let port = ff.port();
    let html = r#"<!doctype html><title>t160 refs</title><body>
<button>One</button><button id="two">Two</button>
<script>
window.__two = 0;
document.getElementById('two').addEventListener('click', function () { window.__two++; });
</script>
</body>"#;
    let Some(_server) = serve_and_navigate(port, "live_160_ref_click_asserts_handler_effect", html)
    else {
        stop_daemon(port);
        return;
    };

    let dom = run_json(port, &["dom", "button"]);
    let ref_two = dom["results"]
        .as_array()
        .expect("two matches")
        .iter()
        .find(|r| r["name"] == "Two")
        .and_then(|r| r["ref"].as_str())
        .unwrap_or_else(|| panic!("no ref for the 'Two' button in {dom}"))
        .to_owned();

    let click = run_json(port, &["click", "--ref", &ref_two]);
    assert_eq!(click["results"]["clicked"], true, "{click}");
    assert_eq!(click["results"]["text"], "Two", "{click}");

    assert_eq!(
        eval_value(port, "window.__two"),
        Value::from(1),
        "the ref click must have reached the SECOND button's own handler — \
         the envelope's own `clicked: true` proves nothing"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme C — `type` emits key events
// ---------------------------------------------------------------------------

/// AC: `live_160_type_emits_key_events`.
///
/// Measured before iter-160: `JSON.stringify(window.__keys)` was `"[]"` — the
/// command dispatched only `input` and `change`, so a combobox that opens on
/// `keydown` saw nothing while `{"typed": true}` was reported.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_type_emits_key_events() {
    if !live_tests_enabled() {
        eprintln!("live_160_type_emits_key_events: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_type_emits_key_events");
    let port = ff.port();
    let html = r#"<!doctype html><title>t160 keys</title><body>
<input id="q">
<script>
window.__keys = [];
var q = document.getElementById('q');
q.addEventListener('keydown', function (e) { window.__keys.push('keydown:' + e.key); });
q.addEventListener('keyup', function (e) { window.__keys.push('keyup:' + e.key); });
</script>
</body>"#;
    let Some(_server) = serve_and_navigate(port, "live_160_type_emits_key_events", html) else {
        stop_daemon(port);
        return;
    };

    let typed = run_json(port, &["type", "#q", "hi"]);
    assert_eq!(typed["results"]["typed"], true, "{typed}");
    assert_eq!(
        typed["results"]["synthetic"], true,
        "the isTrusted ceiling must be stated in the envelope: {typed}"
    );

    assert_eq!(
        eval_value(port, "JSON.stringify(window.__keys)"),
        Value::String(r#"["keydown:h","keyup:h","keydown:i","keyup:i"]"#.to_owned()),
        "each character must produce a keydown/keyup pair"
    );
    assert_eq!(
        eval_value(port, "document.getElementById('q').value"),
        Value::String("hi".to_owned()),
        "the value must still land"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme D — `consent accept` does not report success with the banner up
// ---------------------------------------------------------------------------

/// A page with a cookie banner ff-rdp's CMP tables do not recognise — the
/// common real-world case, and the one that used to exit 0.
const UNKNOWN_BANNER_PAGE: &str = r#"<!doctype html><title>t160 banner</title><body>
<div id="banner" style="position:fixed;inset:0;background:#eee">
  <p>We value your privacy</p><button id="ok">Continue</button>
</div>
<p>content</p>
</body>"#;

/// AC: `live_160_consent_no_cmp_exits_nonzero`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_consent_no_cmp_exits_nonzero() {
    if !live_tests_enabled() {
        eprintln!("live_160_consent_no_cmp_exits_nonzero: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_consent_no_cmp_exits_nonzero");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_consent_no_cmp_exits_nonzero",
        UNKNOWN_BANNER_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    let out = run(port, &["consent", "accept"]);
    assert!(
        !out.status.success(),
        "`consent accept` must not report success with the banner still up: {}",
        combined(&out)
    );
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));

    // Exactly ONE JSON document on stdout — the error envelope. Printing the
    // success envelope first and *then* failing would be the iter-153
    // double-envelope shape, so this parses rather than substring-matches.
    let json = parse_json(&out, &["consent", "accept"]);
    assert_eq!(json["error_type"], "consent_no_cmp", "{json}");
    assert_eq!(json["status"], "no_cmp_detected", "{json}");
    assert!(
        json.as_object().is_some_and(|o| o.contains_key("cmp")),
        "cmp/action keep their always-present-key discipline on the error path too: {json}"
    );

    // The independent read-back: the banner is still in the document.
    assert_eq!(
        eval_value(port, "document.getElementById('banner') !== null"),
        Value::Bool(true),
        "nothing was dismissed, which is exactly why the exit code changed"
    );

    stop_daemon(port);
}

/// AC: `live_160_consent_allow_no_cmp_exits_zero` — the speculative-caller
/// opt-out.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_consent_allow_no_cmp_exits_zero() {
    if !live_tests_enabled() {
        eprintln!("live_160_consent_allow_no_cmp_exits_zero: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_consent_allow_no_cmp_exits_zero");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_consent_allow_no_cmp_exits_zero",
        UNKNOWN_BANNER_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    let args = [
        "consent",
        "accept",
        "--allow-no-cmp",
        "--jq",
        ".results.status",
    ];
    let out = run(port, &args);
    assert!(
        out.status.success(),
        "--allow-no-cmp must restore exit 0: {}",
        combined(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no_cmp_detected"),
        "results.status must name the outcome: {stdout}"
    );

    stop_daemon(port);
}

/// AC: `live_160_with_network_auto_consent_reports_status` — Theme D's
/// `status` field must reach all three producers, including the two
/// `run_with_network` call sites iter-159 added, in both connection modes.
///
/// Exit code stays 0 on all three: a page with no cookie banner is not a
/// failed navigation.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_with_network_auto_consent_reports_status() {
    if !live_tests_enabled() {
        eprintln!("live_160_with_network_auto_consent_reports_status: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_with_network_auto_consent_reports_status");
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(UNKNOWN_BANNER_PAGE.to_owned()),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_160_with_network_auto_consent_reports_status: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = server.base_url();

    let allowed = ["accepted", "detected_not_actioned", "no_cmp_detected"];

    // Plain `navigate --auto-consent` — `merge_auto_consent`.
    let plain = run_json(port, &["navigate", &url, "--auto-consent"]);
    let plain_status = plain["results"]["consent"]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("no consent.status on plain --auto-consent: {plain}"))
        .to_owned();
    assert!(allowed.contains(&plain_status.as_str()), "{plain}");

    // Daemon branch of `run_with_network` (navigate.rs:2145).
    let daemon = run_json(
        port,
        &["navigate", &url, "--with-network", "--auto-consent"],
    );
    let daemon_status = daemon["results"]["consent"]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("no consent.status on daemon --with-network: {daemon}"))
        .to_owned();
    assert!(allowed.contains(&daemon_status.as_str()), "{daemon}");

    // Direct branch of `run_with_network` (navigate.rs:2325).
    let direct_out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["--no-daemon", "navigate", &url])
        .args(["--with-network", "--auto-consent"])
        .output()
        .expect("spawn ff-rdp --no-daemon navigate");
    assert!(
        direct_out.status.success(),
        "--no-daemon --with-network --auto-consent must still exit 0: {}",
        combined(&direct_out)
    );
    let direct = parse_json(&direct_out, &["navigate", "--no-daemon"]);
    let direct_status = direct["results"]["consent"]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("no consent.status on direct --with-network: {direct}"))
        .to_owned();
    assert!(allowed.contains(&direct_status.as_str()), "{direct}");

    // One vocabulary across all three producers.
    assert_eq!(plain_status, daemon_status, "daemon branch drifted");
    assert_eq!(plain_status, direct_status, "direct branch drifted");

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme E — a capped contrast sample says so at the top level
// ---------------------------------------------------------------------------

/// AC: `live_160_contrast_cap_and_source_at_top_level`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_contrast_cap_and_source_at_top_level() {
    if !live_tests_enabled() {
        eprintln!("live_160_contrast_cap_and_source_at_top_level: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_contrast_cap_and_source_at_top_level");
    let port = ff.port();

    // >1000 text-bearing elements, which is the in-page cap
    // (`elements.length > 1000` in the contrast JS).
    let rows: String = (0..1200).fold(String::new(), |mut acc, i| {
        use std::fmt::Write as _;
        let _ = write!(acc, "<p>row {i}</p>");
        acc
    });
    let body = format!("<!doctype html><title>t160 cap</title><body>{rows}</body>");

    let Some(_server) =
        serve_and_navigate(port, "live_160_contrast_cap_and_source_at_top_level", &body)
    else {
        stop_daemon(port);
        return;
    };

    // Independent read-back: the page really does exceed the cap.
    let count = eval_value(port, "document.querySelectorAll('p').length");
    assert!(
        count.as_u64().unwrap_or(0) > 1000,
        "fixture broken: need >1000 text elements, got {count}"
    );

    let json = run_json(port, &["a11y", "contrast", "--fail-only"]);
    assert_eq!(
        json["capped"], true,
        "`capped` must be reachable as --jq '.capped', not only inside meta: {json}"
    );
    assert_eq!(
        json["source"], "js-fallback",
        "`source` must be at the top level too: {json}"
    );
    assert!(
        json.get("sampled").is_some(),
        "the iter-127 `sampled` field must survive: {json}"
    );
    // The meta copies are retained for compatibility.
    assert_eq!(json["meta"]["summary"]["capped"], true, "{json}");

    // A truncated sample that found nothing must carry the qualifier with it.
    // Hints default to off for JSON output (`--hints` opts in; they are on by
    // default in `--format text`, which is the mode where `capped` would
    // otherwise be invisible), so ask for them explicitly here.
    let sampled = json["sampled"].as_u64().expect("sampled must be a number");
    assert_eq!(
        json["total"].as_u64(),
        Some(0),
        "fixture assumption: this page has no AA failures, which is what makes \
         it the false-good case: {json}"
    );
    let hinted = run_json(port, &["a11y", "contrast", "--fail-only", "--hints"]);
    let hints = hinted["hints"].to_string();
    assert!(
        hints.contains("truncated"),
        "a zero result from a truncated sample must not be printable as a \
         clean pass without the qualifier: {hinted}"
    );
    assert!(
        hints.contains(&sampled.to_string()),
        "the hint must name the sampled element count ({sampled}): {hinted}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme F — --jq does not change the shape it filters
// ---------------------------------------------------------------------------

/// AC: `live_160_network_results_shape_ignores_jq`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_network_results_shape_ignores_jq() {
    if !live_tests_enabled() {
        eprintln!("live_160_network_results_shape_ignores_jq: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_network_results_shape_ignores_jq");
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>t160 net</title><body>net</body>".to_owned()),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_160_network_results_shape_ignores_jq: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    navigate(port, &server.base_url());
    let with_net = run(port, &["navigate", &server.base_url(), "--with-network"]);
    assert!(
        with_net.status.success(),
        "navigate --with-network failed: {}",
        combined(&with_net)
    );

    let out = run(port, &["network", "--jq", ".results | type"]);
    assert!(out.status.success(), "{}", combined(&out));
    let shape = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        shape.contains("object"),
        "`--jq` must filter the envelope, not change it: expected \"object\" \
         (identical to plain `network`), got {shape}"
    );

    let detail = run(port, &["network", "--detail", "--jq", ".results | type"]);
    assert!(detail.status.success(), "{}", combined(&detail));
    let detail_shape = String::from_utf8_lossy(&detail.stdout).trim().to_owned();
    assert!(
        detail_shape.contains("array"),
        "--detail is the documented way to the entry list: got {detail_shape}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme G — the real exception survives the diagnostic
// ---------------------------------------------------------------------------

const SELECTOR_DIAG_PAGE: &str = r#"<!doctype html><title>t160 diag</title><body>
<input id="hidden_input" style="display:none">
<p>content</p>
</body>"#;

/// AC: `live_160_type_non_input_reports_thrown_reason`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_type_non_input_reports_thrown_reason() {
    if !live_tests_enabled() {
        eprintln!("live_160_type_non_input_reports_thrown_reason: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_type_non_input_reports_thrown_reason");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_type_non_input_reports_thrown_reason",
        SELECTOR_DIAG_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    let out = run(port, &["type", "body", "hello"]);
    assert!(!out.status.success(), "{}", combined(&out));
    let text = combined(&out);
    assert!(
        text.contains("is not an input, textarea, select, or contenteditable"),
        "the thrown reason must survive the diagnostic: {text}"
    );
    assert!(
        !text.contains("layout did not stabilise"),
        "layout was fine — that string is the diagnostic's last remaining \
         branch, not a finding: {text}"
    );
    assert!(
        !text.contains("rect did not stabilise"),
        "wrong failure mode reported: {text}"
    );

    stop_daemon(port);
}

/// AC: `live_160_selector_diagnostics_survive` — the sibling messages on this
/// path are among the best error text in the repo and must be byte-identical
/// to the strings in `js_helpers.rs`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_160_selector_diagnostics_survive() {
    if !live_tests_enabled() {
        eprintln!("live_160_selector_diagnostics_survive: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_160_selector_diagnostics_survive");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_160_selector_diagnostics_survive",
        SELECTOR_DIAG_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    let not_found = run(port, &["type", "--timeout", "3000", "#nosuch", "hello"]);
    assert!(!not_found.status.success(), "{}", combined(&not_found));
    assert!(
        combined(&not_found).contains("0 elements matched (not found)"),
        "not-found diagnostic regressed: {}",
        combined(&not_found)
    );

    let hidden = run(port, &["type", "#hidden_input", "hello"]);
    assert!(!hidden.status.success(), "{}", combined(&hidden));
    assert!(
        combined(&hidden).contains("the 1 matching element is hidden"),
        "hidden diagnostic regressed — for display:none the diagnostic's \
         hidden-aware text is the better message and must still win: {}",
        combined(&hidden)
    );

    stop_daemon(port);
}
