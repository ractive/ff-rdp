//! Guard against the discipline failure that let iteration 129 ship broken
//! (iteration 137 Theme D).
//!
//! Every feature iteration 129 delivered — `click --frame`, the cross-origin
//! frame scan, `consent accept` — worked only with `--no-daemon`. In the
//! default connection mode frame enumeration returned zero targets, so all
//! three silently degraded. It went green because **every** iter-129 live test
//! passed `--no-daemon`: the tests and the iteration's own `dogfood_path`
//! disagreed, and the tests won.
//!
//! `--no-daemon` is a legitimate thing to test — daemon lifecycle suites need
//! a direct connection, and some protocol interactions genuinely cannot be
//! proxied. What is *not* legitimate is a live suite that only ever exercises
//! the flag users don't pass. So: a live suite that uses `--no-daemon` must
//! say, in one module-level line, where its daemon-mode coverage lives or why
//! direct-only is the correct scope.
//!
//! ```text
//! //! daemon-parity: live_137_consent_accept_via_daemon covers the proxied path
//! //! daemon-parity: daemon lifecycle assertions require a direct connection
//! ```
//!
//! [`GRANDFATHERED`] freezes the suites that predate the rule. The guard fails
//! if that list grows, if an entry stops needing the exemption, or if a suite
//! outside it uses `--no-daemon` without the annotation — so the list can only
//! shrink.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The marker a live suite uses to declare its daemon-mode coverage.
const ANNOTATION: &str = "//! daemon-parity:";

/// The flag whose unaccompanied use this guard exists to catch.
const NO_DAEMON: &str = "--no-daemon";

/// Live suites that used `--no-daemon` before iteration 137 introduced the
/// rule, and have not yet been annotated.
///
/// **This list may only shrink.** Adding an entry means shipping another
/// direct-only live suite, which is the exact hole iteration 137 was opened to
/// close. To remove an entry: add the `daemon-parity:` module line (with a
/// daemon-mode test to point at, or a reason the suite must be direct-only).
const GRANDFATHERED: &[&str] = &[
    "live_100_daemon_lifecycle_hardening.rs",
    "live_103_emulate.rs",
    "live_109_throttle_block.rs",
    "live_111_daemon_follow_cross_process.rs",
    "live_128_network_output_fidelity.rs",
    "live_131_measurement_honesty.rs",
    "live_61l.rs",
    "live_61q_resource_bus.rs",
    "live_62_page_map_index.rs",
    "live_94_polish_bundle.rs",
    "live_console_printf.rs",
    "live_cross_actor.rs",
    "live_daemon_stop_mdn.rs",
    "live_oneway.rs",
    "live_target_destroyed.rs",
];

fn live_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/live")
}

/// Every `tests/live/*.rs` suite file, by file name.
fn live_suites() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(live_dir()).expect("tests/live must be readable");
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name")
            .to_owned();
        if name == "main.rs" {
            // The harness root declares `mod`s; it runs no commands itself.
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("live suite must be readable");
        out.push((name, body));
    }
    out.sort();
    assert!(
        !out.is_empty(),
        "no live suites found under {} — the guard would pass vacuously",
        live_dir().display()
    );
    out
}

/// AC: `unit_no_daemon_live_test_guard` — a live suite that drives the CLI
/// with `--no-daemon` must declare its daemon-mode coverage via a
/// `//! daemon-parity:` module line, unless it is grandfathered.
#[test]
fn unit_no_daemon_live_test_guard() {
    let mut offenders = Vec::new();
    let grandfathered: BTreeSet<&str> = GRANDFATHERED.iter().copied().collect();

    for (name, body) in live_suites() {
        if !body.contains(NO_DAEMON) {
            continue;
        }
        if body.contains(ANNOTATION) {
            continue;
        }
        if grandfathered.contains(name.as_str()) {
            continue;
        }
        offenders.push(name);
    }

    assert!(
        offenders.is_empty(),
        "these live suites use `{NO_DAEMON}` without declaring daemon-mode coverage: {offenders:?}\n\
         Add a module-level line such as\n\
         \x20   {ANNOTATION} live_137_<name>_via_daemon covers the proxied path\n\
         naming the daemon-mode test, or stating why a direct connection is required.\n\
         Every iteration-129 feature shipped broken in daemon mode because its live \
         tests only ever ran with {NO_DAEMON}."
    );
}

/// AC: `unit_no_daemon_grandfather_list_only_shrinks` — an entry stops being
/// exempt the moment it no longer needs the exemption, so the list cannot rot
/// into a permanent allowlist.
#[test]
fn unit_no_daemon_grandfather_list_only_shrinks() {
    let suites = live_suites();
    let mut stale = Vec::new();

    for name in GRANDFATHERED {
        let Some((_, body)) = suites.iter().find(|(n, _)| n == name) else {
            stale.push(format!("{name} (file no longer exists)"));
            continue;
        };
        if !body.contains(NO_DAEMON) {
            stale.push(format!("{name} (no longer uses {NO_DAEMON})"));
        } else if body.contains(ANNOTATION) {
            stale.push(format!("{name} (now carries a {ANNOTATION} line)"));
        }
    }

    assert!(
        stale.is_empty(),
        "remove these entries from GRANDFATHERED in {} — they no longer need the exemption: {stale:?}",
        file!()
    );
}

/// AC: `unit_no_daemon_guard_detects_a_violation` — the guard's own matching
/// logic is exercised against synthetic content, so a refactor that makes it
/// silently match nothing fails here rather than passing vacuously.
#[test]
fn unit_no_daemon_guard_detects_a_violation() {
    let violating = "fn t() { cmd.args([\"--no-daemon\", \"page-text\"]); }";
    let annotated = format!("{ANNOTATION} live_137_x covers it\n{violating}");
    let clean = "fn t() { cmd.args([\"page-text\"]); }";

    assert!(violating.contains(NO_DAEMON) && !violating.contains(ANNOTATION));
    assert!(annotated.contains(NO_DAEMON) && annotated.contains(ANNOTATION));
    assert!(!clean.contains(NO_DAEMON));
}
