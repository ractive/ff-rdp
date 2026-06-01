//! iter-92 Theme B — navigate freshness gate + run/index parity live tests.
//!
//! Pre-fix repro: `ff-rdp navigate <url>` returned `elapsed_ms: 0,
//! ready_state: "complete"` on the second navigation to the same tab because
//! it observed the pre-existing dom-complete from the prior load (stale state).
//!
//! The fix: capture `performance.timing.navigationStart` before dispatching
//! `navigateTo` and reject any `readyState == complete` reading whose
//! `navigationStart` is not fresher than that pre-epoch value.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_92_navigate_epoch -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

const PAGE_A: &str = "data:text/html,A_PAGE";
const PAGE_B: &str = "data:text/html,B_PAGE";

fn parse_results(out: &std::process::Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&out.stdout);
    let top: serde_json::Value = serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    top["results"].clone()
}

/// `pre_fix_repro_navigate_second_call_waits_for_new_commit`:
///
/// Navigate to PAGE_A, then navigate to PAGE_B.  Assert:
/// - Second navigate exits 0.
/// - `elapsed_ms > 0` (did not short-circuit on stale dom-complete).
/// - `document.location.href` after the call returns a URL containing "B_PAGE".
///
/// Pre-fix: second navigate returned `elapsed_ms: 0, ready_state: "complete"`
/// by picking up the stale state from the PAGE_A load.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_navigate_second_call_waits_for_new_commit() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
             Firefox not available — skipping"
        );
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to PAGE_A first.
    let nav_a = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "--allow-unsafe-urls", PAGE_A])
        .output()
        .expect("navigate to PAGE_A");
    assert!(
        nav_a.status.success(),
        "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
         first navigate failed — {}",
        String::from_utf8_lossy(&nav_a.stderr)
    );

    // Navigate to PAGE_B (second call in same tab).
    let nav_b = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "--allow-unsafe-urls", PAGE_B])
        .output()
        .expect("navigate to PAGE_B");
    assert!(
        nav_b.status.success(),
        "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
         second navigate failed — {}",
        String::from_utf8_lossy(&nav_b.stderr)
    );

    let results = parse_results(&nav_b);

    // elapsed_ms must be > 0 — the stale-dom-complete short-circuit produced 0.
    let elapsed = results["elapsed_ms"].as_u64().unwrap_or(0);
    assert!(
        elapsed > 0,
        "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
         second navigate elapsed_ms={elapsed} (expected > 0); \
         stale dom-complete short-circuit may still be present"
    );

    // After the second navigate the URL must reflect PAGE_B.
    let eval_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "document.location.href"])
        .output()
        .expect("eval location");
    assert!(
        eval_out.status.success(),
        "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
         eval failed — {}",
        String::from_utf8_lossy(&eval_out.stderr)
    );

    let eval_s = String::from_utf8_lossy(&eval_out.stdout);
    assert!(
        eval_s.contains("B_PAGE"),
        "pre_fix_repro_navigate_second_call_waits_for_new_commit: \
         URL after second navigate should contain B_PAGE; got: {eval_s}"
    );
}

/// `live_run_navigate_parity`:
///
/// `ff-rdp run --url <url> -e "1"` must exit 0 for a URL where
/// `ff-rdp navigate <url>` also exits 0.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_run_navigate_parity() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_run_navigate_parity: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_run_navigate_parity: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());
    let url = PAGE_A;

    // Baseline: navigate must succeed.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("navigate");
    assert!(
        nav.status.success(),
        "live_run_navigate_parity: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // run must also succeed.  `ff-rdp run` consumes a script file: write a
    // two-step script (navigate + eval) and execute it.
    let script = format!(
        r#"{{"version":1,"steps":[{{"navigate":{{"url":"{url}"}}}},{{"eval":{{"script":"1"}}}}]}}"#
    );
    let script_path = std::env::temp_dir().join("ff-rdp-iter92-run.json");
    std::fs::write(&script_path, script).expect("write script");

    let run = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["run", "--allow-unsafe-urls", &script_path.to_string_lossy()])
        .output()
        .expect("run");
    assert!(
        run.status.success(),
        "live_run_navigate_parity: run failed — stderr={} stdout={}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
}

/// `live_index_navigate_parity`:
///
/// `ff-rdp index <url> --depth 0` must exit 0 for the same URL where
/// `ff-rdp navigate <url>` exits 0.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_index_navigate_parity() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_index_navigate_parity: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_index_navigate_parity: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());
    let url = PAGE_A;

    // Baseline: navigate must succeed.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("navigate");
    assert!(
        nav.status.success(),
        "live_index_navigate_parity: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // index --depth 0 must also succeed.
    let index = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["index", "--allow-unsafe-urls", url, "--depth", "0"])
        .output()
        .expect("index --depth 0");
    assert!(
        index.status.success(),
        "live_index_navigate_parity: index --depth 0 failed — {}",
        String::from_utf8_lossy(&index.stderr)
    );
}
