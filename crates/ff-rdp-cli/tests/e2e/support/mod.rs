#[allow(dead_code)]
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
