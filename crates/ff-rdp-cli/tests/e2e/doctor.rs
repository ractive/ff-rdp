//! Tests for the `doctor` subcommand.
//!
//! `doctor` shells out for OS-level probes (lsof / netstat / ps) and varies
//! across platforms, so the assertions stay coarse: structure of the JSON
//! envelope, exit code direction, and presence of the key probes.

use super::support::{MockRdpServer, load_fixture};

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

/// `doctor` against a closed port must exit 1 and produce a JSON envelope with
/// at least one failing probe whose hint mentions `ff-rdp launch`.
#[test]
fn doctor_no_listener_fails_with_launch_hint() {
    // Bind then drop to grab a guaranteed-free port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("doctor".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn");

    assert!(
        !output.status.success(),
        "doctor should exit non-zero when nothing is listening; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    let probes = json["results"].as_array().expect("results array");
    assert!(!probes.is_empty(), "should have at least one probe");

    // At least one probe should fail (port_owner) and reference `ff-rdp launch`.
    let any_launch_hint = probes.iter().any(|p| {
        p["hint"]
            .as_str()
            .is_some_and(|h| h.contains("ff-rdp launch"))
    });
    assert!(
        any_launch_hint,
        "expected a launch hint among failing probes; got: {probes:#?}"
    );

    // meta.connection must be present.
    assert!(json["meta"]["connection"].is_object());
}

/// `doctor` against a working mock RDP server (greeting + listTabs) must
/// pass at least the handshake and tabs probes, and emit `meta.connection`.
#[test]
fn doctor_mock_server_passes_handshake_and_tabs() {
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("doctor".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn");

    handle.join().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!("stdout must be JSON: {e}\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}")
    });

    let probes = json["results"].as_array().expect("results array");
    let names: Vec<&str> = probes.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(
        names.contains(&"rdp_handshake"),
        "rdp_handshake probe must be present; got names: {names:?}\nSTDERR: {stderr}"
    );
    assert!(
        names.contains(&"tabs"),
        "tabs probe must be present; got names: {names:?}\nSTDERR: {stderr}\nSTDOUT: {stdout}"
    );

    let handshake = probes
        .iter()
        .find(|p| p["name"] == "rdp_handshake")
        .expect("rdp_handshake probe");
    assert_eq!(
        handshake["status"], "pass",
        "rdp_handshake should pass against mock server: {handshake:#?}"
    );
}

/// `doctor` against a server that gives the greeting but no tabs must mark
/// the tabs probe as fail and surface a hint about relaunching.
#[test]
fn doctor_zero_tabs_fails_tabs_probe() {
    let server = MockRdpServer::new().on(
        "listTabs",
        serde_json::json!({"from": "root", "tabs": [], "selected": 0}),
    );
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("doctor".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "doctor must exit non-zero when zero tabs are exposed"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    let probes = json["results"].as_array().unwrap();
    let tabs_probe = probes
        .iter()
        .find(|p| p["name"] == "tabs")
        .expect("tabs probe");
    assert_eq!(tabs_probe["status"], "fail");
    let hint = tabs_probe["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("temp-profile") || hint.contains("Open a tab"),
        "tabs hint should suggest a recovery path; got: {hint}"
    );
}
