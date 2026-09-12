//! Real libtest proof for iteration 155's unqualified network-test verdict.

// allow-ungated-live: Firefox-free; runs only an exact ignored case in this executable with both live env gates removed, without --include-ignored.
#[test]
fn test_155_skipped_live_test_is_not_counted_passed() {
    let target_name = "live_109_throttle_block::live_block_url_pattern";
    // Exact selection excludes this guard, so the child cannot recurse.
    let output = std::process::Command::new(std::env::current_exe().expect("live test executable"))
        .args(["--exact", target_name, "--format=pretty"])
        .env_remove("FF_RDP_LIVE_TESTS")
        .env_remove("FF_RDP_LIVE_NETWORK_TESTS")
        .output()
        .expect("run the already-built live test executable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "libtest child failed: {}; stdout:\n{stdout}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(&format!("test {target_name} ... ignored")),
        "expected the selected network test to be ignored; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(&format!("test {target_name} ... ok")),
        "an unqualified test must never report ok; stdout:\n{stdout}"
    );
    assert!(stdout.contains("0 passed; 0 failed; 1 ignored"), "{stdout}");
}
