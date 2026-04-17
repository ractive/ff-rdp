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

fn sources_server() -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", load_fixture("thread_attach_response.json"))
        .on("sources", load_fixture("sources_response.json"))
        .on("resume", load_fixture("thread_resume_response.json"))
        .on("detach", load_fixture("thread_detach_response.json"))
}

#[test]
fn sources_lists_all_scripts() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("sources".to_owned());

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

    assert_eq!(json["total"], 3);
    let results = json["results"].as_array().expect("results is array");
    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[1]["url"], "https://example.com/vendor.min.js");
    assert_eq!(results[2]["url"], "https://cdn.example.com/analytics.js");
    assert!(results[2]["isBlackBoxed"].as_bool().unwrap());
}

#[test]
fn sources_filter_by_substring() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--filter".to_owned(),
        "vendor".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://example.com/vendor.min.js");
}

#[test]
fn sources_filter_by_pattern() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--pattern".to_owned(),
        r"cdn\.example\.com".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://cdn.example.com/analytics.js");
}

#[test]
fn sources_with_jq_filter() {
    let server = sources_server();
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "sources".to_owned(),
        "--jq".to_owned(),
        ".results[].url".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], r#""https://example.com/app.js""#);
}

#[test]
fn sources_handles_null_entries_in_response() {
    // Verify that null entries in the sources array are silently skipped.
    let response_with_nulls = serde_json::json!({
        "from": "server1.conn0.child2/thread1",
        "sources": [
            {
                "actor": "server1.conn0.child2/source42",
                "url": "https://example.com/app.js",
                "isBlackBoxed": false
            },
            null,
            {
                "actor": "server1.conn0.child2/source43",
                "url": "https://example.com/lib.js",
                "isBlackBoxed": false
            }
        ]
    });

    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on("attach", load_fixture("thread_attach_response.json"))
        .on("sources", response_with_nulls)
        .on("resume", load_fixture("thread_resume_response.json"))
        .on("detach", load_fixture("thread_detach_response.json"));

    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("sources".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 2);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["url"], "https://example.com/app.js");
    assert_eq!(results[1]["url"], "https://example.com/lib.js");
}
