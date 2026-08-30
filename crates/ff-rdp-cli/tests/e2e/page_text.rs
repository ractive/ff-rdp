use super::support::{self, MockRdpServer, load_fixture};

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

fn page_text_server(eval_result_fixture: &str) -> MockRdpServer {
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
fn page_text_returns_visible_text() {
    let server = page_text_server("eval_result_page_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("page-text".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert_eq!(
        json["results"],
        "Example Domain\nThis domain is for use in illustrative examples in documents."
    );
    assert_eq!(json["total"], 1);
}

#[test]
fn page_text_long_string_is_fetched() {
    let server = MockRdpServer::new()
        .on("listTabs", load_fixture("list_tabs_response.json"))
        .on("getTarget", load_fixture("get_target_response.json"))
        .on_with_followup(
            "evaluateJSAsync",
            load_fixture("eval_immediate_response.json"),
            load_fixture("eval_result_page_text_long.json"),
        )
        .on(
            "substring",
            load_fixture("substring_page_text_response.json"),
        );
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("page-text".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    let text = json["results"]
        .as_str()
        .expect("results should be a string");
    assert!(
        text.contains("Example Domain"),
        "should contain full text: {text}"
    );
}

#[test]
fn page_text_with_jq_filter() {
    let server = page_text_server("eval_result_page_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.extend([
        "page-text".to_owned(),
        "--jq".to_owned(),
        ".results | length".to_owned(),
    ]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let len: usize = stdout.trim().parse().expect("should be a number");
    assert!(len > 0, "text length should be > 0");
}

// ── iter-211: `--query` and the default `--max-chars` cap ──────────────────

/// Run `page-text` with `extra` flags against the standard two-line fixture
/// and return the parsed envelope.
fn page_text_json(extra: &[&str]) -> serde_json::Value {
    let server = page_text_server("eval_result_page_text.json");
    let port = server.port();
    let handle = std::thread::spawn(move || server.serve_one());

    let mut args = base_args(port);
    args.push("page-text".to_owned());
    args.extend(extra.iter().map(|s| (*s).to_owned()));

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp");

    handle.join().unwrap();

    assert!(
        output.status.success(),
        "expected success for {extra:?}, stderr: {}",
        support::output_note(&output)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON")
}

/// The size accounting is always present, even when nothing was cut — an
/// explicit `truncated: false` reads as "no, nothing was removed", where an
/// absent key is indistinguishable from "unknown" (iter-128's convention).
#[test]
fn iter_211_page_text_reports_size_even_when_untruncated() {
    let json = page_text_json(&[]);
    let text = json["results"].as_str().expect("results is a string");
    assert_eq!(
        json["meta"]["total_chars"],
        serde_json::json!(text.chars().count()),
        "total_chars must match the returned text when nothing was cut: {json}"
    );
    assert_eq!(json["meta"]["truncated"], false, "{json}");
    assert_eq!(json["meta"]["max_chars"], 8000, "the default cap: {json}");
    assert!(
        json.get("hint").is_none(),
        "an untruncated response must not carry a truncation hint: {json}"
    );
}

/// `--query` narrows `results` to the matching line and reports the counts a
/// caller needs to tell "no matches" from "filtered to nothing".
#[test]
fn iter_211_page_text_query_narrows_results_and_reports_counts() {
    let json = page_text_json(&["--query", "illustrative", "--context", "0"]);
    assert_eq!(
        json["results"], "This domain is for use in illustrative examples in documents.",
        "only the matching line survives --context 0: {json}"
    );
    assert_eq!(json["meta"]["matches"], 1, "{json}");
    assert_eq!(json["meta"]["shown"], 1, "{json}");
    assert_eq!(json["meta"]["context_lines"], 0, "{json}");
    assert_eq!(
        json["meta"]["match_lines"],
        serde_json::json!([2]),
        "the match is on line 2: {json}"
    );
}

/// Matching is case-insensitive by default, and a query nothing matches
/// returns an empty excerpt with `matches: 0` — never the whole page.
#[test]
fn iter_211_page_text_query_is_case_insensitive_and_honest_about_zero() {
    let hit = page_text_json(&["--query", "ILLUSTRATIVE", "--context", "0"]);
    assert_eq!(hit["meta"]["matches"], 1, "{hit}");
    assert_eq!(hit["meta"]["match_lines"], serde_json::json!([2]), "{hit}");

    let miss = page_text_json(&["--query", "no-such-token-211"]);
    assert_eq!(miss["meta"]["matches"], 0, "{miss}");
    assert_eq!(miss["results"], "", "{miss}");
    assert!(
        miss["meta"]["total_chars"].as_u64().unwrap_or(0) > 0,
        "total_chars still reports the page that was searched: {miss}"
    );
}

/// `--max-chars 0` is a usage-time refusal, not a silently empty result.
#[test]
fn iter_211_page_text_rejects_a_zero_cap() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(base_args(1))
        .args(["page-text", "--max-chars", "0"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(
        !output.status.success(),
        "--max-chars 0 must fail: {}",
        support::output_note(&output)
    );
    // ff-rdp writes its error envelope to STDOUT (iter-179), so that is where
    // the message lives — stderr is empty on this path.
    let envelope = String::from_utf8_lossy(&output.stdout);
    assert!(
        envelope.contains("--max-chars"),
        "the error must name the flag: {}",
        support::output_note(&output)
    );
    assert!(
        envelope.contains("--full"),
        "the error must name the escape hatch: {}",
        support::output_note(&output)
    );
}

/// An unparseable `--query-regex` is a clap usage error (exit 2), rejected
/// before any connection is attempted.
#[test]
fn iter_211_page_text_invalid_query_regex_exits_2() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(base_args(1))
        .args(["page-text", "--query-regex", "([unclosed"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert_eq!(
        output.status.code(),
        Some(2),
        "usage error: {}",
        support::output_note(&output)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid regular expression"),
        "the message must say what is wrong: {}",
        support::output_note(&output)
    );
}
