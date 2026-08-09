//! iter-135 — `screenshotActor.capture` reply-shape parsing.
//!
//! Both fixtures are **recorded from a live Firefox 153.0.3** by
//! `live_record_fixtures::live_record_capture_screenshot{,_no_image_data}`;
//! neither is hand-written.
//!
//! Background: ff-rdp used to omit `snapshotScale` from the `capture` request
//! whenever it equalled `1.0`, believing Firefox defaulted it.  It does not —
//! `capture-screenshot.js` hands the value straight to `drawSnapshot`, so an
//! absent field yields a `NaN`-scaled canvas, `toDataURL` throws, and the reply
//! carries `data: null` with the failure explained in `messages`.  ff-rdp threw
//! `messages` away and reported "capture response missing 'data' field", which
//! read as protocol drift.

use ff_rdp_core::{CAPTURE_NO_IMAGE_DATA, parse_capture_response};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {}: {e}", path.display()))
}

/// `unit_screenshot_capture_parses_ff153_shape`
///
/// The Firefox 153 replies parse correctly in both directions:
///
/// - the success reply (`data` + `width`/`height` + empty `messages`) yields the
///   PNG data URL;
/// - the failure reply (`data: null` + a populated `messages`) yields an error
///   that quotes Firefox's own diagnostic instead of claiming the field is
///   missing.
#[test]
fn unit_screenshot_capture_parses_ff153_shape() {
    let ok = fixture("capture_screenshot_response.json");
    let data = parse_capture_response(&ok).expect("FF153 success reply must parse");
    assert!(
        data.starts_with("data:image/png;base64,"),
        "expected a PNG data URL, got: {data}"
    );

    let failed = fixture("capture_screenshot_no_image_data_response.json");
    assert!(
        failed["value"]["data"].is_null(),
        "fixture must be the recorded null-data reply"
    );
    let err = parse_capture_response(&failed).expect_err("null data must be an error");
    let rendered = err.to_string();

    assert!(
        rendered.contains(CAPTURE_NO_IMAGE_DATA),
        "error must carry the stable marker the CLI matches on: {rendered}"
    );
    assert!(
        !rendered.contains("missing 'data' field"),
        "the pre-iter-135 wording must be gone: {rendered}"
    );

    // Firefox localises the text, so assert it is forwarded verbatim rather
    // than matching on words.
    let server_text = failed["value"]["messages"][0]["text"]
        .as_str()
        .expect("recorded reply carries a diagnostic message");
    assert!(
        rendered.contains(server_text),
        "Firefox's own diagnostic must reach the user: {rendered}"
    );
    assert!(
        rendered.contains("[error]"),
        "the message level must be shown: {rendered}"
    );
}

/// `unit_screenshot_capture_parses_legacy_shape`
///
/// Firefox ≤ 152 replied with a bare `{"value":{"data":…,"filename":…}}` — no
/// `messages`, `width`, or `height` keys.  Parsing must not regress for those
/// builds, since ff-rdp supports a version range.
///
/// The legacy packet is derived from the recorded FF153 success reply by
/// deleting the keys FF153 added, so the data URL itself is still real
/// recorded output.
#[test]
fn unit_screenshot_capture_parses_legacy_shape() {
    let mut legacy = fixture("capture_screenshot_response.json");
    let value = legacy["value"]
        .as_object_mut()
        .expect("recorded reply has a value object");
    value.remove("messages");
    value.remove("width");
    value.remove("height");

    let data = parse_capture_response(&legacy).expect("pre-153 reply shape must still parse");
    assert!(
        data.starts_with("data:image/png;base64,"),
        "expected a PNG data URL, got: {data}"
    );
}

/// A reply with neither `data` nor `messages` must still produce an actionable
/// error rather than an empty one.
#[test]
fn unit_screenshot_capture_no_data_no_messages() {
    let bare = serde_json::json!({"from": "server1.conn0.screenshotActor7", "value": {}});
    let err = parse_capture_response(&bare).expect_err("empty value must be an error");
    let rendered = err.to_string();
    assert!(rendered.contains(CAPTURE_NO_IMAGE_DATA), "got: {rendered}");
    assert!(
        rendered.contains("no diagnostic messages"),
        "the absence of server messages must be stated: {rendered}"
    );
}

/// An empty-string `data` is as useless as a null one and must be rejected.
#[test]
fn unit_screenshot_capture_rejects_empty_data() {
    let empty = serde_json::json!({
        "from": "server1.conn0.screenshotActor7",
        "value": {"data": "", "messages": [{"level": "warn", "text": "downscaled"}]},
    });
    let err = parse_capture_response(&empty).expect_err("empty data must be an error");
    let rendered = err.to_string();
    assert!(rendered.contains(CAPTURE_NO_IMAGE_DATA), "got: {rendered}");
    assert!(
        rendered.contains("[warn] downscaled"),
        "non-error levels must be surfaced too: {rendered}"
    );
}
