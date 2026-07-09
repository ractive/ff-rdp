//! Tests for the `launch` command.
//!
//! We cannot actually launch Firefox in CI, so these tests focus on:
//! - CLI argument parsing (--help, flag combinations)
//! - `build_command` argument construction (white-box unit tests via `pub(crate)`)
//! - Graceful failure when given a non-existent binary path
//!
//! A live-Firefox integration test is left for local developer use and is
//! gated behind the `live_firefox` env-var pattern to avoid CI noise.

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

// ---------------------------------------------------------------------------
// CLI argument-parsing smoke tests (no Firefox needed)
// ---------------------------------------------------------------------------

#[test]
fn launch_help_exits_zero() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        output.status.success(),
        "expected zero exit for --help, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("headless") || stdout.contains("Launch"),
        "help output should mention launch flags: {stdout}"
    );
}

/// `launch --port <busy>` must fail with a structured error that names
/// `doctor`, instead of silently spawning a Firefox that no-ops because the
/// port is taken.
#[test]
fn launch_detects_port_collision() {
    // Bind to a port and hold it open for the duration of the test.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();

    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "launch",
            "--debug-port",
            &port.to_string(),
            "--temp-profile",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    drop(listener);

    assert!(
        !output.status.success(),
        "expected non-zero exit when port is in use; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The port-collision error is emitted as the JSON error envelope on stdout
    // (iter-98 Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("already in use"),
        "output must mention 'already in use'; stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        combined.contains("ff-rdp doctor") || combined.contains("`ff-rdp doctor`"),
        "output must reference `ff-rdp doctor`; stderr={stderr:?} stdout={stdout:?}"
    );
}

/// `ff-rdp --help` (top-level) must mention `ff-rdp doctor` somewhere in the
/// command reference so AI agents can discover it without grep-spelunking.
#[test]
fn help_mentions_doctor() {
    let output = std::process::Command::new(ff_rdp_bin())
        .arg("--help")
        .output()
        .expect("spawn");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("doctor"),
        "top-level --help must mention `doctor`; got:\n{stdout}"
    );
}
