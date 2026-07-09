//! iter-95 Theme B ACs:
//!   - `live_cascade_inherited_or_default_note_fires_on_h1_color`
//!   - `pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does`
//!     (live variant — three properties on a small fixture)
//!
//! Exercises the fixed `external_computed` path in `cascade.rs`: verifies that
//! `cascade h1 --prop color` and `computed h1 --prop color` agree byte-for-byte
//! on a known fixture, and that neither returns a null/empty computed value.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_95_cascade_computed_agreement -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// A small HTML page with known computed values that Firefox resolves
/// deterministically regardless of UA stylesheet differences.
///
/// - `color` on `h1` is overridden inline to `red` → always `rgb(255, 0, 0)`.
/// - `background-color` on `body` is overridden to `blue` → `rgb(0, 0, 255)`.
/// - `font-size` on `p` is set to `12px` → `12px`.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head>\
<style>h1{color:red}body{background-color:blue}p{font-size:12px}</style>\
</head><body><h1>heading</h1><p>paragraph</p></body></html>";

/// `live_cascade_inherited_or_default_note_fires_on_h1_color`
///
/// Navigate to a fixture where `h1 { color: red }` is declared.
/// Run both `cascade h1 --prop color` and `computed h1 --prop color`.
/// Assert:
///   - Both commands report a non-empty computed value.
///   - Both computed values agree byte-for-byte.
///
/// This exercises the fixed `fetch_computed_value` / `build_external_computed_js`
/// path that previously swallowed LongString and non-String Grip values, causing
/// cascade to return `{"computed": null}` even though computed returned a value.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cascade_inherited_or_default_note_fires_on_h1_color() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_cascade_inherited_or_default_note_fires_on_h1_color: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_cascade_inherited_or_default_note_fires_on_h1_color: \
             Firefox not available — skipping"
        );
        return;
    };

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        // iter-110 Theme B(a): data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cascade_inherited_or_default_note_fires_on_h1_color: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cascade h1 --prop color.
    let cascade_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp cascade");
    assert!(
        cascade_out.status.success(),
        "live_cascade_inherited_or_default_note_fires_on_h1_color: cascade failed — {}",
        String::from_utf8_lossy(&cascade_out.stderr)
    );

    let cascade_stdout = String::from_utf8_lossy(&cascade_out.stdout);
    let cascade_json: serde_json::Value = serde_json::from_str(cascade_stdout.trim())
        .unwrap_or_else(|e| {
            panic!(
                "live_cascade_inherited_or_default_note_fires_on_h1_color: \
                 cascade output is not valid JSON: {e}\nstdout={cascade_stdout}\nstderr={}",
                String::from_utf8_lossy(&cascade_out.stderr)
            )
        });

    let cascade_computed = cascade_json["results"][0]["computed"]
        .as_str()
        .unwrap_or("");
    assert!(
        !cascade_computed.is_empty(),
        "live_cascade_inherited_or_default_note_fires_on_h1_color: \
         cascade computed must be non-empty; got null/empty. \
         This was the iter-95 bug: external_computed silently returned None. \
         full entry: {}",
        cascade_json["results"][0]
    );

    // Run computed h1 --prop color.
    let computed_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["computed", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp computed");
    assert!(
        computed_out.status.success(),
        "live_cascade_inherited_or_default_note_fires_on_h1_color: computed failed — {}",
        String::from_utf8_lossy(&computed_out.stderr)
    );

    let computed_stdout = String::from_utf8_lossy(&computed_out.stdout);
    let computed_json: serde_json::Value = serde_json::from_str(computed_stdout.trim())
        .unwrap_or_else(|e| {
            panic!(
                "live_cascade_inherited_or_default_note_fires_on_h1_color: \
                 computed output is not valid JSON: {e}\nstdout={computed_stdout}"
            )
        });

    // computed returns [{selector, index, computed: {color: "…"}}]
    let standalone_computed = computed_json["results"][0]["computed"]["color"]
        .as_str()
        .unwrap_or("");
    assert!(
        !standalone_computed.is_empty(),
        "live_cascade_inherited_or_default_note_fires_on_h1_color: \
         standalone computed must be non-empty; got: {}",
        computed_json["results"][0]
    );

    // The key assertion: both commands must agree byte-for-byte.
    assert_eq!(
        cascade_computed, standalone_computed,
        "live_cascade_inherited_or_default_note_fires_on_h1_color: \
         cascade computed={cascade_computed:?} must match standalone computed={standalone_computed:?}"
    );

    eprintln!(
        "live_cascade_inherited_or_default_note_fires_on_h1_color: PASS — computed={cascade_computed:?}"
    );
}

/// `pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does` (live)
///
/// Table-driven: three (selector, prop) pairs on a known fixture.
/// Verifies that `cascade --prop` returns a non-empty `computed` field AND
/// agrees with the standalone `computed` command for each pair.
///
/// Rows:
///   - (`h1`,  `color`)            — explicit author rule
///   - (`body`, `background-color`) — explicit author rule
///   - (`p`,   `font-size`)        — explicit author rule
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does: \
             Firefox not available — skipping"
        );
        return;
    };

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        // iter-110 Theme B(a): data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "pre_fix_repro: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let cases = [
        ("h1", "color"),
        ("body", "background-color"),
        ("p", "font-size"),
    ];

    for (selector, prop) in cases {
        // cascade <selector> --prop <prop>
        let cascade_out = Command::new(ff_rdp_bin())
            .args(base_args(ff.port()))
            .args(["cascade", selector, "--prop", prop])
            .output()
            .expect("ff-rdp cascade");
        assert!(
            cascade_out.status.success(),
            "pre_fix_repro ({selector}, {prop}): cascade failed — {}",
            String::from_utf8_lossy(&cascade_out.stderr)
        );
        let cascade_stdout = String::from_utf8_lossy(&cascade_out.stdout);
        let cascade_json: serde_json::Value = serde_json::from_str(cascade_stdout.trim())
            .unwrap_or_else(|e| {
                panic!(
                    "pre_fix_repro ({selector}, {prop}): cascade JSON parse error: {e}\n\
                     stdout={cascade_stdout}"
                )
            });
        let cascade_computed = cascade_json["results"][0]["computed"]
            .as_str()
            .unwrap_or("");
        assert!(
            !cascade_computed.is_empty(),
            "pre_fix_repro ({selector}, {prop}): cascade computed must be non-empty; \
             got null/empty — this would have failed before the iter-95 fix. \
             entry: {}",
            cascade_json["results"][0]
        );

        // computed <selector> --prop <prop>
        let computed_out = Command::new(ff_rdp_bin())
            .args(base_args(ff.port()))
            .args(["computed", selector, "--prop", prop])
            .output()
            .expect("ff-rdp computed");
        assert!(
            computed_out.status.success(),
            "pre_fix_repro ({selector}, {prop}): computed failed — {}",
            String::from_utf8_lossy(&computed_out.stderr)
        );
        let computed_stdout = String::from_utf8_lossy(&computed_out.stdout);
        let computed_json: serde_json::Value = serde_json::from_str(computed_stdout.trim())
            .unwrap_or_else(|e| {
                panic!(
                    "pre_fix_repro ({selector}, {prop}): computed JSON parse error: {e}\n\
                     stdout={computed_stdout}"
                )
            });
        let standalone_computed = computed_json["results"][0]["computed"][prop]
            .as_str()
            .unwrap_or("");
        assert!(
            !standalone_computed.is_empty(),
            "pre_fix_repro ({selector}, {prop}): standalone computed must be non-empty; \
             got: {}",
            computed_json["results"][0]
        );

        assert_eq!(
            cascade_computed, standalone_computed,
            "pre_fix_repro ({selector}, {prop}): cascade computed={cascade_computed:?} \
             must equal standalone computed={standalone_computed:?}"
        );

        eprintln!("pre_fix_repro ({selector}, {prop}): PASS — computed={cascade_computed:?}");
    }
}
