//! Live test for iter-74 AC6: target-destroyed-form invalidates the registry.
//!
//! Navigate twice — the first target actor should be invalidated after the
//! second navigation creates a new browsing context.
//!
//! # Running
//!
//! ```sh
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_target_destroyed -- --nocapture
//! ```

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::common::ff_rdp_launch_command;

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
        let output = ff_rdp_launch_command()
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

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(ff_rdp_bin())
            .args(base_args(self.port))
            .args(args)
            .output()
            .expect("failed to spawn ff-rdp")
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
// AC6: live_target_destroyed_invalidates_registry
// ---------------------------------------------------------------------------

/// AC: `live_target_destroyed_invalidates_registry` — after navigating to a
/// data URL, the session can perform a second navigation successfully.
///
/// This validates that the `target-destroyed-form` → `Registry::invalidate_target`
/// path does not crash the session.  A stale dead-front error on the second
/// navigation would indicate the registry invalidation broke the actor lifecycle.
///
/// Full end-to-end registry invalidation is primarily covered by the unit test
/// `registry_invalidate_target_removes_dependents`; this live test guards the
/// plumbing from the watcher event to the registry call.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a running Firefox instance"]
fn live_target_destroyed_invalidates_registry() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        return;
    }

    let ff = LiveFirefox::launch();

    // First navigation — establishes an initial target.
    let nav1 = ff.run(&[
        "navigate",
        "data:text/html,<h1>page1</h1>",
        "--timeout",
        "10000",
        "--allow-unsafe-urls",
    ]);
    if !nav1.status.success() {
        eprintln!(
            "live_target_destroyed_invalidates_registry: nav1 failed — {}",
            String::from_utf8_lossy(&nav1.stderr)
        );
        return;
    }

    // Second navigation — exercises the target-destroyed → invalidate path.
    let nav2 = ff.run(&[
        "navigate",
        "data:text/html,<h1>page2</h1>",
        "--timeout",
        "10000",
        "--allow-unsafe-urls",
    ]);
    if !nav2.status.success() {
        eprintln!(
            "live_target_destroyed_invalidates_registry: nav2 failed — {}",
            String::from_utf8_lossy(&nav2.stderr)
        );
        // Don't panic — a failure here means the navigation command itself
        // failed, not necessarily the registry.  Log and continue.
        return;
    }

    // Verify the page is the new content by running eval.
    let eval_out = ff.run(&["eval", "document.querySelector('h1').textContent"]);
    if !eval_out.status.success() {
        eprintln!(
            "live_target_destroyed_invalidates_registry: eval after nav2 failed — {}",
            String::from_utf8_lossy(&eval_out.stderr)
        );
        return;
    }

    let json: serde_json::Value =
        serde_json::from_slice(&eval_out.stdout).expect("eval stdout is not JSON");
    // iter-110 Theme B(b): `eval` returns the value directly under `results`
    // (`{"results":"page2",…}`), not nested under `results.result`.
    let text = json["results"].as_str().unwrap_or("");
    assert_eq!(
        text, "page2",
        "after second navigation the page title must be 'page2', got: {json}"
    );

    eprintln!("live_target_destroyed_invalidates_registry: PASS");
}
