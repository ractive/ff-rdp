//! Live test: `eval --frame <url-substring>` evaluates inside the matching
//! iframe, resolved on the command's own connection.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli \
//!       --test live live_eval_scope -- --include-ignored

use std::process::{Command, Output};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, output_note};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim())
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\n{}", output_note(output)))
}

/// Navigate to a page with an srcdoc iframe, then `eval --frame srcdoc
/// document.title` must answer from the iframe, not the top document; an
/// unmatched substring is a usage error naming the available frames.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_in_frame() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_in_frame: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let data_url = "data:text/html,<title>OuterFrame</title><iframe srcdoc='<title>InnerFrame</title><p>inner</p>'></iframe>";

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .arg("--allow-unsafe-urls")
        .args(["navigate", data_url])
        .output()
        .expect("navigate to data URL with srcdoc iframe");
    assert!(nav.status.success(), "navigate: {}", output_note(&nav));

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["eval", "--frame", "srcdoc", "document.title"])
        .output()
        .expect("eval --frame srcdoc");
    assert!(out.status.success(), "eval --frame: {}", output_note(&out));
    let json = parse_json(&out);
    assert_eq!(
        json["results"], "InnerFrame",
        "eval --frame must run in the iframe: {json}"
    );

    let miss = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["eval", "--frame", "no-such-frame", "document.title"])
        .output()
        .expect("eval --frame no-such-frame");
    assert!(!miss.status.success(), "unmatched --frame must fail");
    let note = output_note(&miss);
    assert!(note.contains("matched no frame"), "{note}");
}
