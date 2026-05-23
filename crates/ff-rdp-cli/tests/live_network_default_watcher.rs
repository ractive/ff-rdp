//! Live test for iter-61n Theme B — double-boundary fix.
//!
//! Verifies that `ff-rdp network` (no flags) returns `source: watcher` with
//! non-null `status` and `method` for at least one entry after a
//! `navigate --with-network` call.  Without the fix, the double-boundary
//! bug causes `--since -1` to resolve past the stored events, returning
//! an empty buffer and falling back to the Performance API (`source: performance-api`).
//!
//! # Running
//!
//! Requires Firefox, network access (example.com), and the ff-rdp binary.
//! Gates on `FF_RDP_LIVE_NETWORK_TESTS=1` (implies network).
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live_network_default_watcher -- --nocapture

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
                .args([
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &ff.port.to_string(),
                    "--no-daemon",
                    "tabs",
                ])
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

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_network_watcher_source_after_navigate_with_network`:
/// Navigate to example.com with `--with-network`.  Then call `ff-rdp network`
/// (no flags) and assert at least one entry has `source: "watcher"` with a
/// non-null `status` and `method`.
///
/// Without the double-boundary fix, `--since -1` resolves past the stored
/// events and the command falls back to `source: "performance-api"`.
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_watcher_source_after_navigate_with_network() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_network_watcher_source_after_navigate_with_network: \
             set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::launch() else {
        eprintln!(
            "live_network_watcher_source_after_navigate_with_network: Firefox not available — skipping"
        );
        return;
    };

    let daemon_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port.to_string(),
            "--timeout".to_owned(),
            "20000".to_owned(),
        ]
    };

    // Navigate to example.com with --with-network to capture watcher events.
    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args(["navigate", "https://example.com", "--with-network"])
        .output()
        .expect("navigate --with-network");

    if !nav.status.success() {
        eprintln!(
            "live_network_watcher_source_after_navigate_with_network: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
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

    // Now call `ff-rdp network` (no flags) — should read from watcher buffer.
    let network = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args(["network", "--format", "json"])
        .output()
        .expect("network");

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

    let net_json = parse_json(&network);
    let results = &net_json["results"];

    // The watcher buffer should have at least the document request.
    // Each entry should have source="watcher" and non-null status/method.
    let empty: Vec<serde_json::Value> = Vec::new();
    let entries = results.as_array().unwrap_or(&empty);

    let watcher_entries: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["source"].as_str() == Some("watcher"))
        .collect();

    assert!(
        !watcher_entries.is_empty(),
        "at least one network entry should have source='watcher' after navigate --with-network.\n\
         Got {} entries, all with sources: {:?}\n\
         This likely means the double-boundary bug is not fixed.",
        entries.len(),
        entries
            .iter()
            .map(|e| e["source"].as_str().unwrap_or("?"))
            .collect::<Vec<_>>()
    );

    let entry_with_status = watcher_entries
        .iter()
        .find(|e| !e["status"].is_null() && e["method"].as_str().is_some());

    assert!(
        entry_with_status.is_some(),
        "at least one watcher entry should have non-null status and method.\n\
         Watcher entries: {watcher_entries:?}"
    );

    let count = watcher_entries.len();
    let first = watcher_entries.first();
    eprintln!(
        "live_network_watcher_source_after_navigate_with_network: PASSED — \
         {count} watcher entries, first: {first:?}"
    );
}
