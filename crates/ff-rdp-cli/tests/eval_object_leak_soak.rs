//! Soak test for iter-61t Theme C — `ScopedGrip` actor release in eval paths.
//!
//! Drives 1 000 `eval 'document.body'` calls against a headless Firefox daemon
//! and asserts that the daemon's process RSS grows by less than 50 MB.
//! `document.body` is an object grip, so without the release fix each call
//! would leave a server-side actor accumulating in Firefox's memory.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test eval_object_leak_soak -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;
use std::time::Duration;

use common::{LiveFirefox, ff_rdp_bin};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of consecutive `eval 'document.body'` calls in the soak loop.
const ITERATIONS: u32 = 1_000;

/// Maximum allowed daemon RSS growth over the soak run (bytes).
const MAX_RSS_DELTA_BYTES: u64 = 50 * 1024 * 1024; // 50 MB

// ---------------------------------------------------------------------------
// Process RSS helpers
// ---------------------------------------------------------------------------

/// Read the RSS (resident set size) of `pid` in bytes.
///
/// Returns `None` when the platform is unsupported or the measurement fails.
fn rss_bytes(pid: u32) -> Option<u64> {
    rss_bytes_impl(pid)
}

#[cfg(target_os = "linux")]
fn rss_bytes_impl(pid: u32) -> Option<u64> {
    // /proc/<pid>/status contains "VmRSS:\t<N> kB"
    let path = format!("/proc/{pid}/status");
    let content = std::fs::read_to_string(&path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn rss_bytes_impl(pid: u32) -> Option<u64> {
    // `ps -o rss= -p <pid>` returns RSS in kilobytes on macOS.
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let kb: u64 = s.trim().parse().ok()?;
    Some(kb * 1024)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rss_bytes_impl(_pid: u32) -> Option<u64> {
    // RSS measurement is not implemented on this platform; the test will skip
    // the RSS assertion and only assert on eval failure rate.
    None
}

// ---------------------------------------------------------------------------
// Soak test
// ---------------------------------------------------------------------------

/// `live_eval_object_leak_soak`: run `eval 'document.body'` 1 000 times
/// through the daemon and verify that daemon RSS delta < 50 MB.
///
/// `document.body` returns an object grip.  Without `ScopedGrip::release`,
/// every call leaves a server-side actor in Firefox, and the daemon's RSS
/// climbs linearly.  With the fix, actors are released after each call and
/// the RSS stays bounded.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_eval_object_leak_soak() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_eval_object_leak_soak: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_eval_object_leak_soak: Firefox not available — skipping");
        return;
    };

    // Build the common CLI args that route through the daemon (no --no-daemon).
    let daemon_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port().to_string(),
            "--timeout".to_owned(),
            "10000".to_owned(),
        ]
    };

    // Start the daemon with an initial tabs call (auto-start path).
    let init = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .arg("tabs")
        .output()
        .expect("tabs (daemon init)");
    assert!(
        init.status.success(),
        "live_eval_object_leak_soak: daemon init failed\nstderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    // Poll `daemon status` until the daemon has registered and reports a PID.
    // A fixed post-init sleep raced the daemon's registry write on slow CI
    // runners (`results.pid` absent while the daemon was still coming up).
    let status_deadline = std::time::Instant::now() + Duration::from_secs(10);
    let daemon_pid = loop {
        let status_out = Command::new(ff_rdp_bin())
            .args(daemon_args())
            .args(["daemon", "status"])
            .output()
            .expect("daemon status");
        assert!(
            status_out.status.success(),
            "live_eval_object_leak_soak: daemon status failed\nstderr: {}",
            String::from_utf8_lossy(&status_out.stderr)
        );
        let status_json: serde_json::Value =
            serde_json::from_slice(&status_out.stdout).expect("daemon status JSON");
        if let Some(pid) = status_json["results"]["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
        {
            break pid;
        }
        assert!(
            std::time::Instant::now() < status_deadline,
            "live_eval_object_leak_soak: daemon never reported a pid within 10 s; \
             last status: {status_json}"
        );
        std::thread::sleep(Duration::from_millis(250));
    };

    eprintln!("live_eval_object_leak_soak: daemon pid={daemon_pid}");

    // Record baseline RSS before the soak loop.
    let rss_before = rss_bytes(daemon_pid);
    eprintln!("live_eval_object_leak_soak: RSS before = {rss_before:?} bytes");

    // Run 1 000 `eval 'document.body'` calls through the daemon.
    let mut failures = 0u32;

    for i in 0..ITERATIONS {
        let out = Command::new(ff_rdp_bin())
            .args(daemon_args())
            .args(["eval", "document.body"])
            .output()
            .expect("eval document.body");

        if !out.status.success() {
            failures += 1;
            if failures <= 3 {
                eprintln!(
                    "live_eval_object_leak_soak: eval #{i} failed\nstderr: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }

    // Allow the daemon a moment to settle (pending releases may be in-flight).
    std::thread::sleep(Duration::from_millis(200));

    // Record RSS after the soak loop.
    let rss_after = rss_bytes(daemon_pid);
    eprintln!("live_eval_object_leak_soak: RSS after  = {rss_after:?} bytes");

    // Stop the daemon before asserting so Firefox is cleaned up on failure.
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "daemon",
            "stop",
        ])
        .output();

    // Assert: fewer than 5% of evals should fail.
    let failure_threshold = ITERATIONS / 20; // 5% = 50
    assert!(
        failures <= failure_threshold,
        "live_eval_object_leak_soak: too many eval failures: {failures}/{ITERATIONS} \
         (threshold {failure_threshold})"
    );

    // Assert: RSS delta < 50 MB.
    // If we can't measure RSS on this platform we skip the assertion and log.
    match (rss_before, rss_after) {
        (Some(before), Some(after)) => {
            let delta = after.saturating_sub(before);
            eprintln!(
                "live_eval_object_leak_soak: RSS delta = {delta} bytes ({} MB)",
                delta / (1024 * 1024)
            );
            assert!(
                delta < MAX_RSS_DELTA_BYTES,
                "live_eval_object_leak_soak: RSS delta {delta} bytes ({} MB) \
                 exceeds 50 MB limit after {ITERATIONS} evals — actor leak detected.\n\
                 Hint: check that ScopedGrip::release is called in commands/eval.rs.",
                delta / (1024 * 1024)
            );
            eprintln!(
                "live_eval_object_leak_soak: PASSED — RSS delta {} MB < 50 MB limit",
                delta / (1024 * 1024)
            );
        }
        _ => {
            eprintln!(
                "live_eval_object_leak_soak: RSS measurement not available on this \
                 platform — skipping RSS assertion (eval failure rate: {failures}/{ITERATIONS})"
            );
        }
    }
}
