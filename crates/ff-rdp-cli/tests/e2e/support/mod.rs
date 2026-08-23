pub mod mock_server;

pub use mock_server::MockRdpServer;

/// Load a fixture JSON file from `tests/fixtures/` relative to the crate root.
///
/// Panics if the file cannot be read or parsed — fixture failures should be
/// loud and immediate.
pub fn load_fixture(name: &str) -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);

    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"));

    serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"))
}

/// Render a subprocess's exit status and *both* output streams for a failure
/// message.
///
/// `ff-rdp` is a JSON-on-stdout tool: its error envelopes go to **stdout**,
/// not stderr, so an e2e assertion that reports only `stderr` on failure
/// ships a message with nothing after the colon — stdout, the stream
/// actually carrying the diagnostic, is silently dropped. iteration-179 fixed
/// this for the live tier (`crate::common::output_note`); this is the e2e
/// tier's mirror, kept as its own copy because the tiers do not share a
/// module. `crates/ff-rdp-cli/tests/iter_179_harness_stdout_evidence.rs`
/// fails the build if an e2e assertion goes back to naming only `stderr`.
///
/// Both streams are trimmed and included unconditionally — an empty one is
/// itself evidence (it says the tool wrote nothing there).
pub fn output_note(out: &std::process::Output) -> String {
    format!(
        "status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}
