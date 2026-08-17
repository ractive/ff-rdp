//! Live tests for iter-158: `launch` must survive a contended debug-port
//! bind, report the bound it used, create a missing `--profile` directory,
//! repeat `--replace` cleanly, and free a port whose parent has been killed.
//!
//! ## Why these are live tests and not unit tests
//!
//! Every defect this iteration fixes is a *timing and process-lifetime*
//! defect. The 5 s port wait passed in isolation and failed 5/5 at load
//! average 6.8; the stop ladder's dead code was invisible until a parent was
//! killed out from under it. Both are only observable against a real Firefox.
//!
//! On 2026-08-13 the one real product defect a full `live-sweep` found was
//! `live_153_replace_emits_single_envelope` failing on
//! `"debug port … is not reachable after 5s"` — a test about `--replace`
//! failing on a bug about `launch`. That is why the assertions below check the
//! error *text* as well as the exit code.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_158 -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::{
    FirefoxGuard, LiveFirefox, base_args, ff_rdp_bin, kill_pid, live_tests_enabled, pid_alive,
};

/// Parse a `ff-rdp` stdout buffer into JSON, with the raw text in the panic
/// message when it is not JSON at all.
fn parse_json(what: &str, stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).unwrap_or_else(|e| {
        panic!(
            "{what}: stdout is not a single JSON document ({e})\nstdout={}",
            String::from_utf8_lossy(stdout)
        )
    })
}

/// Poll until `127.0.0.1:port` stops accepting connections, or `timeout`.
fn wait_for_port_closed(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(200),
        )
        .is_err()
        {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC `live_158_launch_survives_contended_bind`: four concurrent
/// `ff-rdp launch --headless` all exit 0 with distinct live PIDs, and no
/// stdout mentions the pre-158 5 s bound.
///
/// This is the regression the whole iteration exists for: Firefox was measured
/// binding its debug port at 7 s under load, and `launch`'s hardcoded
/// `Duration::from_secs(5)` turned that into a 5/5 failure.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_launch_survives_contended_bind() {
    if !live_tests_enabled() {
        return;
    }

    // Four at once: enough contention to reproduce the >5 s bind, launched in
    // parallel threads so they genuinely compete rather than queue.
    let handles: Vec<_> = (0..4)
        .map(|i| {
            std::thread::spawn(move || {
                let out = Command::new(ff_rdp_bin())
                    .args(["launch", "--headless"])
                    .args(["--debug-port", &(7101 + i).to_string()])
                    .output()
                    .expect("spawn `ff-rdp launch`");
                (
                    out.status.success(),
                    String::from_utf8_lossy(&out.stdout).into_owned(),
                    String::from_utf8_lossy(&out.stderr).into_owned(),
                )
            })
        })
        .collect();

    let results: Vec<(bool, String, String)> = handles
        .into_iter()
        .map(|h| h.join().expect("thread"))
        .collect();

    // Bind guards before asserting so a failure still reaps every instance.
    let guards: Vec<FirefoxGuard> = results
        .iter()
        .filter(|(ok, _, _)| *ok)
        .filter_map(|(_, stdout, _)| {
            serde_json::from_str::<serde_json::Value>(stdout)
                .ok()
                .and_then(|j| j["results"]["pid"].as_u64())
                .and_then(|p| u32::try_from(p).ok())
        })
        .map(FirefoxGuard::new)
        .collect();

    for (i, (ok, stdout, stderr)) in results.iter().enumerate() {
        assert!(
            *ok,
            "live_158_launch_survives_contended_bind: concurrent launch {i} failed\n\
             stdout={stdout}\nstderr={stderr}"
        );
        assert!(
            !stdout.contains("not reachable after 5s"),
            "the pre-158 5 s bound must be gone; launch {i} stdout={stdout}"
        );
        assert!(
            !stdout.contains("is the port already in use?"),
            "the deadline path must not blame a port conflict; launch {i} stdout={stdout}"
        );
    }

    let pids: Vec<u32> = guards.iter().map(FirefoxGuard::pid).collect();
    assert_eq!(pids.len(), 4, "all four launches must report a pid");
    let mut sorted = pids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 4, "the four pids must be distinct: {pids:?}");
    for pid in &pids {
        assert!(pid_alive(*pid), "launched pid {pid} must be alive");
    }
    eprintln!("live_158_launch_survives_contended_bind: PASS — pids {pids:?}");
}

/// AC `live_158_launch_reports_effective_wait_bound`: `--launch-timeout 45`
/// reports `meta.launch_wait_secs == 45`, and `FF_RDP_LAUNCH_TIMEOUT_SECS=40`
/// with no flag reports 40.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_launch_reports_effective_wait_bound() {
    if !live_tests_enabled() {
        return;
    }

    let flag_out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", "7105"])
        .args(["--launch-timeout", "45"])
        .output()
        .expect("spawn `ff-rdp launch --launch-timeout`");
    let flag_json = parse_json("launch --launch-timeout 45", &flag_out.stdout);
    let _flag_guard = flag_json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);
    assert!(
        flag_out.status.success(),
        "launch --launch-timeout 45 failed: {}",
        String::from_utf8_lossy(&flag_out.stderr)
    );
    assert_eq!(
        flag_json["meta"]["launch_wait_secs"].as_u64(),
        Some(45),
        "--launch-timeout must be reported in meta.launch_wait_secs: {flag_json}"
    );

    let env_out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", "7106"])
        .env("FF_RDP_LAUNCH_TIMEOUT_SECS", "40")
        .output()
        .expect("spawn `ff-rdp launch` with FF_RDP_LAUNCH_TIMEOUT_SECS");
    let env_json = parse_json("launch with FF_RDP_LAUNCH_TIMEOUT_SECS=40", &env_out.stdout);
    let _env_guard = env_json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);
    assert!(
        env_out.status.success(),
        "launch with FF_RDP_LAUNCH_TIMEOUT_SECS=40 failed: {}",
        String::from_utf8_lossy(&env_out.stderr)
    );
    assert_eq!(
        env_json["meta"]["launch_wait_secs"].as_u64(),
        Some(40),
        "FF_RDP_LAUNCH_TIMEOUT_SECS must be reported in meta.launch_wait_secs: {env_json}"
    );
}

/// AC `live_158_replace_repeats_cleanly`: three consecutive
/// `launch --debug-port P --replace` each exit 0, each emit exactly one JSON
/// document with `meta.replaced.stopped == true`, and neither the
/// "no owner-PID marker" refusal nor the "still in use after stopping the
/// prior instance" error appears.
///
/// Theme B is what makes this repeatable: a failed stop used to delete the
/// `DaemonRecord` first, so the next `--replace` fell into the raw port-owner
/// branch and was refused against an instance ff-rdp had launched itself.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_replace_repeats_cleanly() {
    if !live_tests_enabled() {
        return;
    }

    let port = 7108u16;
    let first = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("spawn the initial `ff-rdp launch`");
    let first_json = parse_json("initial launch", &first.stdout);
    let mut guard = first_json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);
    assert!(
        first.status.success(),
        "the initial launch failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    for round in 1..=3u8 {
        let out = Command::new(ff_rdp_bin())
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .arg("--replace")
            .output()
            .expect("spawn `ff-rdp launch --replace`");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

        // Exactly one JSON document — `parse_json` fails on trailing data,
        // which is iter-153's double-envelope defect.
        let json = parse_json(&format!("--replace round {round}"), &out.stdout);
        // Re-point the guard at the replacement before asserting.
        if let Some(pid) = json["results"]["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
        {
            guard = Some(FirefoxGuard::new(pid));
        }

        assert!(
            out.status.success(),
            "--replace round {round} exited non-zero\nstdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !stdout.contains("no owner-PID marker"),
            "--replace round {round} hit the fails-closed guard against an \
             ff-rdp-launched instance (Theme B): {stdout}"
        );
        assert!(
            !stdout.contains("still in use after stopping the prior instance"),
            "--replace round {round} could not free the port (Theme C): {stdout}"
        );
        assert_eq!(
            json["meta"]["replaced"]["stopped"].as_bool(),
            Some(true),
            "--replace round {round} must report meta.replaced.stopped == true: {json}"
        );
    }
    drop(guard);
}

/// AC `live_158_stop_reaches_orphaned_children`: after SIGKILLing only the
/// parent PID, `ff-rdp daemon stop --port P` still exits 0 and the port is
/// genuinely free — the escalation ladder reaches the orphaned children.
///
/// Pre-158 `run_escalation` returned at its `is_alive` guard (its only caller
/// had already killed the pid), so steps 3-7 never ran and this reported
/// `"port still listening after 8s"`.
#[cfg(unix)]
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_stop_reaches_orphaned_children() {
    if !live_tests_enabled() {
        return;
    }

    let port = 7109u16;
    let out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("spawn `ff-rdp launch`");
    let json = parse_json("launch for the orphan test", &out.stdout);
    // iter-169: print stdout as well. ff-rdp reports failures as a JSON error
    // envelope on **stdout** (that is the whole point of the JSON-only output
    // rule), so the previous stderr-only message rendered as a bare
    // `launch failed: ` — which is exactly what iteration 169's sweep got,
    // leaving the one failure in 272 undiagnosable.
    assert!(
        out.status.success(),
        "launch on port {port} failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let pid = u32::try_from(json["results"]["pid"].as_u64().expect("results.pid"))
        .expect("results.pid fits u32");
    let guard = FirefoxGuard::new(pid);

    // Orphan the children: SIGKILL only the parent, leaving whatever still
    // holds the port behind. The parent is dead, so `getpgid(pid)` now fails —
    // exactly the case the pre-captured-pgid fallback exists for.
    kill_pid(pid);
    let dead_by = std::time::Instant::now() + Duration::from_secs(5);
    while pid_alive(pid) && std::time::Instant::now() < dead_by {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!pid_alive(pid), "the parent pid {pid} should be dead");

    let stop = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output()
        .expect("spawn `ff-rdp daemon stop`");
    let stop_stdout = String::from_utf8_lossy(&stop.stdout).into_owned();
    let stop_stderr = String::from_utf8_lossy(&stop.stderr).into_owned();

    assert!(
        !stop_stdout.contains("port still listening after 8")
            && !stop_stderr.contains("port still listening after 8"),
        "the escalation ladder must reach the orphaned children\n\
         stdout={stop_stdout}\nstderr={stop_stderr}"
    );
    assert!(
        stop.status.success(),
        "`daemon stop --port {port}` exited {}\nstdout={stop_stdout}\nstderr={stop_stderr}",
        stop.status
    );
    assert!(
        wait_for_port_closed(port, Duration::from_secs(8)),
        "port {port} must be genuinely free after `daemon stop`"
    );
    drop(guard);
}

/// AC `live_158_launch_creates_missing_profile_dir`: `--profile` pointed at a
/// path that does not exist yet is created, populated with `user.js`, and
/// reported back as `results.profile_path`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_launch_creates_missing_profile_dir() {
    if !live_tests_enabled() {
        return;
    }

    let root = std::env::temp_dir().join(format!("ff-rdp-158-profile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let profile = root.join("absent").join("prof");
    assert!(
        !profile.exists(),
        "precondition: the profile must not exist"
    );

    let out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", "7110"])
        .args(["--profile", &profile.to_string_lossy()])
        .output()
        .expect("spawn `ff-rdp launch --profile`");
    let json = parse_json("launch --profile <absent>", &out.stdout);
    let _guard = json["results"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .map(FirefoxGuard::new);

    assert!(
        out.status.success(),
        "launch into a missing --profile directory failed: {}\nstdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        json["results"]["profile_path"].as_str(),
        Some(profile.to_string_lossy().as_ref()),
        "results.profile_path must be the directory that was requested: {json}"
    );
    let user_js = profile.join("user.js");
    assert!(
        user_js.exists(),
        "{} must have been created",
        user_js.display()
    );
    let contents = std::fs::read_to_string(&user_js).expect("read user.js");
    assert!(
        contents.contains("devtools.debugger.remote-enabled"),
        "user.js must carry the devtools prefs: {contents}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// The occupied-port branch, end to end: a plain TCP listener squatting the
/// debug port makes `launch` fail immediately with a message naming the
/// conflict — never the deadline message, and without waiting out the bound.
///
/// Uses a listener rather than `nc` so it runs on every platform. No Firefox
/// is involved, so this needs no live gate beyond the suite's own.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_occupied_port_fails_fast_with_the_occupant() {
    if !live_tests_enabled() {
        return;
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a squatter");
    let port = listener.local_addr().expect("local_addr").port();

    let started = std::time::Instant::now();
    let out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("spawn `ff-rdp launch` against an occupied port");
    let elapsed = started.elapsed();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !out.status.success(),
        "an occupied debug port must fail the launch: {combined}"
    );
    assert!(
        combined.contains(&format!("port {port} is already in use")),
        "the failure must name the occupancy: {combined}"
    );
    assert!(
        !combined.contains("did not open debug port"),
        "an occupied port must not be reported as a bind deadline: {combined}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "the occupancy check runs before the spawn, so it must not wait out the \
         30 s bind bound (took {elapsed:?})"
    );
    drop(listener);
}

/// The harness itself now fails loudly rather than skipping. Guarded by an
/// explicit opt-in because it deliberately makes a launch fail: with `PATH`
/// emptied and every well-known Firefox path unreadable, `ff-rdp launch`
/// cannot find a browser, and `LiveFirefox::headless_on_random_port` must
/// panic with a diagnostic instead of returning a skip.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_158_harness_reports_a_working_launch() {
    if !live_tests_enabled() {
        return;
    }
    // The positive half: a successful launch yields a usable instance and the
    // `tabs` command answers over it. If Firefox is unavailable this now
    // PANICS inside the helper (iter-158 Theme D) rather than returning `None`
    // and reporting `ok`.
    let ff = LiveFirefox::headless_on_random_port();
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .arg("tabs")
        .output()
        .expect("spawn `ff-rdp tabs`");
    assert!(
        out.status.success(),
        "tabs failed against the launched instance: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json("tabs", &out.stdout);
    assert!(
        json["total"].as_u64().unwrap_or(0) >= 1,
        "the launched Firefox must expose at least one tab: {json}"
    );
}
