//! Tests for the `launch` command.
//!
//! We cannot actually launch Firefox in CI, so these tests focus on:
//! - CLI argument parsing (--help, flag combinations)
//! - `build_command` argument construction (white-box unit tests via `pub(crate)`)
//! - Graceful failure when given a non-existent binary path
//!
//! A live-Firefox integration test is left for local developer use and is
//! gated behind the `live_firefox` env-var pattern to avoid CI noise.

use super::support;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

#[test]
fn e2e_147_launch_url_validation_precedes_replace_and_profile_creation() {
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join("must-not-exist");
    for url in [
        "https://",
        "about:support\n",
        "file:///private/report",
        "javascript:void(0)",
    ] {
        let output = std::process::Command::new(ff_rdp_bin())
            .env("FF_RDP_HOME", home.path())
            .args(["launch", "--url", url, "--replace", "--profile"])
            .arg(&profile)
            .output()
            .expect("run invalid startup URL");
        assert!(!output.status.success());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["error_type"], "User");
        assert!(!profile.exists());
        assert!(!home.path().join(".ff-rdp").exists());
    }
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
        support::output_note(&output)
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
        support::output_note(&output)
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

// ---------------------------------------------------------------------------
// iter-133 Theme A — `launch --window-size`
// ---------------------------------------------------------------------------

/// AC: `e2e_help_viewport_pointers` — `launch --help` documents
/// `--window-size` and the ~500px live-viewport floor, without needing
/// Firefox installed.
#[test]
fn launch_help_mentions_window_size_and_floor() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("window-size"),
        "launch --help must document --window-size: {stdout}"
    );
    assert!(
        stdout.contains("500px") || stdout.contains("floor"),
        "launch --help must document the live-viewport floor: {stdout}"
    );
}

/// A malformed `--window-size` value must be rejected with a user error
/// naming the expected `WxH` form — before any port-collision check or
/// Firefox spawn, so this test needs neither a free port nor Firefox
/// installed (see the parse-before-spawn ordering in `commands::launch::run`).
#[test]
fn launch_window_size_invalid_rejected() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--window-size", "0x0", "--temp-profile"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(
        !output.status.success(),
        "expected non-zero exit for an invalid --window-size"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("WxH") || combined.contains("greater than 0"),
        "error must name the expected WxH form; stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// iter-191: a stale launch record is not ownership proof
// ---------------------------------------------------------------------------

/// AC (iter-110/191): `launch --replace` must not signal a process ff-rdp did
/// not launch. The port is held by this test process — a listener with no
/// owner-PID marker under ff-rdp's profile root — so the only acceptable
/// outcome is a refusal that names why, with the listener untouched.
///
/// `FF_RDP_HOME` scopes the command to a temp dir so the test never touches
/// the developer's real state.
#[test]
fn launch_replace_refuses_a_foreign_port_owner() {
    let home = tempfile::tempdir().expect("tempdir");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "launch",
            "--replace",
            "--headless",
            "--debug-port",
            &port.to_string(),
        ])
        .output()
        .expect("run ff-rdp launch --replace");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "launch --replace must fail rather than pretend it freed the port; got: {combined}"
    );
    assert!(
        combined.contains("did not launch")
            || combined.contains("does not own")
            || combined.contains("could not be identified"),
        "refusal must explain ff-rdp will not stop an unowned process; got: {combined}"
    );
    assert!(
        listener.local_addr().is_ok(),
        "the foreign listener must be untouched"
    );
    drop(listener);
}
