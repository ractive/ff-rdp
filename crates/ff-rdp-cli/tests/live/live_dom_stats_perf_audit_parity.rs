/// Live test for Theme H (iter-84): `dom stats` and `perf audit` report the
/// same value for `images_without_lazy` — both should count only
/// out-of-viewport images that lack `loading=lazy`.
///
/// Serves a local fixture page (rather than httpbin.org/html, which has no
/// `<img>` tags and made the parity check nearly vacuous) containing several
/// `<img>` elements without `loading="lazy"`, placed below a tall spacer so
/// they are provably out of the viewport — see `crates/ff-rdp-cli/src/commands/dom.rs`
/// (`build_stats_js`) and `crates/ff-rdp-cli/src/commands/perf.rs` for the
/// shared "out of viewport AND lacks loading=lazy" rule.
///
/// AC: live_dom_stats_perf_parity — dom_stats.images_without_lazy ==
///     perf_audit.images_without_lazy on a local out-of-viewport-images fixture,
///     and the shared value is non-zero.
use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::Command;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
};

/// Number of `<img>` tags placed below the spacer (out of viewport, no
/// `loading="lazy"`), so `images_without_lazy` is deterministically this count.
const IMG_COUNT: usize = 4;

/// Fixture HTML: a tall spacer (well beyond any reasonable viewport height)
/// followed by several `<img>` elements without `loading="lazy"`. The images
/// use tiny inline `data:` sources — `dom stats`/`perf audit` count markup via
/// `getBoundingClientRect`, not fetched image bytes, so the src need not
/// resolve to a real image.
fn fixture_body() -> String {
    let mut imgs = String::new();
    for i in 0..IMG_COUNT {
        let _ = writeln!(
            imgs,
            "<img id=\"img{i}\" src=\"data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBTAA7\" width=\"10\" height=\"10\">"
        );
    }
    format!(
        "<!DOCTYPE html><html><head></head><body>\
         <div style=\"height:4000px\">spacer</div>\
         {imgs}\
         </body></html>"
    )
}

/// Start a fixture server serving the out-of-viewport-images fixture at `/`.
fn spawn_html_server(body: String) -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(body));
    FixtureServer::start(routes)
}

/// Theme H: `dom stats` and `perf audit` agree on `images_without_lazy`.
///
/// Self-launches headless Firefox on a random port and navigates to a local
/// fixture with several out-of-viewport `<img>` elements lacking
/// `loading="lazy"`.
/// Post-condition: both commands return equal, non-zero `images_without_lazy` values.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_stats_perf_audit_parity_images_without_lazy() {
    if !live_tests_enabled() {
        eprintln!(
            "live_dom_stats_perf_audit_parity_images_without_lazy: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_dom_stats_perf_audit_parity_images_without_lazy: Firefox not available — skipping"
        );
        return;
    };

    let Some(server) = spawn_html_server(fixture_body()) else {
        eprintln!(
            "live_dom_stats_perf_audit_parity_images_without_lazy: could not bind HTTP server — skipping"
        );
        return;
    };
    let url = server.base_url();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let dom_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["dom", "stats"])
        .output()
        .expect("ff-rdp dom stats failed");
    assert!(
        dom_out.status.success(),
        "dom stats failed: {}",
        String::from_utf8_lossy(&dom_out.stderr)
    );

    let perf_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit failed");
    assert!(
        perf_out.status.success(),
        "perf audit failed: {}",
        String::from_utf8_lossy(&perf_out.stderr)
    );

    let dom_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dom_out.stdout))
            .expect("dom stats is not valid JSON");
    let perf_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&perf_out.stdout))
            .expect("perf audit is not valid JSON");

    // `dom stats` returns `images_without_lazy` directly under `results`;
    // `perf audit` nests the same DOM-stats block under `results.dom_stats`.
    let dom_val = dom_json
        .pointer("/results/images_without_lazy")
        .or_else(|| dom_json.pointer("/results/0/images_without_lazy"))
        .and_then(serde_json::Value::as_u64);
    let perf_val = perf_json
        .pointer("/results/dom_stats/images_without_lazy")
        .or_else(|| perf_json.pointer("/results/images_without_lazy"))
        .or_else(|| perf_json.pointer("/results/0/images_without_lazy"))
        .and_then(serde_json::Value::as_u64);

    let dom = dom_val.unwrap_or_else(|| {
        panic!("dom stats output missing images_without_lazy: {dom_json}");
    });
    let perf = perf_val.unwrap_or_else(|| {
        panic!("perf audit output missing images_without_lazy: {perf_json}");
    });

    assert_eq!(
        dom, perf,
        "Theme H regression: dom stats ({dom}) != perf audit ({perf}) for images_without_lazy"
    );
    // The fixture places IMG_COUNT out-of-viewport images without loading="lazy",
    // so the shared value must be deterministically non-zero — this makes the
    // parity assertion meaningful rather than vacuous (httpbin.org/html had 0 images).
    assert!(
        dom > 0,
        "fixture places {IMG_COUNT} out-of-viewport <img> tags without loading=lazy; \
         images_without_lazy must be non-zero, got {dom}"
    );
}
