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

fn computed_server(eval_result_fixture: &str) -> MockRdpServer {
    MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture(eval_result_fixture),
        )
}

#[test]
fn computed_single_match_returns_object() {
    let server = computed_server("eval_result_computed_single.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["computed".to_owned(), "h1".to_owned()]);

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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["total"], 1);
    assert_eq!(json["results"]["selector"], "h1");
    assert_eq!(json["results"]["index"], 0);
    assert_eq!(json["results"]["computed"]["color"], "rgb(10, 20, 30)");
    assert_eq!(json["results"]["computed"]["font-size"], "32px");
}

#[test]
fn computed_multi_match_returns_array() {
    let server = computed_server("eval_result_computed_multi.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["computed".to_owned(), ".card".to_owned()]);

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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["total"], 2);
    let arr = json["results"].as_array().expect("results is array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["index"], 0);
    assert_eq!(arr[1]["index"], 1);
    assert_eq!(arr[0]["computed"]["color"], "rgb(0, 0, 0)");
    assert_eq!(arr[1]["computed"]["color"], "rgb(255, 0, 0)");
}

#[test]
fn computed_prop_mode_single_returns_scalar() {
    let server = computed_server("eval_result_computed_prop.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "computed".to_owned(),
        "h1".to_owned(),
        "--prop".to_owned(),
        "color".to_owned(),
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

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"], "rgb(10, 20, 30)");
}

#[test]
fn computed_no_match_errors() {
    let server = computed_server("eval_result_computed_empty.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend(["computed".to_owned(), ".nonexistent".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit when no elements match"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no element") || stderr.contains(".nonexistent"),
        "expected a helpful error, got: {stderr}"
    );
}

#[test]
fn computed_with_jq_filter() {
    let server = computed_server("eval_result_computed_single.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "computed".to_owned(),
        "h1".to_owned(),
        "--jq".to_owned(),
        ".results.computed.color".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#""rgb(10, 20, 30)""#);
}

#[test]
fn computed_prop_and_all_conflict() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["computed", "h1", "--prop", "color", "--all"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        !output.status.success(),
        "expected clap to reject --prop + --all"
    );
}
