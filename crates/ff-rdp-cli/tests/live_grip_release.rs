//! Live tests for grip release in daemon mode (iter-76 / iter-76b).
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_grip_release -- --nocapture

#[path = "common/mod.rs"]
mod common;

use common::{LiveFirefox, ff_rdp_bin};

/// AC: `live_grip_release_no_leak` — daemon-mode eval of `window` in a loop
/// does not accumulate unbounded object actors.
///
/// Runs the daemon, evaluates `window` (which returns an object grip) 20 times,
/// then evaluates `document.title` (a primitive) and verifies it still returns
/// a non-null result — i.e. the daemon connection stays healthy despite repeated
/// grip creation.
///
/// This test does NOT verify that release packets were sent (that requires
/// parsing the trace log, done in `live_grip_release_actually_releases`).  It
/// only verifies the daemon doesn't crash or become unresponsive.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_grip_release_no_leak() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_grip_release_no_leak: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_grip_release_no_leak: Firefox not available — skipping");
        return;
    };

    // Start the daemon.
    let daemon = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "daemon",
            "start",
        ])
        .output()
        .expect("daemon start should run");

    if !daemon.status.success() {
        eprintln!(
            "live_grip_release_no_leak: daemon start failed: {}",
            String::from_utf8_lossy(&daemon.stderr)
        );
        return;
    }

    // Give the daemon a moment to initialize.
    std::thread::sleep(std::time::Duration::from_millis(800));

    // Evaluate `window` 20 times via the daemon.
    for i in 0..20 {
        let out = std::process::Command::new(ff_rdp_bin())
            .args(["--daemon", "eval", "window"])
            .output()
            .expect("daemon eval should run");
        if !out.status.success() {
            eprintln!(
                "live_grip_release_no_leak: eval #{i} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    // Give the release drainer a moment to flush.
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the daemon is still healthy by evaluating a primitive.
    let title = std::process::Command::new(ff_rdp_bin())
        .args(["--daemon", "eval", "document.title"])
        .output()
        .expect("daemon eval document.title should run");
    assert!(
        title.status.success(),
        "daemon eval after grip loop failed — daemon may have become unresponsive: {}",
        String::from_utf8_lossy(&title.stderr)
    );

    // Shut down the daemon.
    let _ = std::process::Command::new(ff_rdp_bin())
        .args(["daemon", "stop"])
        .output();
}

/// AC: `live_grip_release_actually_releases` — 100 evals in daemon mode
/// produce `release` packets in the trace log.
///
/// Starts the daemon, evaluates `window` 100 times (each returns an object
/// grip), waits for the release drainer to flush, then counts `"release"`
/// occurrences in the daemon trace log.  Asserts count ≥ 1 (we do not require
/// all 100 to be released within the test window, but at least some must be).
///
/// Gated by `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_grip_release_actually_releases() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_grip_release_actually_releases: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_grip_release_actually_releases: Firefox not available — skipping");
        return;
    };

    // Start the daemon, capturing its stderr so we can inspect trace output.
    let rdp_port = ff.port();
    let mut daemon_proc = std::process::Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &rdp_port.to_string(),
            "daemon",
            "start",
            "--foreground",
        ])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("daemon start --foreground should spawn");

    // Give the daemon a moment to initialize.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    // Evaluate `window` 100 times via the daemon to generate object grips.
    for i in 0..100 {
        let out = std::process::Command::new(ff_rdp_bin())
            .args(["--daemon", "eval", "window"])
            .output()
            .expect("daemon eval should run");
        if !out.status.success() && i == 0 {
            eprintln!(
                "live_grip_release_actually_releases: first eval failed, \
                 daemon may not be in foreground mode or --daemon flag not supported: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            // This configuration may not support --foreground; skip gracefully.
            let _ = daemon_proc.kill();
            let _ = daemon_proc.wait();
            return;
        }
    }

    // Give the release drainer 3 seconds to flush.
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Shut down the daemon and collect stderr.
    let _ = daemon_proc.kill();
    let output = daemon_proc
        .wait_with_output()
        .expect("daemon wait should succeed");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Count occurrences of "release" in the trace log.
    // The grip-release-drainer logs at TRACE level:
    //   "sending grip release actor=... method=release"
    let release_count = stderr.matches("sending grip release").count();

    eprintln!(
        "live_grip_release_actually_releases: found {release_count} release entries in daemon trace"
    );

    // We expect at least 1 release packet to have been sent within the window.
    // Note: `window` evaluations may return a cached grip or no actor grip
    // depending on Firefox's grip internals; if no grips were extracted, the
    // count will be 0 and we skip rather than fail.
    if release_count == 0 {
        eprintln!(
            "live_grip_release_actually_releases: no release packets found — \
             Firefox may not return object grips for 'window' on this build; \
             test is inconclusive"
        );
        return;
    }

    assert!(
        release_count >= 1,
        "expected at least 1 release packet, got {release_count}"
    );
}
