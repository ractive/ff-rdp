/// Live test for Theme E (iter-84): `styles applied` deduplicates rules that
/// share the same `rule_actor_id` so that inherited rules from parent elements
/// are not listed multiple times.
///
/// iter-114 Theme B: ported to the self-launch harness using a local `data:`
/// URL fixture where `<p>` is matched by rules spread across two separate
/// `<style>` sheets (plus a `*` selector and a repeated selector within the
/// same sheet), so dedupe is meaningfully exercised rather than relying on
/// whatever cascade example.com happened to produce.
///
/// AC: live_styles_applied_dedupe — `results` contains no duplicate actor IDs
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use std::collections::HashSet;
use std::process::Command;

/// Two `<style>` sheets both targeting `<p>` (directly and via inheritance
/// from `*`/`body`), plus a repeated `p` selector within the second sheet —
/// so `styles p --applied` sees several rules that could plausibly collide
/// on `rule_actor_id` if dedupe regressed.
///
/// Hex colors use `%23` in place of `#` — an unescaped `#` in a `data:` URL
/// is parsed as a fragment delimiter, truncating everything after it before
/// the page ever loads.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head>\
<style>* { margin: 0; } body { color: %23111111; } p { font-size: 14px; }</style>\
<style>p { line-height: 1.5; } p { font-weight: 400; } .note { color: %23222222; }</style>\
</head><body><p class=\"note\">hello world</p></body></html>";

/// Theme E: `styles applied` does not emit duplicate rules when the same CSS
/// rule applies via multiple inheritance paths, or when multiple stylesheets
/// each contribute rules for the same element.
///
/// Post-condition: `results` array has no two entries with the same non-empty
/// `rule_actor_id`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_styles_applied_dedupe_no_duplicate_actor_ids() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_styles_applied_dedupe_no_duplicate_actor_ids: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_styles_applied_dedupe_no_duplicate_actor_ids: Firefox not available — skipping"
        );
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        // data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["styles", "p", "--applied"])
        .output()
        .expect("ff-rdp styles --applied failed");

    assert!(
        out.status.success(),
        "styles applied failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("styles applied output is not valid JSON");

    let results = json["results"].as_array().expect("results is not an array");
    assert!(
        !results.is_empty(),
        "fixture must produce at least one applied rule for <p>; got empty results"
    );

    let mut seen: HashSet<&str> = HashSet::new();
    for entry in results {
        if let Some(actor_id) = entry.get("rule_actor_id").and_then(|v| v.as_str()) {
            if actor_id.is_empty() {
                continue;
            }
            assert!(
                seen.insert(actor_id),
                "Theme E regression: duplicate rule_actor_id '{actor_id}' in styles applied output"
            );
        }
    }

    eprintln!(
        "live_styles_applied_dedupe_no_duplicate_actor_ids: PASS — {} result(s), {} unique actor id(s)",
        results.len(),
        seen.len()
    );
}
