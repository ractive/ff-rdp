//! Live test: `eval --format text` prints the bare value without changing what
//! runs. A plain Object/Array prints as its JSON value (read through the
//! object actors), every other result prints as in JSON mode, and the script
//! is sent exactly as in JSON mode — so `--no-isolate` scope sharing and
//! statement completion values keep working, and results `JSON.stringify`
//! cannot represent (`document.body`, `10n`, `window`) still print.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli \
//!       --test live live_eval_text_values -- --include-ignored

use std::process::{Command, Output};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, output_note};

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

/// Run `ff-rdp <args>`, assert exit 0 and no `-> ff-rdp` hint line, return stdout.
fn text_ok(port: u16, args: &[&str]) -> String {
    let out = run(port, args);
    assert!(out.status.success(), "{args:?}: {}", output_note(&out));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !stdout.contains("-> ff-rdp"),
        "{args:?} must print no hint line; got: {stdout:?}"
    );
    stdout
}

fn json_ok(port: u16, args: &[&str]) -> serde_json::Value {
    let out = run(port, args);
    assert!(out.status.success(), "{args:?}: {}", output_note(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("{args:?} stdout is not JSON: {e}\n{stdout}"))
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_text_values() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_text_values: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let nav = run(
        port,
        &[
            "--allow-unsafe-urls",
            "navigate",
            "data:text/html,<title>T</title><p>x</p>",
        ],
    );
    assert!(nav.status.success(), "navigate: {}", output_note(&nav));

    // A plain object prints as its JSON value, keys in insertion order.
    let out = text_ok(
        port,
        &["eval", "--format", "text", "({z: 1, a: [1, {b: 'x'}]})"],
    );
    let value: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("object text output is not JSON: {e}\n{out}"));
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        r#"{"z":1,"a":[1,{"b":"x"}]}"#
    );

    assert_eq!(text_ok(port, &["eval", "--format", "text", "1+1"]), "2\n");

    // Statement completion value: the script is not re-wrapped.
    assert_eq!(
        text_ok(port, &["eval", "--format", "text", "if (1) { 2 }"]),
        "2\n"
    );

    // --no-isolate shares one scope across calls in text mode too.
    assert_eq!(
        text_ok(
            port,
            &[
                "eval",
                "--no-isolate",
                "--format",
                "text",
                "let k402 = 7; k402"
            ]
        ),
        "7\n"
    );
    assert_eq!(
        text_ok(port, &["eval", "--no-isolate", "--format", "text", "k402"]),
        "7\n"
    );

    // Results JSON.stringify cannot represent keep their grip output.
    let body = text_ok(port, &["eval", "--format", "text", "document.body"]);
    assert!(body.contains("HTMLBodyElement"), "document.body: {body}");
    let big = text_ok(port, &["eval", "--format", "text", "10n"]);
    assert!(big.contains("10"), "10n: {big}");
    let win = text_ok(port, &["eval", "--format", "text", "window"]);
    assert!(win.contains("Window"), "window: {win}");

    // --jq filters the JSON envelope (the grip), as in JSON mode.
    let jq = text_ok(
        port,
        &[
            "eval",
            "--format",
            "text",
            "--jq",
            ".results.class",
            "({a: 1})",
        ],
    );
    assert!(jq.contains("Object"), "--format text --jq: {jq}");

    // JSON mode: a plain object grip names --stringify; --no-hints and a
    // DOM node do not.
    let grip = json_ok(port, &["eval", "({a: [1, 2]})"]);
    assert!(
        grip["hint"]
            .as_str()
            .is_some_and(|h| h.contains("--stringify")),
        "object grip hint: {grip}"
    );
    let quiet = json_ok(port, &["--no-hints", "eval", "({a: [1, 2]})"]);
    assert!(quiet.get("hint").is_none(), "--no-hints: {quiet}");
    let node = json_ok(port, &["eval", "document.body"]);
    assert!(node.get("hint").is_none(), "DOM node: {node}");
}
