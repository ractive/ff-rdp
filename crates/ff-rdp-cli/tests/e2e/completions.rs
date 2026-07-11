//! E2E tests for `ff-rdp completions`.
//!
//! `completions` needs no RDP connection — it just renders a shell script to
//! stdout — so unlike most other e2e tests here there is no
//! `MockRdpServer`/`base_args` port setup; the binary is invoked directly.

use std::path::PathBuf;

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Each supported shell must produce non-empty stdout that references the
/// binary name in some form. `clap_complete` substitutes `-` with `_` for
/// some shells (e.g. powershell/elvish function names), so the assertion is
/// deliberately loose: case-insensitive, accepting either spelling.
#[test]
fn completions_each_supported_shell_produces_binary_name() {
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let output = std::process::Command::new(ff_rdp_bin())
            .args(["completions", shell])
            .output()
            .unwrap_or_else(|e| panic!("spawn `ff-rdp completions {shell}`: {e}"));

        assert!(
            output.status.success(),
            "completions {shell} should exit 0; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.trim().is_empty(),
            "completions {shell} stdout must not be empty"
        );

        let lower = stdout.to_lowercase();
        assert!(
            lower.contains("ff-rdp") || lower.contains("ff_rdp"),
            "completions {shell} output should reference the binary name (ff-rdp or ff_rdp); got:\n{stdout}"
        );
    }
}

/// An unrecognized shell value must fail as a clap parse error: non-zero
/// exit, stderr mentioning the invalid value.
#[test]
fn completions_unknown_shell_fails_with_clap_parse_error() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["completions", "bogus"])
        .output()
        .expect("spawn `ff-rdp completions bogus`");

    assert!(
        !output.status.success(),
        "completions with an unknown shell should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bogus"),
        "stderr should mention the invalid value 'bogus'; got: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("invalid value") || stderr.to_lowercase().contains("error"),
        "stderr should read as a clap usage error; got: {stderr}"
    );
}

/// `completions` requires no `--host`/`--port`/`--no-daemon` flags — it never
/// touches Firefox or the daemon.
#[test]
fn completions_requires_no_connection_flags() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["completions", "bash"])
        .output()
        .expect("spawn `ff-rdp completions bash`");

    assert!(
        output.status.success(),
        "completions bash without any connection flags should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stdout.is_empty(),
        "completions bash without connection flags should still produce output"
    );
}
