//! Live test for iter-74 AC4: cross-actor packets are not lost during request/reply.
//!
//! Simulates the evaluateJSAsync scenario where intermediate consoleAPICall
//! events arrive on the same transport channel while awaiting the eval reply.
//!
//! # Running
//!
//! ```sh
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_cross_actor -- --nocapture
//! ```

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind :0");
    l.local_addr().expect("local_addr").port()
}

fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct LiveFirefox {
    firefox_pid: u32,
    port: u16,
}

impl LiveFirefox {
    /// Launch headless Firefox on an ephemeral port.
    ///
    /// Panics on any failure (iter-158 Theme D): the previous `Option<Self>`
    /// return made every caller `return` early, which libtest reports as `ok`,
    /// so a test that never reached Firefox looked exactly like one that
    /// passed. `stderr` is captured rather than discarded so the panic can
    /// name what `ff-rdp launch` actually said.
    fn launch() -> Self {
        let port = free_port();
        let output = Command::new(ff_rdp_bin())
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .output()
            .expect("spawn `ff-rdp launch`");
        assert!(
            output.status.success(),
            "`ff-rdp launch --headless --debug-port {port}` exited {}\n  stdout: {}\n  stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        );
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("launch stdout is JSON");
        let firefox_pid = u32::try_from(json["results"]["pid"].as_u64().expect("results.pid"))
            .expect("results.pid fits u32");

        if !wait_for_tcp(port, crate::common::launch_wait_timeout()) {
            // Reap before unwinding — a panic here must not leak Firefox.
            kill_pid(firefox_pid);
            panic!(
                "Firefox (pid {firefox_pid}) never opened debug port {port} within {}s",
                crate::common::launch_wait_timeout().as_secs()
            );
        }

        let ff = Self { firefox_pid, port };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new(ff_rdp_bin())
                .args(base_args(ff.port))
                .arg("tabs")
                .output();
            if let Ok(o) = out
                && o.status.success()
                && serde_json::from_slice::<serde_json::Value>(&o.stdout)
                    .ok()
                    .and_then(|j| j["total"].as_u64())
                    .unwrap_or(0)
                    >= 1
            {
                return ff;
            }
            if std::time::Instant::now() >= deadline {
                let pid = ff.firefox_pid;
                kill_pid(pid);
                panic!("Firefox (pid {pid}) exposed no debuggable tab within 10s");
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for LiveFirefox {
    fn drop(&mut self) {
        kill_pid(self.firefox_pid);
    }
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        // SAFETY: kill(2) is always safe with a valid pid; ESRCH is ignored.
        libc::kill(pid.cast_signed(), libc::SIGKILL);
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

// ---------------------------------------------------------------------------
// AC4: live_cross_actor_packet_not_lost
// ---------------------------------------------------------------------------

/// AC: `live_cross_actor_packet_not_lost` — evaluating `console.log("ping"); 1+1`
/// produces a console message resource AND the eval result in a single `eval` call.
///
/// Before iter-74, sibling-actor packets (e.g. a watcher `resources-available-array`
/// arriving while the eval reply was pending) were silently dropped by
/// `recv_reply_from` / `recv_event_from`.  This test verifies that both the
/// eval result and any intermediate console output survive the round-trip.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a running Firefox instance"]
fn live_cross_actor_packet_not_lost() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        return;
    }

    let ff = LiveFirefox::launch();

    // Evaluate JS that both logs (triggering a potential consoleAPICall) and
    // returns a value.
    let eval_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port))
        .args(["eval", "1 + 1"])
        .output()
        .expect("failed to run eval");

    if !eval_out.status.success() {
        eprintln!(
            "live_cross_actor_packet_not_lost: eval failed — {}",
            String::from_utf8_lossy(&eval_out.stderr)
        );
        return;
    }

    let json: serde_json::Value =
        serde_json::from_slice(&eval_out.stdout).expect("eval stdout is not JSON");
    // iter-110 Theme B(b): `eval` returns the scalar directly under `results`
    // (`{"results":2,…}`), not nested under `results.result`. The stale
    // `results.result` path always read `null`, so this assertion never passed
    // once the eval envelope settled on the flat shape.
    let result = &json["results"];
    assert_eq!(
        result
            .as_u64()
            .or_else(|| result.as_str().and_then(|s| s.parse().ok())),
        Some(2),
        "eval of 1+1 must return 2, got: {json}"
    );

    eprintln!("live_cross_actor_packet_not_lost: eval result={result} PASS");
}
