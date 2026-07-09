//! e2e coverage for `ff-rdp manifest` that needs no live Firefox.
//!
//! The wire path (target frame → manifest actor → fetchCanonicalManifest) and
//! the parsed-manifest / no-manifest result shapes are covered by the live
//! suite `tests/live/live_104_security_pwa.rs`. Here we assert the help surface
//! and the standard error envelope for an unresolvable tab.

use super::support::{MockRdpServer, load_fixture};
use serde_json::Value;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "1000".to_owned(),
        "--no-daemon".to_owned(),
    ]
}

fn parse_stdout_json(stdout: &[u8]) -> Option<Value> {
    let s = std::str::from_utf8(stdout).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    serde_json::from_str(s).ok()
}

/// `e2e_manifest_no_tab_error_shape`:
///
/// `manifest` against an unknown tab id produces the standard structured error
/// envelope (an `error_type` field) and a non-zero exit code — never a
/// partial/success result. The mock server answers `listTabs`, so the failure
/// is the unresolved `--tab-id`, exercised entirely before any manifest RPC.
#[test]
fn manifest_unknown_tab_produces_error_envelope() {
    // Answer listTabs so the client reaches tab resolution; the unknown
    // --tab-id then fails deterministically ("tab not found") before any
    // manifest RPC — exactly the standard-error-envelope path we assert.
    let server = MockRdpServer::new().on("listTabs", load_fixture("list_tabs_response.json"));
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("--tab-id".to_owned());
    args.push("does-not-exist-actor".to_owned());
    args.push("manifest".to_owned());

    let out = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("run ff-rdp manifest");

    let _ = handle.join();

    assert!(
        !out.status.success(),
        "manifest against an unknown tab must exit non-zero; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let json = parse_stdout_json(&out.stdout)
        .unwrap_or_else(|| panic!("stdout not JSON: {}", String::from_utf8_lossy(&out.stdout)));
    assert!(
        json.get("error_type").is_some(),
        "error envelope must carry an error_type field: {json}"
    );
    assert!(
        json.get("error").and_then(Value::as_str).is_some(),
        "error envelope must carry a human-readable error field: {json}"
    );
}

/// `manifest --help` documents the command, the no-manifest semantics, and the
/// output shape.
#[test]
fn manifest_help_documents_command() {
    let out = std::process::Command::new(ff_rdp_bin())
        .args(["manifest", "--help"])
        .output()
        .expect("manifest --help");
    assert!(out.status.success(), "manifest --help must exit 0");
    let help = String::from_utf8_lossy(&out.stdout);
    for needle in ["fetchCanonicalManifest", "manifest: null", "errors"] {
        assert!(
            help.contains(needle),
            "manifest --help must mention {needle:?}:\n{help}"
        );
    }
}
