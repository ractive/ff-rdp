//! Live test for iter-74 AC1: oneway methods return quickly without hanging.
//!
//! # Running
//!
//! ```sh
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live_oneway -- --nocapture
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
    fn launch() -> Option<Self> {
        let port = free_port();
        let output = Command::new(ff_rdp_bin())
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let firefox_pid = u32::try_from(json["results"]["pid"].as_u64()?).ok()?;

        if !wait_for_tcp(port, Duration::from_secs(30)) {
            kill_pid(firefox_pid);
            return None;
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
                return Some(ff);
            }
            if std::time::Instant::now() >= deadline {
                kill_pid(ff.firefox_pid);
                return None;
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
// AC1: live_watcher_oneway_unwatch_no_hang
// ---------------------------------------------------------------------------

/// AC: `live_watcher_oneway_unwatch_no_hang` — the `navigate` command (which
/// internally calls `unwatchResources` as cleanup) must complete in under
/// 5 seconds, not hang waiting for a reply that never arrives.
///
/// Before iter-74 `unwatch_resources` used `actor_request`, which would block
/// for the full socket read timeout (~10 s) because `unwatchResources` is
/// declared `oneway: true` — Firefox never sends a reply.  After the fix it
/// uses `actor_send` and returns immediately.
///
/// We time a data-URL navigation which exercises the watch → navigate →
/// unwatch lifecycle.  The assertion threshold is generous (5 s) to avoid
/// flakes on slow CI, while still catching a full socket timeout (≥10 s).
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a running Firefox instance"]
fn live_watcher_oneway_unwatch_no_hang() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        return;
    }

    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_watcher_oneway_unwatch_no_hang: Firefox not available — skipping");
        return;
    };

    // Navigate with --with-network so that watch_resources + unwatch_resources
    // are both exercised in the same command.
    let start = std::time::Instant::now();
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port))
        .args([
            "navigate",
            "data:text/html,<h1>oneway-test</h1>",
            "--timeout",
            "8000",
            "--allow-unsafe-urls",
            "--with-network",
            "--network-timeout",
            "1000",
        ])
        .output()
        .expect("failed to run navigate");
    let elapsed = start.elapsed();

    if !nav.status.success() {
        eprintln!(
            "live_watcher_oneway_unwatch_no_hang: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        // Navigation failure is not a test failure here — we only care about
        // whether it hung.
    }

    assert!(
        elapsed < Duration::from_secs(5),
        "navigate + unwatchResources took {elapsed:?} — expected <5s. \
         A longer time suggests unwatchResources is awaiting a reply that never arrives."
    );

    eprintln!("live_watcher_oneway_unwatch_no_hang: elapsed={elapsed:?} PASS");
}
