//! iter-172 — a zero-byte `daemon.<port>.json` must not silently downgrade a
//! daemon-routed command to a direct connection.
//!
//! ## The defect
//!
//! Through iter-171, `registry::write_registry_in` took its exclusive lock by
//! opening the **published** record with `create(true)`:
//!
//! ```text
//! let lock_file = fs::OpenOptions::new().create(true).truncate(false)
//!     .write(true).open(&registry_path)?;   // ← publishes a zero-byte record
//! lock_file.lock_exclusive()?;
//! …write daemon.<port>.json.tmp, then fs::rename onto registry_path…
//! ```
//!
//! So an empty `daemon.<port>.json` existed from lock-open until the rename.
//! A reader landing in that window parsed zero bytes and got
//! `EOF while parsing a value at line 1 column 0`, which autostart treated as
//! terminal — it abandoned the wait and ran the command over a *direct*
//! connection after the caller had explicitly asked for the daemon. It reddened
//! `live_128_meta_route`, `live_134_meta_route_all_commands` and
//! `live_123_daemon_autostart_tabless` across three separate live sweeps.
//!
//! ## Why this test plants the file instead of racing for it
//!
//! The window is short — the three sweep failures needed a fourteen-minute
//! contended tier to land in it. But its *contents* are not ambiguous: a
//! zero-byte record is a zero-byte record however it got there, and one left
//! behind by a pre-iter-172 build (or by any external truncation) poisons that
//! port **permanently**, not just for the width of a race. Planting it is the
//! deterministic form of the same input, and it is the form a user actually
//! hits after upgrading.
//!
//! Both assertions fail on `main`: there, the planted record makes
//! `find_running_daemon` return `Err`, so `resolve_connection_target` bails
//! straight to `Direct` and `meta.route` reads `"direct"`.
//!
//! daemon-parity: this test runs **exclusively** through the daemon path.
//! `--no-daemon` skips `resolve_connection_target` entirely and therefore
//! cannot observe the registry at all, so there is no parallel `--no-daemon`
//! assertion to make.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_172 \
//!       -- --include-ignored --nocapture

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// The registry directory the product reads, honouring the same `FF_RDP_HOME`
/// override `registry::registry_dir` does. Duplicated rather than imported for
/// the reason every other live test duplicates a product constant: this crate
/// ships no `[lib]` target for an integration-test binary to import from.
fn registry_dir() -> PathBuf {
    let home = std::env::var_os("FF_RDP_HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .expect("live_172: could not determine the home directory");
    home.join(".ff-rdp")
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Run a daemon-routed command with `--verbose`, so `meta.daemon_fallback` is
/// present when autostart degraded and the assertion can say why.
fn run_verbose(port: u16, args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "30000",
            "--verbose",
        ])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("live_172: spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "live_172: {args:?} failed — stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "live_172: {args:?} stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// AC (iter-172): a planted zero-byte registry record does not prevent
/// autostart from producing a daemon, and the command reports
/// `meta.route == "daemon"` rather than degrading to `"direct"`.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_172_zero_byte_registry_does_not_downgrade_to_direct() {
    if !live_tests_enabled() {
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    // No daemon must be running for this port yet, or the fast path in
    // `resolve_connection_target` would never look at the registry.
    stop_daemon(port);

    let dir = registry_dir();
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("live_172: creating {}: {e}", dir.display()));
    let record = dir.join(format!("daemon.{port}.json"));
    // Exactly what the pre-iter-172 writer published while holding its lock.
    std::fs::write(&record, [])
        .unwrap_or_else(|e| panic!("live_172: planting {}: {e}", record.display()));
    assert_eq!(
        std::fs::metadata(&record).map(|m| m.len()).ok(),
        Some(0),
        "live_172: the planted record must really be zero bytes"
    );

    let json = run_verbose(port, &["eval", "1 + 1"]);
    let route = json["meta"]["route"].as_str().unwrap_or("(absent)");
    let fallback = json["meta"]["daemon_fallback"]
        .as_str()
        .unwrap_or("(none recorded)");

    stop_daemon(port);
    drop(ff);

    assert_eq!(
        route, "daemon",
        "live_172: a zero-byte daemon.{port}.json must not downgrade the route; \
         meta.daemon_fallback said: {fallback}"
    );
    assert_eq!(
        json["meta"]["daemon_fallback"],
        Value::Null,
        "live_172: nothing degraded, so no fallback reason should be recorded"
    );
}

/// AC (iter-172), the writer half end to end: after a real daemon has
/// registered, the published record on disk is a complete, parseable JSON
/// object and the write lock lives in its own sibling file.
///
/// On `main` the sibling file does not exist at all — the lock was the record.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_172_published_record_is_complete_and_lock_is_a_sibling() {
    if !live_tests_enabled() {
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let daemon_port = match ff.with_daemon_or_reason() {
        Ok(p) => p,
        Err(reason) => {
            stop_daemon(port);
            panic!("live_172: the proxy daemon did not start for Firefox on port {port}: {reason}");
        }
    };

    let dir = registry_dir();
    let record = dir.join(format!("daemon.{port}.json"));
    let contents = std::fs::read_to_string(&record).unwrap_or_else(|e| {
        stop_daemon(port);
        panic!("live_172: {} unreadable: {e}", record.display())
    });
    let write_lock_exists = dir.join(format!("daemon.{port}.write.lock")).exists();

    stop_daemon(port);
    drop(ff);

    let parsed: Value = serde_json::from_str(&contents).unwrap_or_else(|e| {
        panic!("live_172: the published record must be complete JSON ({e}): {contents:?}")
    });
    assert_eq!(
        parsed["firefox_port"].as_u64(),
        Some(u64::from(port)),
        "live_172: the record must name the Firefox port it was written for"
    );
    assert_eq!(
        parsed["proxy_port"].as_u64(),
        Some(u64::from(daemon_port)),
        "live_172: the record must name the proxy port `daemon status` reported"
    );
    assert!(
        write_lock_exists,
        "live_172: the registry write lock must be a sibling file \
         (daemon.{port}.write.lock), never the published record itself"
    );
}
