/// Live test for Theme J (iter-84): `a11y contrast` detects failures on a
/// low-contrast fixture page — originally the WAI bad-example page
/// (https://www.w3.org/WAI/demos/bad/before/), the canonical reference for
/// low-contrast text, now reproduced locally as a `data:` URL (iter-114
/// Theme B) so the test no longer depends on real network access.
///
/// The fix widens element detection to include containers where all children
/// are inline elements (span, a, b, etc.) — not just leaf text nodes.
///
/// iter-127 also uses these fixtures to pin the `--fail-only` count contract:
/// the top-level `total` counts returned failures (not the sampled element
/// count) and a distinct `sampled` field carries the examined-element count.
///
/// AC: live_a11y_contrast_low_contrast_fixture — aa_fail ≥ 1 on the local
///     low-contrast fixture page
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use std::process::Command;

/// A local low-contrast fixture: several elements combining light-gray text
/// on a white background (ratio well under the WCAG AA 4.5:1 threshold for
/// normal text), spanning leaf elements, an all-inline-children container,
/// and a `<td>` (the WAI bad-demo pattern this AC guards against
/// regressing). Mirrors the shape of real low-contrast violations without
/// depending on the external WAI page staying reachable or unchanged.
///
/// Hex colors use `%23` in place of `#` — an unescaped `#` in a `data:` URL
/// is parsed as a fragment delimiter, truncating everything after it before
/// the page ever loads.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head></head><body>\
<p id=\"p1\" style=\"color:%23cccccc;background:%23ffffff\">low contrast paragraph</p>\
<div id=\"d1\" style=\"color:%23d9d9d9;background:%23ffffff\">\
<span>low contrast</span> <b>inline children</b></div>\
<table><tr><td id=\"td1\" style=\"color:%23e0e0e0;background:%23ffffff\">\
low contrast cell</td></tr></table>\
</body></html>";

/// An all-passing fixture: black text on white (ratio 21:1, well above the AA
/// 4.5:1 threshold) across the same element shapes as `FIXTURE_HTML`, so
/// `--fail-only` returns zero failures while still sampling ≥ 1 element.
const FIXTURE_HTML_ALL_PASS: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head></head><body>\
<p id=\"p1\" style=\"color:%23000000;background:%23ffffff\">high contrast paragraph</p>\
<div id=\"d1\" style=\"color:%23000000;background:%23ffffff\">\
<span>high contrast</span> <b>inline children</b></div>\
</body></html>";

/// Theme J: `a11y contrast` with `--fail-only` returns at least one failure
/// on a fixture page that deliberately contains low-contrast text across
/// leaf, inline-children-container, and table-cell elements.
///
/// Post-condition: `summary.aa_fail` ≥ 1.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_a11y_contrast_low_contrast_fixture_detects_failures() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_a11y_contrast_low_contrast_fixture_detects_failures: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_a11y_contrast_low_contrast_fixture_detects_failures: Firefox not available — skipping"
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
        .args(["a11y", "contrast", "--fail-only"])
        .output()
        .expect("ff-rdp a11y contrast failed");

    assert!(
        out.status.success(),
        "a11y contrast failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("a11y contrast output is not valid JSON");

    let aa_fail = json
        .pointer("/meta/summary/aa_fail")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    assert!(
        aa_fail >= 1,
        "Theme J regression: low-contrast fixture reported 0 AA failures \
         (contrast detection too narrow — may have missed inline-child containers)"
    );

    let total = json["total"].as_u64().unwrap_or(0);
    assert!(
        total >= 1,
        "Theme J: --fail-only returned 0 results (aa_fail={aa_fail})"
    );

    eprintln!("live_a11y_contrast_low_contrast_fixture_detects_failures: PASS — aa_fail={aa_fail}");
}

/// Navigate to `url` then run `a11y contrast` with `extra_args`, returning the
/// parsed JSON envelope. Panics (fails the test) on navigate/command failure.
fn contrast_json(port: u16, url: &str, extra_args: &[&str]) -> serde_json::Value {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        // data: URLs require --allow-unsafe-urls.
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let mut args = base_args(port);
    args.extend(["a11y".to_owned(), "contrast".to_owned()]);
    args.extend(extra_args.iter().map(ToString::to_string));

    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp a11y contrast failed");
    assert!(
        out.status.success(),
        "a11y contrast failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    serde_json::from_slice(&out.stdout).expect("a11y contrast output is not valid JSON")
}

/// iter-127 AC live_a11y_contrast_fail_only_total_zero: on an all-passing page,
/// `a11y contrast --fail-only` yields `total == 0`, `results == []`, and
/// `sampled >= 1` — the sample size no longer masquerades as the failure count.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_a11y_contrast_fail_only_total_zero() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_a11y_contrast_fail_only_total_zero: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_a11y_contrast_fail_only_total_zero: Firefox not available — skipping");
        return;
    };

    let json = contrast_json(ff.port(), FIXTURE_HTML_ALL_PASS, &["--fail-only", "--all"]);

    let total = json["total"].as_u64().unwrap_or(u64::MAX);
    let results_len = json["results"].as_array().map_or(usize::MAX, Vec::len);
    let sampled = json["sampled"].as_u64().unwrap_or(0);

    assert_eq!(
        total, 0,
        "all-passing page: --fail-only total must be 0, not the sample size (got total={total}, sampled={sampled})"
    );
    assert_eq!(
        results_len, 0,
        "all-passing page: results must be empty under --fail-only"
    );
    assert!(
        sampled >= 1,
        "sampled must report at least one examined element (got {sampled})"
    );

    eprintln!("live_a11y_contrast_fail_only_total_zero: PASS — total=0 sampled={sampled}");
}

/// iter-127 AC live_a11y_contrast_fail_only_total_counts_failures: on a page
/// with known AA failures, `--fail-only --all` yields
/// `total == (.results | length)` and `sampled >= total`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_a11y_contrast_fail_only_total_counts_failures() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_a11y_contrast_fail_only_total_counts_failures: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_a11y_contrast_fail_only_total_counts_failures: Firefox not available — skipping"
        );
        return;
    };

    let json = contrast_json(ff.port(), FIXTURE_HTML, &["--fail-only", "--all"]);

    let total = json["total"].as_u64().expect("total must be a number");
    let results_len = json["results"]
        .as_array()
        .map(Vec::len)
        .expect("results must be an array") as u64;
    let sampled = json["sampled"].as_u64().expect("sampled must be a number");

    assert!(
        total >= 1,
        "failing fixture must report at least one failure"
    );
    assert_eq!(
        total, results_len,
        "--fail-only --all: total must equal the number of returned results"
    );
    assert!(
        sampled >= total,
        "sampled ({sampled}) must be >= the failure count ({total})"
    );

    eprintln!(
        "live_a11y_contrast_fail_only_total_counts_failures: PASS — total={total} sampled={sampled}"
    );
}

/// iter-127 AC live_a11y_contrast_limit_keeps_total: `--fail-only --limit 1` on
/// the failing page still reports the full failure count in `total` (with
/// `truncated == true`), not 1 — the limit truncates `results`, never `total`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a live Firefox instance"]
fn live_a11y_contrast_limit_keeps_total() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_a11y_contrast_limit_keeps_total: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_a11y_contrast_limit_keeps_total: Firefox not available — skipping");
        return;
    };

    // First learn the full failure count with --all.
    let full = contrast_json(ff.port(), FIXTURE_HTML, &["--fail-only", "--all"]);
    let full_total = full["total"].as_u64().expect("total must be a number");
    assert!(
        full_total >= 2,
        "fixture must have >= 2 failures for the limit test to be meaningful (got {full_total})"
    );

    // Now cap the results at 1 — total must still report the full failure count.
    let limited = contrast_json(ff.port(), FIXTURE_HTML, &["--fail-only", "--limit", "1"]);
    let limited_total = limited["total"].as_u64().expect("total must be a number");
    let shown = limited["results"]
        .as_array()
        .map(Vec::len)
        .expect("results must be an array");

    assert_eq!(
        limited_total, full_total,
        "--limit 1 must not shrink total: full={full_total}, limited={limited_total}"
    );
    assert_eq!(shown, 1, "--limit 1 must return exactly one result row");
    assert_eq!(
        limited["truncated"],
        serde_json::Value::Bool(true),
        "limited output must be flagged truncated"
    );

    eprintln!(
        "live_a11y_contrast_limit_keeps_total: PASS — total={limited_total} shown={shown} (truncated)"
    );
}
