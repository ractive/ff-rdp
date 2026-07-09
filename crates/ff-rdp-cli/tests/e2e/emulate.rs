//! e2e coverage for `ff-rdp emulate` that needs no live Firefox.
//!
//! These exercise the argument-validation and help surface, which run before
//! any RDP connection is opened. The wire path (watcher → target-configuration
//! actor → updateConfiguration) and the applied-config probes are covered by
//! the live suite `tests/live/live_103_emulate.rs`.

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Run `ff-rdp emulate <args...>` against an unreachable port with a short
/// timeout. Validation that fails before connecting exits without ever
/// touching the network, so no server is needed.
fn run_emulate(args: &[&str]) -> std::process::Output {
    std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "1", // nothing listens here
            "--no-daemon",
            "--timeout",
            "300",
        ])
        .arg("emulate")
        .args(args)
        .output()
        .expect("run ff-rdp emulate")
}

/// `--reset` combined with a field flag is rejected up front (exit 1, User
/// error) — before any connection attempt.
#[test]
fn reset_with_field_flag_is_rejected() {
    let out = run_emulate(&["--reset", "--color-scheme", "dark"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "combining --reset with a field flag must be a user error; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("not JSON: {e}\n{stdout}"));
    let msg = json["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("--reset cannot be combined"),
        "error must explain the --reset conflict: {json}"
    );
}

/// No flags at all is rejected up front with a helpful message.
#[test]
fn no_flags_is_rejected() {
    let out = run_emulate(&[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "emulate with no flags must be a user error; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("not JSON: {e}\n{stdout}"));
    let msg = json["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("no configuration flags given"),
        "error must prompt for at least one flag: {json}"
    );
}

/// A non-positive `--dppx` is rejected before connecting.
#[test]
fn non_positive_dppx_is_rejected() {
    let out = run_emulate(&["--dppx", "0"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "--dppx 0 must be a user error; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("not JSON: {e}\n{stdout}"));
    let msg = json["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("--dppx must be a positive"),
        "error must explain the dppx constraint: {json}"
    );
}

/// An invalid `--color-scheme` value is rejected by clap (exit 2, usage error).
#[test]
fn invalid_color_scheme_is_usage_error() {
    let out = run_emulate(&["--color-scheme", "sepia"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "invalid enum value must be a clap usage error (exit 2); stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `emulate --help` documents the flags and lifetime semantics.
#[test]
fn help_documents_flags_and_lifetime() {
    let out = std::process::Command::new(ff_rdp_bin())
        .args(["emulate", "--help"])
        .output()
        .expect("emulate --help");
    assert!(out.status.success(), "emulate --help must exit 0");
    let help = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "--user-agent",
        "--color-scheme",
        "--dppx",
        "--print",
        "--touch",
        "--js",
        "--offline",
        "--cache",
        "--reset",
        "LIFETIME",
    ] {
        assert!(
            help.contains(needle),
            "emulate --help must mention {needle:?}:\n{help}"
        );
    }
}
