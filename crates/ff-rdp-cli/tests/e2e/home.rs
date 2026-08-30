//! E2E tests for the bare `ff-rdp` home view (iter-212 Theme A).
//!
//! The unit tests in `commands/home.rs` cover the assembly and rendering; what
//! only a spawned process can prove is the part this iteration actually
//! changed — the **exit code** and the fact that bare `ff-rdp` no longer prints
//! clap's usage dump.

use std::process::Command;

use super::support;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// A port nothing is listening on, so the browser probe is guaranteed to fail
/// fast rather than pick up whatever Firefox the developer left running.
fn dark_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    port
}

fn run(extra: &[&str]) -> std::process::Output {
    Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &dark_port().to_string(),
            "--no-daemon",
            "--timeout",
            "1500",
        ])
        .args(extra)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn ff-rdp")
}

/// AC `home_without_browser_exits_zero_and_names_launch`.
///
/// This is the behaviour change the whole iteration turns on: before it, bare
/// `ff-rdp` exited 2 with clap's usage. A missing browser is state, not an
/// error.
#[test]
fn e2e_212_home_without_browser_exits_zero_and_names_launch() {
    let out = run(&[]);
    assert!(
        out.status.success(),
        "bare `ff-rdp` must exit 0 even with no browser: {}",
        support::output_note(&out)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("browser: not reachable"),
        "the view must say plainly that nothing is up: {text}"
    );
    assert!(
        text.contains("-> ff-rdp launch"),
        "and name the command that fixes it: {text}"
    );
    assert!(
        !text.contains("Usage:"),
        "the clap usage dump must be gone: {text}"
    );
}

/// The JSON form the session hook and scripts consume. `--jq` must reach the
/// same payload, including the `hints` array — the home view is the one
/// command that carries hints in JSON.
#[test]
fn e2e_212_home_json_carries_state_and_hints() {
    let out = run(&["--format", "json"]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("--format json must produce JSON");
    let results = &json["results"];
    assert_eq!(results["browser"]["reachable"], serde_json::json!(false));
    assert_eq!(results["page"], serde_json::Value::Null);
    assert!(results["tabs"].as_array().expect("tabs array").is_empty());
    assert!(
        results["bin"].as_str().is_some_and(|b| !b.is_empty()),
        "the view must name the binary that produced it: {results}"
    );
    let hints = results["hints"].as_array().expect("hints array");
    assert!(!hints.is_empty(), "{results}");
    assert!(hints.len() <= 5, "at most five hints: {hints:?}");

    let jq = run(&["--jq", ".results.hints | length"]);
    assert!(jq.status.success(), "{}", support::output_note(&jq));
    assert_eq!(
        String::from_utf8_lossy(&jq.stdout).trim(),
        hints.len().to_string(),
        "--jq must reach the same payload"
    );
}

/// `ff-rdp --help` is the reference and is explicitly out of scope for this
/// iteration: it must still render clap's help and exit 0, and it must not
/// advertise the hidden `home` / `skill-doc` entry points.
#[test]
fn e2e_212_help_is_unchanged_by_the_home_view() {
    let out = Command::new(ff_rdp_bin())
        .arg("--help")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", support::output_note(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Usage:"), "{text}");
    for hidden in ["  home", "  skill-doc"] {
        assert!(
            !text.contains(hidden),
            "{hidden} must stay hidden from --help: {text}"
        );
    }
    assert!(
        text.contains("install-hook"),
        "install-hook is a real command and must be listed: {text}"
    );
}

/// A real subcommand that is missing its own sub-subcommand must still error —
/// the bare-invocation rewrite keys off "no subcommand token at all", so this
/// pins that it cannot swallow other usage errors.
#[test]
fn e2e_212_a_subcommand_missing_its_own_subcommand_still_errors() {
    let out = Command::new(ff_rdp_bin())
        .arg("scroll")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "`ff-rdp scroll` must not be rewritten into the home view: {}",
        support::output_note(&out)
    );
}

/// The hidden named form the session hook calls. It must work, and `--hook`
/// must be accepted (the trimming it does needs a live page to be visible).
#[test]
fn e2e_212_named_home_and_hook_flag_both_run() {
    for args in [vec!["home"], vec!["home", "--hook"]] {
        let out = run(&args);
        assert!(
            out.status.success(),
            "`ff-rdp {}` must exit 0: {}",
            args.join(" "),
            support::output_note(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("browser: not reachable"),
            "`ff-rdp {}` must render the same view",
            args.join(" ")
        );
    }
}

/// `skill-doc` is what `cargo run -p xtask -- check-skill-drift` shells out to;
/// if it stops emitting the marked region, that gate silently stops checking
/// anything.
#[test]
fn e2e_212_skill_doc_emits_the_marked_region() {
    let out = Command::new(ff_rdp_bin())
        .arg("skill-doc")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", support::output_note(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("<!-- ff-rdp:generated:begin -->"),
        "{text}"
    );
    assert!(
        text.trim_end().ends_with("<!-- ff-rdp:generated:end -->"),
        "{text}"
    );
    assert!(text.contains("## Command groups"), "{text}");
}
