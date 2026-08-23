//! Tests for `ff-rdp throttle status` (Theme D, iter-131).
//!
//! `status` is a read-only query that never opens an RDP connection —
//! Firefox's network-parent actor has no getter, so there is nothing to
//! query. These tests exercise the client-side bookkeeping path directly:
//! no `MockRdpServer` is needed at all.

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// With no daemon running for the port (an isolated, empty `FF_RDP_HOME`),
/// `throttle status` must succeed (it never tries to connect) and report
/// `profile: null` with an explanatory `note`, not fabricate a profile.
#[test]
fn throttle_status_without_daemon_reports_null_with_note() {
    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    // Any port works — status never connects, so nothing needs to listen.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "throttle",
            "status",
        ])
        .output()
        .expect("spawn ff-rdp throttle status");

    assert!(
        output.status.success(),
        "throttle status must succeed even with no daemon running; stderr: {}",
        support::output_note(&output)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");

    assert!(
        json["results"]["profile"].is_null(),
        "no daemon running → profile must be null, not fabricated: {json}"
    );
    assert!(
        json["results"]["note"]
            .as_str()
            .is_some_and(|n| n.contains("no daemon")),
        "expected a note explaining why profile is null: {json}"
    );
    assert!(
        json["results"]["cache_caveat"]
            .as_str()
            .is_some_and(|c| c.contains("cache")),
        "expected the cache caveat to travel with every status response: {json}"
    );
}

/// `throttle status --block <pattern>` is a nonsensical combination — status
/// is read-only. Must be rejected before any connection attempt, with a
/// message pointing at the fix.
#[test]
fn throttle_status_rejects_combination_with_block() {
    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "throttle",
            "status",
            "--block",
            "*.png",
        ])
        .output()
        .expect("spawn ff-rdp throttle status --block");

    assert!(
        !output.status.success(),
        "status combined with --block must be rejected"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("read-only") || combined.contains("status"),
        "unexpected error output: {combined}"
    );
}
