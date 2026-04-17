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

fn inspect_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on(
            "prototypeAndProperties",
            load_fixture("prototype_and_properties_response.json"),
        )
}

#[test]
fn inspect_shows_properties() {
    let server = inspect_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "inspect".to_owned(),
        "server1.conn0.child2/obj19".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    // The results should contain ownProperties with keys "a" and "b".
    let props = &json["results"]["ownProperties"];
    assert_eq!(props["a"]["value"], 1);
    assert_eq!(props["b"]["value"]["type"], "object");
    assert_eq!(props["b"]["value"]["class"], "Array");

    // Prototype should be present.
    assert_eq!(json["results"]["prototype"]["type"], "object");
    assert_eq!(json["results"]["prototype"]["class"], "Object");
}

#[test]
fn inspect_with_jq_filter() {
    let server = inspect_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "inspect".to_owned(),
        "server1.conn0.child2/obj19".to_owned(),
        "--jq".to_owned(),
        ".results.ownProperties.a.value".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "1");
}
