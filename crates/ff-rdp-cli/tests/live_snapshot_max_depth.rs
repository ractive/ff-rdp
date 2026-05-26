//! iter-82 AC: `snapshot_max_depth_truncates_tree`.
//!
//! Runs `ff-rdp snapshot --max-depth 2` on a nested fixture page (4 levels
//! deep) and asserts:
//!   - The returned tree has no nodes deeper than 2.
//!   - `meta.depth == 2`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_snapshot_max_depth -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

/// Four-level nested fixture: div > div > div > div > span.
/// With `--max-depth 2` we should see at most 2 levels of children.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head></head><body>\
<div id='l1'><div id='l2'><div id='l3'><div id='l4'><span>leaf</span></div></div></div></div>\
</body></html>";

/// Recursively compute the maximum depth of the snapshot tree.
/// `depth` starts at 0 for the root node's children.
fn max_tree_depth(node: &serde_json::Value, current_depth: usize) -> usize {
    let children = match node["children"].as_array() {
        Some(c) if !c.is_empty() => c,
        _ => return current_depth,
    };
    children
        .iter()
        .map(|child| max_tree_depth(child, current_depth + 1))
        .max()
        .unwrap_or(current_depth)
}

/// `snapshot_max_depth_truncates_tree`:
/// Navigate to a 4-level nested fixture page, run
/// `ff-rdp snapshot --max-depth 2`, and assert no node in the returned
/// tree is deeper than 2 levels from the root.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_snapshot_max_depth_truncates_tree() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("snapshot_max_depth_truncates_tree: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("snapshot_max_depth_truncates_tree: Firefox not available — skipping");
        return;
    };

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "snapshot_max_depth_truncates_tree: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run snapshot --max-depth 2.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["snapshot", "--max-depth", "2"])
        .output()
        .expect("ff-rdp snapshot --max-depth 2");
    assert!(
        out.status.success(),
        "snapshot_max_depth_truncates_tree: snapshot failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "snapshot_max_depth_truncates_tree: output is not valid JSON: {e}\n\
                 stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    // Find the tree root — it's under results or directly as a tree object.
    let tree = json.get("results").unwrap_or(&json);

    // Compute actual max depth of the returned tree.
    let actual_depth = max_tree_depth(tree, 0);

    assert!(
        actual_depth <= 2,
        "snapshot_max_depth_truncates_tree: tree depth {actual_depth} exceeds --max-depth 2; \
         first 200 chars of output: {}",
        stdout.chars().take(200).collect::<String>()
    );

    // meta.depth must reflect the limit passed via --max-depth.
    let meta_depth = json["meta"]["depth"].as_u64().unwrap_or(u64::MAX);
    assert_eq!(
        meta_depth, 2,
        "snapshot_max_depth_truncates_tree: meta.depth must be 2; got {meta_depth}"
    );

    eprintln!("snapshot_max_depth_truncates_tree: PASS — max depth in tree = {actual_depth}");
}
