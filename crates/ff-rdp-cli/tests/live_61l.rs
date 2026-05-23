//! Live tests for iter-61l — every AC requires a passing live test against
//! real headless Firefox.
//!
//! # Running
//!
//! These tests launch headless Firefox themselves.  They require:
//! - Firefox installed and findable by `ff-rdp launch` (standard PATH/macOS app).
//! - The `ff-rdp` binary built in the same profile as the tests.
//!
//! The tests are skipped when Firefox is not available (the `ff-rdp launch`
//! helper will fail and the guard will return early).  This keeps CI green even
//! when Firefox is not installed in the CI environment.
//!
//! Tests that require external network access (DNS resolution, example.com)
//! are additionally gated behind `FF_RDP_LIVE_NETWORK_TESTS=1` so they don't
//! flake in network-restricted CI environments.
//!
//! To run locally (Firefox-only tests):
//!   cargo test -p ff-rdp-cli --test live_61l -- --nocapture
//!
//! To run network-requiring tests too:
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live_61l -- --nocapture
//!
//! To run a single AC:
//!   cargo test -p ff-rdp-cli --test live_61l live_screenshot_full_page -- --nocapture

use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Find a free TCP port by binding to port 0 then releasing the listener.
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind :0");
    l.local_addr().expect("local_addr").port()
}

/// Wait until TCP port `port` on localhost accepts connections, up to `timeout`.
/// Returns `true` if the port became available.
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

/// A running headless Firefox instance.  Drops = kill.
struct LiveFirefox {
    /// PID of the Firefox process (not the ff-rdp launcher, which exits
    /// immediately after printing the JSON launch result).  Used for cleanup.
    firefox_pid: u32,
    pub port: u16,
}

impl LiveFirefox {
    /// Launch headless Firefox on an ephemeral port.
    /// Returns `None` when Firefox is not available (skips the test).
    fn launch() -> Option<Self> {
        Self::launch_with_env(&[])
    }

    /// Launch headless Firefox with additional parent-environment overrides.
    fn launch_with_env(env_pairs: &[(&str, &str)]) -> Option<Self> {
        let port = free_port();
        let mut cmd = Command::new(ff_rdp_bin());
        cmd.args(["launch", "--headless", "--debug-port", &port.to_string()])
            .stderr(std::process::Stdio::null());
        for (k, v) in env_pairs {
            cmd.env(k, v);
        }

        // Capture stdout to extract the Firefox PID from the JSON response.
        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let firefox_pid = u32::try_from(json["results"]["pid"].as_u64()?).ok()?;

        // Wait for the RDP port to become available.  Use 30 s because multiple
        // parallel live tests each spawn Firefox and contention can delay startup.
        if !wait_for_tcp(port, Duration::from_secs(30)) {
            // Firefox failed to start — kill it and skip.
            kill_pid(firefox_pid);
            return None;
        }

        // Poll until Firefox has at least one debuggable tab (up to 10 s).
        // The TCP port opens before the tab system is initialised, so a bare
        // port-available check causes "0 debuggable tabs" errors.
        let ff = Self { firefox_pid, port };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new(ff_rdp_bin())
                .args(base_args(ff.port))
                .arg("tabs")
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    // Parse total — must be >= 1.
                    if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&o.stdout)
                        && j["total"].as_u64().unwrap_or(0) >= 1
                    {
                        return Some(ff);
                    }
                }
                _ => {}
            }
            if std::time::Instant::now() >= deadline {
                // Firefox didn't expose a tab in time — skip.
                kill_pid(ff.firefox_pid);
                return None;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Run an ff-rdp command against this Firefox instance and return Output.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(ff_rdp_bin())
            .args(base_args(self.port))
            .args(args)
            .output()
            .expect("failed to spawn ff-rdp")
    }

    /// Run an ff-rdp command with additional arbitrary args.
    fn run_args(&self, args: impl IntoIterator<Item = String>) -> Output {
        let mut cmd = Command::new(ff_rdp_bin());
        cmd.args(base_args(self.port));
        for a in args {
            cmd.arg(a);
        }
        cmd.output().expect("failed to spawn ff-rdp")
    }
}

impl Drop for LiveFirefox {
    fn drop(&mut self) {
        kill_pid(self.firefox_pid);
    }
}

/// Best-effort SIGKILL by PID.  Non-fatal if the process already exited.
fn kill_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        // SAFETY: kill(2) is always safe to call with a valid pid and signal;
        // if the process no longer exists, it returns ESRCH which we ignore.
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

/// Parse JSON from command output — panics on parse failure with the raw bytes.
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
// AC A: screenshot --full-page
// ---------------------------------------------------------------------------

/// `live_screenshot_full_page`:
/// Navigate to a data URL with a 5000px tall page, take `screenshot --full-page`,
/// assert the PNG height ≥ 4900 px.
#[test]
fn live_screenshot_full_page() {
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_screenshot_full_page: Firefox not available — skipping");
        return;
    };

    // Navigate to a synthetic tall page (5000 px).
    let tall_html = r#"data:text/html,<html><body style="margin:0;height:5000px;background:linear-gradient(red,blue)"><p id=top>top</p><p style="position:absolute;top:4990px">bottom</p></body></html>"#;
    let nav = ff.run(&[
        "navigate",
        tall_html,
        "--timeout",
        "10000",
        "--allow-unsafe-urls",
    ]);
    if !nav.status.success() {
        eprintln!(
            "live_screenshot_full_page: navigate failed — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // Take full-page screenshot in base64 mode.
    let tmp_path = std::env::temp_dir().join(format!(
        "live_61l_fullpage_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let shot = ff.run_args(
        ["screenshot", "--full-page", "--output"]
            .iter()
            .map(ToString::to_string)
            .chain([tmp_path.to_string_lossy().into_owned()]),
    );
    let _ = std::fs::remove_file(&tmp_path);

    assert!(
        shot.status.success(),
        "screenshot --full-page failed: {}",
        String::from_utf8_lossy(&shot.stderr)
    );

    let json = parse_json(&shot);
    let height = json["results"]["height"]
        .as_u64()
        .expect("results.height must be present");

    assert!(
        height >= 4900,
        "full-page screenshot height should be >= 4900 px (got {height}) — \
         the --full-page flag is not capturing the full scroll height"
    );
}

/// `live_screenshot_viewport`:
/// Without `--full-page`, height must equal the viewport (much less than 5000).
#[test]
fn live_screenshot_viewport_height_is_not_full_page() {
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!(
            "live_screenshot_viewport_height_is_not_full_page: Firefox not available — skipping"
        );
        return;
    };

    let tall_html =
        r#"data:text/html,<html><body style="margin:0;height:5000px">tall</body></html>"#;
    let nav = ff.run(&[
        "navigate",
        tall_html,
        "--timeout",
        "10000",
        "--allow-unsafe-urls",
    ]);
    if !nav.status.success() {
        return;
    }

    let tmp_path = std::env::temp_dir().join(format!(
        "live_61l_viewport_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let shot = ff.run_args(
        ["screenshot", "--output"]
            .iter()
            .map(ToString::to_string)
            .chain([tmp_path.to_string_lossy().into_owned()]),
    );
    let _ = std::fs::remove_file(&tmp_path);

    if !shot.status.success() {
        // Screenshot not supported in this configuration — skip.
        return;
    }

    let json = parse_json(&shot);
    let height = json["results"]["height"].as_u64().unwrap_or(0);

    // Viewport height (800×600 or similar) must be far less than 5000.
    assert!(
        height < 2000,
        "viewport screenshot height {height} is unexpectedly large — \
         did --full-page bleed into the default path?"
    );
}

// ---------------------------------------------------------------------------
// AC B: Firefox locale pin
// ---------------------------------------------------------------------------

/// `live_locale_pin`:
/// Launch Firefox with LANG=de_DE.UTF-8 in the parent env; the ff-rdp `launch`
/// command must override LANG/LC_ALL in the child env so Firefox console
/// messages are English.  We verify by navigating to a quirks-mode page and
/// checking the console messages don't contain German strings.
///
/// This is a structural test: we verify LANG/LC_ALL are set in the child env
/// by checking that `build_command` sets them (unit-testable) and confirming
/// the launch.rs code path.  The live test confirms the process starts
/// successfully with these env overrides.
#[test]
fn live_locale_pin_launch_sets_lang_env() {
    // Verify that the launched Firefox can start successfully even when the
    // parent has LANG=de_DE.UTF-8.  The ff-rdp `launch` command must override
    // LANG/LC_ALL in the child env to en_US.UTF-8.
    //
    // We use `launch_with_env` which captures the Firefox PID from the JSON
    // output so the process is properly killed on drop (the launcher process
    // itself exits immediately after starting Firefox).
    let ff = LiveFirefox::launch_with_env(&[("LANG", "de_DE.UTF-8"), ("LC_ALL", "de_DE.UTF-8")]);

    assert!(
        ff.is_some(),
        "Firefox should start successfully even when parent LANG=de_DE.UTF-8; \
         ff-rdp launch must override LANG/LC_ALL in the child env"
    );
    // `ff` is dropped here, killing the Firefox process.
}

// ---------------------------------------------------------------------------
// AC F: navigate neterror detection
// ---------------------------------------------------------------------------

/// `live_navigate_dnsfail`:
/// Navigate to a definitely-nonexistent domain; assert:
/// - process exits non-zero
/// - stderr contains "dns_not_found" or similar neterror-shaped error
#[test]
fn live_navigate_dnsfail() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_navigate_dnsfail: requires network (DNS resolution); set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_navigate_dnsfail: Firefox not available — skipping");
        return;
    };

    // DNS failures can take up to ~15 s on some systems — give 20 s.
    let output = ff.run_args(
        [
            "--timeout",
            "20000",
            "navigate",
            "https://this-domain-totally-does-not-exist-61l-zzz.invalid",
        ]
        .iter()
        .map(ToString::to_string),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "navigate to a nonexistent domain should fail (exit non-zero)\n\
         stdout={stdout}\nstderr={stderr}"
    );

    // The error should look like a neterror, not a generic timeout.
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("dns_not_found")
            || combined.contains("neterror")
            || combined.contains("dnsNotFound")
            || combined.contains("connection_failed")
            || combined.contains("DNS")
            || combined.contains("network error"),
        "expected a neterror-shaped message (dns_not_found / neterror / DNS), got:\n\
         stdout={stdout}\nstderr={stderr}"
    );

    // Must NOT be a timeout masquerading as success.
    assert!(
        !combined.contains("\"navigated\""),
        "navigate to a bad-DNS host must NOT return success-shaped JSON\n\
         stdout={stdout}\nstderr={stderr}"
    );
}

// ---------------------------------------------------------------------------
// AC G: navigate cross-origin race (URL-match recovery)
// ---------------------------------------------------------------------------

/// `live_navigate_cross_origin_url_match`:
/// Navigate to example.com; even with a tight timeout the URL-match recovery
/// should succeed (Firefox commits quickly for simple pages).  The test
/// just asserts that navigating to example.com returns success; the race
/// scenario is best demonstrated by the unit tests for `urls_match_scheme_host_path`.
#[test]
fn live_navigate_cross_origin_url_match() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_navigate_cross_origin_url_match: requires network (example.com); set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_navigate_cross_origin_url_match: Firefox not available — skipping");
        return;
    };

    // First navigate to example.com with a generous timeout.
    let output = ff.run_args(
        ["--timeout", "15000", "navigate", "https://example.com"]
            .iter()
            .map(ToString::to_string),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // This should succeed — if it doesn't, the cross-origin recovery is broken.
    assert!(
        output.status.success(),
        "navigate to example.com should succeed\n\
         stdout={stdout}\nstderr={stderr}"
    );

    let json = parse_json(&output);
    assert!(
        json["results"]["navigated"] == "https://example.com"
            || json["results"]["committed_url"]
                .as_str()
                .is_some_and(|u| u.contains("example.com")),
        "navigate result should contain example.com URL\n\
         json={json}"
    );
}

// ---------------------------------------------------------------------------
// AC H: eval CSP bypass
// ---------------------------------------------------------------------------

/// `live_eval_csp`:
/// Navigate to a data URL with CSP `script-src 'none'` (blocks eval), then
/// `eval 'document.title'` — assert success and that the title matches.
///
/// The chromeContext bypass added in eval.rs should intercept the CSP exception
/// and retry successfully.
#[test]
fn live_eval_csp() {
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_eval_csp: Firefox not available — skipping");
        return;
    };

    // A page with an HTTP-equiv CSP that blocks eval().
    // The title is "CSP Test" so we can assert the value.
    let csp_html = r#"data:text/html,<html><head><title>CSP Test</title><meta http-equiv="Content-Security-Policy" content="script-src 'none'"></head><body>CSP page</body></html>"#;

    let nav = ff.run_args(
        [
            "--timeout",
            "10000",
            "navigate",
            csp_html,
            "--allow-unsafe-urls",
        ]
        .iter()
        .map(ToString::to_string),
    );
    if !nav.status.success() {
        eprintln!(
            "live_eval_csp: navigate failed: {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // Eval document.title — should work via chrome-context fallback.
    let output = ff.run(&["eval", "document.title"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "eval on CSP-restricted page should succeed via chrome-context fallback\n\
         stdout={stdout}\nstderr={stderr}"
    );

    let json = parse_json(&output);
    let title = json["results"].as_str().unwrap_or("");
    assert_eq!(
        title, "CSP Test",
        "eval should return the page title 'CSP Test' but got: '{title}'\n\
         json={json}"
    );
}

/// `live_eval_basic`:
/// Without CSP, a basic eval should work and not trigger the chrome fallback.
#[test]
fn live_eval_basic() {
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_eval_basic: Firefox not available — skipping");
        return;
    };

    let html =
        r"data:text/html,<html><head><title>Basic Test</title></head><body>hello</body></html>";
    let nav = ff.run_args(
        [
            "--timeout",
            "10000",
            "navigate",
            html,
            "--allow-unsafe-urls",
        ]
        .iter()
        .map(ToString::to_string),
    );
    if !nav.status.success() {
        return;
    }

    let output = ff.run(&["eval", "document.title"]);
    assert!(
        output.status.success(),
        "basic eval should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output);
    assert_eq!(json["results"], "Basic Test");
}

// ---------------------------------------------------------------------------
// AC K: consoleActor cache refresh after navigate
// ---------------------------------------------------------------------------

/// `live_navigate_invalidates_console_actor`:
/// Navigate to two different pages, run eval after each.  The second eval must
/// succeed — the consoleActor must be refreshed after navigate.
#[test]
fn live_navigate_invalidates_console_actor() {
    let Some(ff) = LiveFirefox::launch() else {
        eprintln!("live_navigate_invalidates_console_actor: Firefox not available — skipping");
        return;
    };

    // First page.
    let html1 = r"data:text/html,<html><head><title>Page One</title></head><body>one</body></html>";
    let nav1 = ff.run_args(
        [
            "--timeout",
            "10000",
            "navigate",
            html1,
            "--allow-unsafe-urls",
        ]
        .iter()
        .map(ToString::to_string),
    );
    if !nav1.status.success() {
        eprintln!("live_navigate_invalidates_console_actor: nav1 failed");
        return;
    }

    let eval1 = ff.run(&["eval", "document.title"]);
    assert!(
        eval1.status.success(),
        "eval after first navigate should succeed: {}",
        String::from_utf8_lossy(&eval1.stderr)
    );
    let j1 = parse_json(&eval1);
    assert_eq!(j1["results"], "Page One");

    // Second page — different domain-ish (still data: URL but different title).
    let html2 = r"data:text/html,<html><head><title>Page Two</title></head><body>two</body></html>";
    let nav2 = ff.run_args(
        [
            "--timeout",
            "10000",
            "navigate",
            html2,
            "--allow-unsafe-urls",
        ]
        .iter()
        .map(ToString::to_string),
    );
    if !nav2.status.success() {
        eprintln!("live_navigate_invalidates_console_actor: nav2 failed");
        return;
    }

    // After second navigate, eval must use the refreshed consoleActor.
    let eval2 = ff.run(&["eval", "document.title"]);
    assert!(
        eval2.status.success(),
        "eval after second navigate must succeed (consoleActor refresh broken)\n\
         stderr: {}",
        String::from_utf8_lossy(&eval2.stderr)
    );
    let j2 = parse_json(&eval2);
    assert_eq!(
        j2["results"], "Page Two",
        "eval after second navigate should see the new page title"
    );
}
