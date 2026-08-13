//! Live tests for iter-132 — CLI polish: live DOM `value` field, top-level
//! `await` in `eval`.
//!
//! ACs (see kb/iterations/iteration-132-cli-polish.md):
//!   - live_132_dom_live_value: fixture input with attribute value "0"; after
//!     `eval '...value="42"'`, `dom '#el'` reports `value:"42"` AND
//!     `attrs.value:"0"`.
//!   - live_132_eval_top_level_await: `eval 'await Promise.resolve(41) + 1'`
//!     → results 42, exit 0, on all three input paths (arg, --file, --stdin).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_132_cli_polish -- --nocapture

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

fn navigate(port: u16, url: &str) {
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate to {url} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

/// AC `live_132_dom_live_value`: an `<input id="el" value="0">` starts with
/// the static HTML attribute "0". After `eval` sets the live `.value`
/// property to "42", `dom '#el'` must report BOTH the live `value: "42"`
/// field and the still-static `attrs.value: "0"` — proving the two are
/// tracked independently (iter-132 Theme B).
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_132_dom_live_value() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_132_dom_live_value: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    let url = "data:text/html,<title>live-value</title><input id=\"el\" value=\"0\">";
    navigate(ff.port(), url);

    // Sanity: before the live edit, both the attribute and the live
    // property read "0".
    let before = run_json(ff.port(), &["dom", "#el"]);
    let before_entry = &before["results"][0];
    assert_eq!(
        before_entry["value"], "0",
        "live value must start at the HTML default, got: {before}"
    );
    assert_eq!(
        before_entry["attrs"]["value"], "0",
        "attrs.value must start at the HTML default, got: {before}"
    );

    // Set the live DOM property without touching the HTML attribute.
    let set = run_json(
        ff.port(),
        &["eval", "document.querySelector('#el').value = '42'"],
    );
    assert_eq!(set["results"], "42", "eval set-value failed: {set}");

    let after = run_json(ff.port(), &["dom", "#el"]);
    let after_entry = &after["results"][0];
    assert_eq!(
        after_entry["value"], "42",
        "live `value` field must reflect the .value property write, got: {after}"
    );
    assert_eq!(
        after_entry["attrs"]["value"], "0",
        "attrs.value (static HTML attribute) must stay unchanged, got: {after}"
    );
}

/// AC `live_132_eval_top_level_await`: `await Promise.resolve(41) + 1`
/// resolves to 42 on all three `eval` input paths — positional arg,
/// `--file`, and `--stdin` (iter-132 Theme C).
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_132_eval_top_level_await() {
    const SCRIPT: &str = "await Promise.resolve(41) + 1";

    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_132_eval_top_level_await: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    navigate(ff.port(), "data:text/html,<title>top-level-await</title>");

    // Positional arg.
    let arg_result = run_json(ff.port(), &["eval", SCRIPT]);
    assert_eq!(
        arg_result["results"], 42,
        "positional-arg await script must resolve to 42, got: {arg_result}"
    );

    // --file
    let tmp = std::env::temp_dir().join(format!("ff_rdp_live_132_await_{}.js", std::process::id()));
    std::fs::write(&tmp, SCRIPT).expect("write temp script file");
    let file_result = run_json(
        ff.port(),
        &["eval", "--file", tmp.to_str().expect("utf8 tmp path")],
    );
    let _ = std::fs::remove_file(&tmp);
    assert_eq!(
        file_result["results"], 42,
        "--file await script must resolve to 42, got: {file_result}"
    );

    // --stdin
    let mut child = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["eval", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff-rdp eval --stdin");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(SCRIPT.as_bytes())
        .expect("write script to stdin");
    let out = child.wait_with_output().expect("wait for eval --stdin");
    assert!(
        out.status.success(),
        "--stdin await script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stdin_result: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("--stdin output not JSON: {e}\n{stdout}"));
    assert_eq!(
        stdin_result["results"], 42,
        "--stdin await script must resolve to 42, got: {stdin_result}"
    );
}
