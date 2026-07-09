/// Live test for Theme E (iter-84): `styles applied` deduplicates rules that
/// share the same `rule_actor_id` so that inherited rules from parent elements
/// are not listed multiple times.
///
/// AC: live_styles_applied_dedupe — `results` contains no duplicate actor IDs
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::collections::HashSet;
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme E: `styles applied` does not emit duplicate rules when the same CSS
/// rule applies via multiple inheritance paths (e.g. `*` selector on a page
/// with deep DOM nesting).
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: `results` array has no two entries with the same non-empty
/// `rule_actor_id`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_styles_applied_dedupe_no_duplicate_actor_ids() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.com/"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(["styles", "applied", "--selector", "p"])
        .output()
        .expect("ff-rdp styles applied failed");

    assert!(
        out.status.success(),
        "styles applied failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("styles applied output is not valid JSON");

    let results = json["results"].as_array().expect("results is not an array");

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
}
