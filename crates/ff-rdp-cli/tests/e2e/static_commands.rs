fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

#[test]
fn recipes_help_does_not_show_host_flag() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["recipes", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("--host"),
        "recipes --help should not show --host flag"
    );
    assert!(
        !stdout.contains("--port"),
        "recipes --help should not show --port flag"
    );
    assert!(
        !stdout.contains("--timeout"),
        "recipes --help should not show --timeout flag"
    );
}

#[test]
fn llm_help_help_does_not_show_host_flag() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["llm-help", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("--host"),
        "llm-help --help should not show --host flag"
    );
    assert!(
        !stdout.contains("--port"),
        "llm-help --help should not show --port flag"
    );
    assert!(
        !stdout.contains("--timeout"),
        "llm-help --help should not show --timeout flag"
    );
}
