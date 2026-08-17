//! iter-172 Theme C (harness half) — a live test that cannot say *why* the
//! daemon did not start sends the next reader hunting.
//!
//! iteration-171's live sweep produced exactly one failure:
//!
//! ```text
//! ---- live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect stdout ----
//!   live_160_ref_click_asserts_handler_effect: the proxy daemon did not start
//!   for Firefox on port 63690
//! ```
//!
//! That message is all the evidence there was. `ff-rdp` itself knew more:
//! autostart records why it degraded in `meta.daemon_fallback` and reports
//! `meta.route == "direct"`. `LiveFirefox::with_daemon` simply threw it away by
//! returning a bare `Option`, so the sweep could not distinguish "the registry
//! read failed" (iteration-172's product defect) from "Firefox never came up"
//! (iteration-173's classification defect) from anything else.
//!
//! [`daemon_route_note`] is the extraction that keeps it. These tests pin its
//! contract against literal envelopes, so it is verifiable with no Firefox and
//! no daemon anywhere in sight — Firefox-free and ungated, so it runs on every
//! `cargo test`.

use serde_json::json;

#[path = "common/mod.rs"]
mod common;

use common::daemon_route_note;

/// The case that motivated this: autostart gave up and recorded why. The note
/// must carry the reason verbatim, because that string is the only thing that
/// distinguishes the zero-byte-registry defect from every other cause.
#[test]
fn unit_172_route_note_carries_the_daemon_fallback_reason() {
    let envelope = json!({
        "meta": {
            "route": "direct",
            "daemon_fallback": "warning: daemon started but did not register within 20s \
                                (registry write raced or was slow): reading daemon registry \
                                while waiting: parsing registry at \
                                /Users/x/.ff-rdp/daemon.53497.json: EOF while parsing a value \
                                at line 1 column 0 — connecting directly"
        }
    });
    let note = daemon_route_note(envelope.to_string().as_bytes());

    assert!(note.contains("route=direct"), "{note}");
    assert!(
        note.contains("EOF while parsing a value at line 1 column 0"),
        "the recorded fallback reason must survive into the note: {note}"
    );
}

/// The happy envelope: nothing degraded, so there is nothing to explain.
#[test]
fn unit_172_route_note_reports_a_clean_daemon_route() {
    let note = daemon_route_note(json!({"meta": {"route": "daemon"}}).to_string().as_bytes());
    assert!(note.contains("route=daemon"), "{note}");
    assert!(note.contains("no meta.daemon_fallback recorded"), "{note}");
}

/// A `direct` route with no recorded reason is itself a finding — it says the
/// downgrade happened somewhere that never called `remember_daemon_fallback`.
/// The note must not silently imply a reason exists.
#[test]
fn unit_172_route_note_flags_a_direct_route_with_no_recorded_reason() {
    let note = daemon_route_note(json!({"meta": {"route": "direct"}}).to_string().as_bytes());
    assert!(note.contains("route=direct"), "{note}");
    assert!(note.contains("no meta.daemon_fallback recorded"), "{note}");
}

/// Degenerate inputs must produce a usable sentence rather than panicking —
/// this runs on the failure path of a test that is already failing.
#[test]
fn unit_172_route_note_tolerates_non_json_and_missing_meta() {
    let note = daemon_route_note(b"error: could not connect\n");
    assert!(note.contains("no JSON envelope"), "{note}");

    let note = daemon_route_note(json!({"results": {}}).to_string().as_bytes());
    assert!(note.contains("(no meta.route)"), "{note}");
}
