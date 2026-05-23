//! Live test for iter-61n Theme A — `watchTargets("frame")` engagement.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_daemon_watch_targets -- --nocapture

use std::path::PathBuf;
use std::process::{Command, Output};
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
    pub port: u16,
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

#[cfg(unix)]
fn kill_pid(pid: u32) {
    unsafe {
        // SAFETY: kill(2) is safe to call with a valid signal; ESRCH is ignored.
        libc::kill(pid.cast_signed(), libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
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

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

// ---------------------------------------------------------------------------
// Helper: start a daemon for this Firefox instance and return its proxy port.
// ---------------------------------------------------------------------------

fn start_daemon_for(ff: &LiveFirefox) -> Option<u16> {
    // Trigger daemon start by running a command without --no-daemon.
    // The daemon auto-starts on the first non-no-daemon invocation.
    let out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port.to_string(),
            "--timeout",
            "5000",
            "tabs",
        ])
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    // Give the daemon a moment to start and write its registry.
    std::thread::sleep(Duration::from_millis(500));

    // Check daemon status to confirm it's running.
    let status = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port.to_string(),
            "daemon",
            "status",
        ])
        .output()
        .ok()?;
    let status_json = serde_json::from_slice::<serde_json::Value>(&status.stdout).ok()?;
    if status_json["results"]["running"].as_bool() != Some(true) {
        return None;
    }
    status_json["results"]["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
}

/// `live_daemon_watch_targets`:
/// Navigate to two data URLs back-to-back.  The daemon should receive
/// `target-available-form` events and increment `target_count` in `daemon status`.
///
/// Asserts: `daemon status` shows `target_count >= 1` after the navigations.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_daemon_watch_targets() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_daemon_watch_targets: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_daemon_watch_targets: Firefox not available — skipping");
        return;
    };

    // Trigger daemon startup.
    if start_daemon_for(&ff).is_none() {
        eprintln!("live_daemon_watch_targets: daemon did not start — skipping");
        // Stop any partial daemon.
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port.to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    let daemon_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port.to_string(),
            "--timeout".to_owned(),
            "10000".to_owned(),
        ]
    };

    // Navigate to first page.
    let nav1 = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args([
            "navigate",
            "data:text/html,<h1>Page A</h1>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("nav1");
    if !nav1.status.success() {
        eprintln!("live_daemon_watch_targets: nav1 failed; skipping");
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port.to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    // Navigate to second page (cross-origin from first).
    let nav2 = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args([
            "navigate",
            "data:text/html,<h1>Page B</h1>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("nav2");
    if !nav2.status.success() {
        eprintln!("live_daemon_watch_targets: nav2 failed; skipping");
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port.to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    // Give the daemon a moment to process the target events.
    std::thread::sleep(Duration::from_millis(500));

    // Check daemon status — target_count should be >= 1.
    let status = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port.to_string(),
            "daemon",
            "status",
        ])
        .output()
        .expect("daemon status");
    let status_json = parse_json(&status);
    let target_count = status_json["results"]["target_count"].as_u64().unwrap_or(0);

    // Clean up daemon before asserting.
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port.to_string(),
            "daemon",
            "stop",
        ])
        .output();

    assert!(
        target_count >= 1,
        "daemon target_count should be >= 1 after navigations (got {target_count}); \
         watchTargets('frame') may not be engaged.\n\
         daemon status: {status_json}"
    );

    eprintln!("live_daemon_watch_targets: PASSED — target_count={target_count}");
}
