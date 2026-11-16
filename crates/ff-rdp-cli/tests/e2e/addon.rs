//! `ff-rdp addon install|list|reload|uninstall` against the mock RDP server,
//! replaying replies recorded from Firefox 160 by
//! `live_298_record_addon_commands` (`crates/ff-rdp-core/tests/live_record_fixtures.rs`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

use super::support::{self, MockRdpServer, load_fixture};

const FIXTURE_ID: &str = "temporary-addon-fixture@ff-rdp.test";

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Run `ff-rdp --host 127.0.0.1 --port <port> <args>` against `server` and
/// return the output plus every request the server read.
fn run(server: MockRdpServer, args: &[&str]) -> (Output, Vec<Value>) {
    let port = server.port();
    let log = server.request_log();
    let handle = std::thread::spawn(move || server.serve_one());
    let output = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(args)
        .output()
        .expect("spawn ff-rdp");
    handle.join().expect("mock server thread");
    let requests = log.lock().expect("request log").clone();
    (output, requests)
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("stdout is not JSON: {}", support::output_note(output)))
}

/// The recorded `listAddons` reply with the fixture add-on's `file:` source
/// URL pointing at this checkout's fixture directory (the recording stores it
/// as `<FIXTURES>`).
fn list_addons_here() -> Value {
    let mut reply = load_fixture("list_addons_response.json");
    let dir = fixture_dir("temporary_addon_mv3").canonicalize().unwrap();
    let url = url::Url::from_directory_path(&dir).unwrap();
    for addon in reply["addons"].as_array_mut().unwrap() {
        if addon["id"] == FIXTURE_ID {
            addon["url"] = json!(url.as_str());
        }
    }
    reply
}

fn requests_of<'a>(requests: &'a [Value], method: &str) -> Vec<&'a Value> {
    requests.iter().filter(|r| r["type"] == method).collect()
}

#[test]
fn e2e_298_addon_install_sends_absolute_path_and_reports_install() {
    let server = MockRdpServer::new()
        .on("getRoot", load_fixture("get_root_addons_response.json"))
        .on(
            "installTemporaryAddon",
            load_fixture("install_temporary_addon_response.json"),
        );
    let dir = fixture_dir("temporary_addon_mv3");
    let (output, requests) = run(server, &["addon", "install", dir.to_str().unwrap()]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json = stdout_json(&output);
    assert_eq!(json["total"], 1, "{json}");
    assert_eq!(json["results"]["id"], FIXTURE_ID, "{json}");
    assert_eq!(json["results"]["actor"], false, "{json}");
    let sent = requests_of(&requests, "installTemporaryAddon");
    assert_eq!(sent.len(), 1, "{requests:?}");
    let path = sent[0]["addonPath"].as_str().unwrap();
    assert!(Path::new(path).is_absolute(), "{path}");
    assert!(!path.starts_with(r"\\?\"), "{path}");
    assert_eq!(json["results"]["path"], path, "{json}");
    assert_eq!(sent[0]["openDevTools"], false);
}

#[test]
fn e2e_298_addon_install_rejected_keeps_firefox_message() {
    let server = MockRdpServer::new()
        .on("getRoot", load_fixture("get_root_addons_response.json"))
        .on(
            "installTemporaryAddon",
            load_fixture("install_temporary_addon_error_response.json"),
        );
    let dir = fixture_dir("temporary_addon_bad_manifest");
    let (output, _) = run(server, &["addon", "install", dir.to_str().unwrap()]);
    assert!(!output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["error_type"], "User", "{json}");
    let message = json["error"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("addon install ")
            && message.contains("Firefox rejected the temporary install")
            && message.contains("Property \"version\" is required"),
        "{json}"
    );
}

/// A missing path, or a directory without manifest.json, is a `User` error
/// before Firefox is contacted: nothing listens on the port, so reaching it
/// would have been a `Connection` error instead.
#[test]
fn e2e_298_addon_path_checks_precede_connecting() {
    let tmp = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port();
    let missing = tmp.path().join("missing-extension");
    let cases = [
        ("install", missing.clone(), "cannot read path"),
        ("install", tmp.path().to_path_buf(), "no manifest.json"),
        ("reload", missing, "cannot read path"),
    ];
    for (verb, path, needle) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["addon", verb])
            .arg(&path)
            .output()
            .expect("spawn ff-rdp");
        assert!(!output.status.success(), "{verb} {path:?}");
        let json = stdout_json(&output);
        assert_eq!(json["error_type"], "User", "{verb} {path:?}: {json}");
        let message = json["error"].as_str().unwrap_or_default();
        assert!(
            message.starts_with(&format!("addon {verb} ")) && message.contains(needle),
            "{json}"
        );
    }
}

#[test]
fn e2e_298_addon_list_defaults_to_about_debugging_view() {
    let server = MockRdpServer::new().on("listAddons", list_addons_here());
    let (output, requests) = run(server, &["addon", "list"]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json = stdout_json(&output);
    let results = json["results"].as_array().unwrap();
    assert!(
        results
            .iter()
            .all(|a| a["debuggable"] == true && a["hidden"] == false),
        "{json}"
    );
    let fixture = results
        .iter()
        .find(|a| a["id"] == FIXTURE_ID)
        .unwrap_or_else(|| panic!("fixture add-on missing: {json}"));
    assert_eq!(fixture["temporarily_installed"], true, "{fixture}");
    assert_eq!(fixture["name"], "ff-rdp temporary add-on fixture");
    assert!(fixture["uuid"].as_str().is_some_and(|u| u.len() == 36));
    assert!(fixture["url"].as_str().unwrap().starts_with("file:///"));
    assert!(
        fixture["actor"]
            .as_str()
            .unwrap()
            .contains("webExtensionDescriptor")
    );
    assert_eq!(json["total"], results.len());
    // No icon fetch requested.
    let sent = requests_of(&requests, "listAddons");
    assert_eq!(sent.len(), 1);
    assert!(sent[0].get("iconDataURL").is_none(), "{:?}", sent[0]);
}

#[test]
fn e2e_298_addon_list_include_hidden_and_jq() {
    let recorded = load_fixture("list_addons_response.json");
    let all = recorded["addons"].as_array().unwrap().len();
    let server = MockRdpServer::new().on("listAddons", recorded);
    let (output, _) = run(
        server,
        &[
            "addon",
            "list",
            "--include-hidden",
            "--jq",
            "[.total, ([.results[] | select(.system == true)] | length)]",
        ],
    );
    assert!(output.status.success(), "{}", support::output_note(&output));
    let counts = stdout_json(&output);
    assert_eq!(counts[0], all, "{counts}");
    assert!(
        counts[1].as_u64().unwrap() > 0,
        "system add-ons listed: {counts}"
    );
}

/// `reload <id>` and `reload <path>` both send the descriptor's `reload` to
/// the fixture add-on's descriptor actor and report its UUID before and after.
#[test]
fn e2e_298_addon_reload_by_id_and_by_path() {
    let dir = fixture_dir("temporary_addon_mv3");
    for target in [FIXTURE_ID, dir.to_str().unwrap()] {
        let listed = list_addons_here();
        let descriptor = listed["addons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == FIXTURE_ID)
            .unwrap()["actor"]
            .clone();
        let server = MockRdpServer::new()
            .on("listAddons", listed)
            .on("reload", load_fixture("webextension_reload_response.json"));
        let (output, requests) = run(server, &["addon", "reload", target]);
        assert!(
            output.status.success(),
            "{target}: {}",
            support::output_note(&output)
        );
        let json = stdout_json(&output);
        assert_eq!(json["results"]["id"], FIXTURE_ID, "{json}");
        assert_eq!(json["results"]["temporarily_installed"], true, "{json}");
        assert!(json["results"]["uuid"].is_string(), "{json}");
        assert_eq!(
            json["results"]["uuid"], json["results"]["previous_uuid"],
            "{json}"
        );
        let sent = requests_of(&requests, "reload");
        assert_eq!(sent.len(), 1, "{target}: {requests:?}");
        assert_eq!(sent[0]["to"], descriptor, "{target}");
        assert_eq!(requests_of(&requests, "listAddons").len(), 2, "{target}");
    }
}

#[test]
fn e2e_298_addon_reload_unknown_id_and_uninstalled_path() {
    let other = fixture_dir("temporary_addon_bad_manifest");
    for (target, needle) in [
        ("no-such-addon@ff-rdp.test", "no add-on with this id"),
        (other.to_str().unwrap(), "no add-on is installed from"),
    ] {
        let server = MockRdpServer::new().on("listAddons", list_addons_here());
        let (output, requests) = run(server, &["addon", "reload", target]);
        assert!(!output.status.success(), "{target}");
        let json = stdout_json(&output);
        assert_eq!(json["error_type"], "User", "{json}");
        assert!(
            json["error"].as_str().unwrap_or_default().contains(needle),
            "{json}"
        );
        assert!(requests_of(&requests, "reload").is_empty(), "{requests:?}");
    }
}

#[test]
fn e2e_298_addon_uninstall_reports_success() {
    let server = MockRdpServer::new()
        .on("getRoot", load_fixture("get_root_addons_response.json"))
        .on(
            "uninstallAddon",
            load_fixture("uninstall_addon_response.json"),
        );
    let (output, requests) = run(server, &["addon", "uninstall", FIXTURE_ID]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    let json = stdout_json(&output);
    assert_eq!(
        json["results"],
        json!({"id": FIXTURE_ID, "uninstalled": true}),
        "{json}"
    );
    let sent = requests_of(&requests, "uninstallAddon");
    assert_eq!(sent[0]["addonId"], FIXTURE_ID);
}

/// Firefox's refusal is the same for an unknown id and a regular add-on; the
/// error says which, from a follow-up `listAddons`.
#[test]
fn e2e_298_addon_uninstall_refusals_say_why() {
    for (id, needle) in [
        ("no-such-addon@ff-rdp.test", "no add-on with this id"),
        ("default-theme@mozilla.org", "not temporarily installed"),
    ] {
        let server = MockRdpServer::new()
            .on("getRoot", load_fixture("get_root_addons_response.json"))
            .on(
                "uninstallAddon",
                load_fixture("uninstall_addon_unknown_id_error_response.json"),
            )
            .on("listAddons", load_fixture("list_addons_response.json"));
        let (output, _) = run(server, &["addon", "uninstall", id]);
        assert!(!output.status.success(), "{id}");
        let json = stdout_json(&output);
        assert_eq!(json["error_type"], "User", "{json}");
        let message = json["error"].as_str().unwrap_or_default();
        assert!(
            message.starts_with(&format!("addon uninstall {id}: ")) && message.contains(needle),
            "{json}"
        );
    }
}
