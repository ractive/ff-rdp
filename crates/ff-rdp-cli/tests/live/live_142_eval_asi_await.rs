//! Live test for iter-142 Theme E: ASI-separated top-level-await scripts.
//!
//! AC: `e2e_eval_asi_await_script` (live half — the mock-server e2e test in
//! `tests/e2e/eval.rs` can prove the CLI plumbing works but cannot prove the
//! generated wrapper is valid JS, since the mock returns a canned fixture
//! regardless of the script sent. This test runs the exact dogfooding
//! session 63 repro against real Firefox, where an actual `SyntaxError`
//! would surface as a genuine eval failure.)
//!
//! `await Promise.resolve(1)\n42` has no `;` anywhere — pre-iter-142 this
//! was misclassified as a single expression and wrapped as
//! `return (\nawait Promise.resolve(1)\n42\n)`, itself invalid JS
//! (`missing ) in parenthetical`). Post-fix it must both parse and honor
//! the trailing expression's value (`42`), not silently return `undefined`.
//!
//! Runs on the default daemon-mode connection path (no direct-connection
//! flag anywhere in this suite), per the iteration's run guidance.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_142_eval_asi_await -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled};

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_142_eval_asi_await_script() {
    if !live_tests_enabled() {
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();

    let out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "eval",
            "await Promise.resolve(1)\n42",
        ])
        .output()
        .expect("live_142_eval_asi_await_script: eval spawn failed");

    assert!(
        out.status.success(),
        "live_142_eval_asi_await_script: FAIL — ASI-separated await script must \
         not produce a SyntaxError — stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("live_142_eval_asi_await_script: eval JSON parse");

    assert_eq!(
        json["results"], 42,
        "live_142_eval_asi_await_script: the trailing expression's value must be \
         honored, not silently dropped as undefined — got: {json}"
    );

    eprintln!("live_142_eval_asi_await_script: PASS — results=42, no SyntaxError");
}
