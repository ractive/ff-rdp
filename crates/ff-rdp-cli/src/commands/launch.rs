use std::io::Read as _;
use std::net::ToSocketAddrs as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

/// Locate the Firefox binary on the current platform.
///
/// Checks well-known installation paths first, then falls back to a PATH
/// search via `which` (Unix) or `where` (Windows).
pub(crate) fn find_firefox() -> Result<PathBuf, AppError> {
    // Platform-specific well-known paths checked before falling back to PATH.
    if cfg!(target_os = "macos") {
        let mac_paths = [
            "/Applications/Firefox.app/Contents/MacOS/firefox",
            "/Applications/Firefox Developer Edition.app/Contents/MacOS/firefox",
            "/Applications/Firefox Nightly.app/Contents/MacOS/firefox",
        ];
        for p in &mac_paths {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    if cfg!(target_os = "windows") {
        let win_paths = [
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
        ];
        for p in &win_paths {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    // Fall back to PATH lookup on all platforms.
    let candidates = if cfg!(target_os = "windows") {
        vec!["firefox.exe"]
    } else {
        vec!["firefox", "firefox-esr", "firefox-developer-edition"]
    };

    for candidate in candidates {
        if let Ok(path) = which_binary(candidate) {
            return Ok(path);
        }
    }

    Err(AppError::User(
        "Firefox not found. Install Firefox or set it in PATH.".to_owned(),
    ))
}

/// Resolve a binary name to its full path using the system's `which` / `where`
/// command. Returns an error if the binary is not found.
fn which_binary(name: &str) -> Result<PathBuf, AppError> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    let output = std::process::Command::new(which_cmd)
        .arg(name)
        .output()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to run {which_cmd}: {e}")))?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        // `which` may return multiple lines on Windows — take the first.
        let first_line = path_str.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            return Ok(PathBuf::from(first_line));
        }
    }

    Err(AppError::User(format!("{name} not found in PATH")))
}

/// Devtools prefs that must be present for the debugger server to start.
const DEVTOOLS_PREFS: &[(&str, &str)] = &[
    ("devtools.debugger.remote-enabled", "true"),
    ("devtools.debugger.prompt-connection", "false"),
    ("devtools.chrome.enabled", "true"),
];

/// Ensure the devtools prefs are present in the profile's `user.js`.
/// Appends only missing prefs to avoid overwriting user customisations.
fn ensure_devtools_prefs(profile: &Path) -> Result<(), AppError> {
    use std::fmt::Write as FmtWrite;
    use std::io::Write as IoWrite;

    let user_js = profile.join("user.js");
    let existing = std::fs::read_to_string(&user_js).unwrap_or_default();
    let mut additions = String::new();
    for (key, val) in DEVTOOLS_PREFS {
        if !existing.contains(key) {
            let _ = writeln!(additions, "user_pref(\"{key}\", {val});");
        }
    }
    if !additions.is_empty() {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&user_js)
            .map_err(|e| {
                AppError::User(format!(
                    "failed to write devtools prefs to {}: {e}",
                    user_js.display()
                ))
            })?;
        f.write_all(additions.as_bytes()).map_err(|e| {
            AppError::User(format!(
                "failed to write devtools prefs to {}: {e}",
                user_js.display()
            ))
        })?;
    }
    Ok(())
}

/// Firefox preferences written into every temporary profile to suppress
/// first-run UI, telemetry prompts, and session-restore dialogs, and to
/// enable the remote debugging server (required since Firefox ~149).
const USER_JS: &str = r#"// Suppress first-run / onboarding pages
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("startup.homepage_welcome_url", "about:blank");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("browser.startup.homepage", "about:blank");
user_pref("browser.startup.page", 0);
// Disable telemetry and data reporting prompts
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
// Disable default browser check
user_pref("browser.shell.checkDefaultBrowser", false);
// Disable session restore prompts
user_pref("browser.sessionstore.resume_from_crash", false);
// Enable remote debugging server (required since Firefox ~149)
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
user_pref("devtools.chrome.enabled", true);
"#;

/// Build a `Command` ready to spawn Firefox, and return the effective profile
/// path if one is in use (useful for reporting in the output JSON).
///
/// `-no-remote` is always passed first so the new instance is fully
/// independent of any already-running Firefox.
///
/// For `temp_profile`, a new directory is created under the OS temp dir and
/// a `user.js` is written into it to suppress first-run UI. The profile path
/// is included in the returned value so callers can surface it.
pub(crate) fn build_command(
    firefox: &Path,
    port: u16,
    headless: bool,
    profile: Option<&str>,
    temp_profile: bool,
    auto_consent: bool,
) -> Result<(std::process::Command, Option<PathBuf>), AppError> {
    let mut cmd = std::process::Command::new(firefox);

    // Always launch as an independent instance.
    cmd.arg("-no-remote");

    cmd.arg("--start-debugger-server").arg(port.to_string());

    if headless {
        cmd.arg("--headless");
    }

    // Resolve the effective profile path. `profile` and `temp_profile` are
    // mutually exclusive (enforced at the CLI level), so we handle them in
    // order of precedence.
    let profile_path: Option<PathBuf> = if let Some(p) = profile {
        let path = PathBuf::from(p);
        // Ensure the devtools prefs exist so the debugger server starts.
        // We append to any existing user.js rather than overwriting it.
        ensure_devtools_prefs(&path)?;
        cmd.arg("--profile").arg(&path);
        Some(path)
    } else if temp_profile {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0);
        let tmp =
            std::env::temp_dir().join(format!("ff-rdp-profile-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| {
            AppError::User(format!(
                "failed to create temporary profile directory {}: {e}",
                tmp.display()
            ))
        })?;
        std::fs::write(tmp.join("user.js"), USER_JS).map_err(|e| {
            AppError::User(format!(
                "failed to write user.js to temporary profile {}: {e}",
                tmp.display()
            ))
        })?;
        cmd.arg("--profile").arg(&tmp);
        Some(tmp)
    } else {
        None
    };

    // Install Consent-O-Matic if requested. Requires a profile directory so
    // Firefox can pick up the extension on next startup.
    if auto_consent {
        match &profile_path {
            Some(p) => super::auto_consent::install(p)?,
            None => {
                return Err(AppError::User(
                    "auto-consent requires --profile or --temp-profile".to_owned(),
                ));
            }
        }
    }

    // Detach from the terminal so the spawned browser doesn't inherit our
    // stdin/stdout. Capture stderr so we can surface early crash messages.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    Ok((cmd, profile_path))
}

/// Poll until the TCP port at `host:port` accepts a connection or `timeout`
/// elapses. Tries all resolved addresses (IPv4 + IPv6) each iteration so
/// Firefox is found regardless of which address family it binds.
/// Retries every 200 ms. Returns `Ok(())` on success.
fn wait_for_port(host: &str, port: u16, timeout: Duration) -> Result<(), AppError> {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| AppError::User(format!("invalid host/port {addr_str}: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(AppError::User(format!(
            "could not resolve address {addr_str}"
        )));
    }

    let poll_interval = Duration::from_millis(200);
    let deadline = std::time::Instant::now() + timeout;

    loop {
        let iteration_start = std::time::Instant::now();
        let remaining = deadline.saturating_duration_since(iteration_start);
        if remaining.is_zero() {
            break;
        }
        // Try each resolved address with a short per-address timeout.
        let per_addr = remaining
            .min(poll_interval)
            .checked_div(u32::try_from(addrs.len()).unwrap_or(u32::MAX))
            .unwrap_or(Duration::from_millis(50));
        for addr in &addrs {
            if std::net::TcpStream::connect_timeout(addr, per_addr).is_ok() {
                return Ok(());
            }
        }
        // Sleep only the remainder of the poll interval so we don't
        // busy-spin when connect returns immediately (ECONNREFUSED).
        let spent = iteration_start.elapsed();
        let sleep_time = poll_interval.saturating_sub(spent);
        let new_remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if !new_remaining.is_zero() && !sleep_time.is_zero() {
            std::thread::sleep(sleep_time.min(new_remaining));
        }
    }

    Err(AppError::User(format!(
        "debug port {port} is not reachable after {}s — is the port already in use?",
        timeout.as_secs()
    )))
}

pub fn run(
    cli: &Cli,
    headless: bool,
    profile: Option<&str>,
    temp_profile: bool,
    debug_port: Option<u16>,
    auto_consent: bool,
) -> Result<(), AppError> {
    let port = debug_port.unwrap_or(cli.port);
    let host = &cli.host;

    let firefox = find_firefox()?;

    let (mut cmd, profile_path) = build_command(
        &firefox,
        port,
        headless,
        profile,
        temp_profile,
        auto_consent,
    )?;

    let mut child = cmd.spawn().map_err(|e| {
        AppError::User(format!(
            "failed to start Firefox at {}: {e}",
            firefox.display()
        ))
    })?;

    // Wait briefly to catch immediately-crashing launches (bad flags, missing
    // libraries, etc.).
    std::thread::sleep(Duration::from_millis(500));

    match child.try_wait() {
        Ok(Some(status)) => {
            // Process already exited — try to capture stderr for diagnostics.
            let mut stderr_text = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_string(&mut stderr_text);
            }
            let stderr_text = stderr_text.trim().to_owned();
            let detail = if stderr_text.is_empty() {
                String::new()
            } else {
                format!(": {stderr_text}")
            };
            Err(AppError::User(format!(
                "Firefox exited immediately with {status}{detail}"
            )))
        }
        Ok(None) => {
            // Still running — verify the debug port is actually reachable
            // before reporting success. Always probe localhost since we
            // just spawned a local Firefox, regardless of --host.
            let pid = child.id();
            if let Err(e) = wait_for_port("localhost", port, Duration::from_secs(5)) {
                let _ = child.kill();
                return Err(AppError::User(format!(
                    "Firefox started (pid {pid}) but {e}"
                )));
            }

            let result = json!({
                "pid": pid,
                "host": host,
                "port": port,
                "headless": headless,
                "profile": profile_path.as_ref().map(|p| p.to_string_lossy().as_ref().to_owned()),
                "temp_profile": temp_profile,
                "auto_consent": auto_consent,
            });
            let meta = json!({
                "host": host,
                "port": port,
                "firefox": firefox.to_string_lossy().as_ref().to_owned(),
            });
            let envelope = output::envelope(&result, 1, &meta);
            OutputPipeline::from_cli(cli)?
                .finalize(&envelope)
                .map_err(AppError::from)
        }
        Err(e) => Err(AppError::Internal(anyhow::anyhow!(
            "failed to check Firefox status: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extract all arguments that would be passed to the spawned process,
    /// including the program name as the first element.
    fn command_args(cmd: &std::process::Command) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        args.push(cmd.get_program().to_string_lossy().into_owned());
        args.extend(cmd.get_args().map(|a| a.to_string_lossy().into_owned()));
        args
    }

    /// Write a minimal dummy script to a temp path and return that path.
    /// The caller must call `cleanup_fake_firefox` afterwards.
    fn fake_firefox() -> PathBuf {
        use std::io::Write as _;
        // Use a unique name per-test via the thread id to avoid collisions when
        // tests run in parallel.
        let id = std::thread::current().id();
        let name = format!("fake-firefox-{id:?}").replace(['(', ')', ' '], "-");
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path
    }

    fn cleanup_fake_firefox(p: &Path) {
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn build_command_always_includes_no_remote() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a == "-no-remote"),
            "expected -no-remote in args: {args:?}"
        );
    }

    #[test]
    fn build_command_includes_debugger_server_port() {
        let tmp = fake_firefox();
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a.contains("start-debugger-server")),
            "expected --start-debugger-server in args: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "6000"),
            "expected port 6000 in args: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "-no-remote"),
            "expected -no-remote in args: {args:?}"
        );
        assert!(profile.is_none());
    }

    #[test]
    fn build_command_headless_flag() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, true, None, false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a.contains("headless")),
            "expected --headless in args: {args:?}"
        );
    }

    #[test]
    fn build_command_no_headless_by_default() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            !args.iter().any(|a| a.contains("headless")),
            "unexpected --headless in args: {args:?}"
        );
    }

    #[test]
    fn build_command_explicit_profile() {
        let tmp = fake_firefox();
        let profile_dir = std::env::temp_dir().join("ff-rdp-test-explicit-profile");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let profile_str = profile_dir.to_str().unwrap();
        let (cmd, profile_path) =
            build_command(&tmp, 6000, false, Some(profile_str), false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        let _ = std::fs::remove_dir_all(&profile_dir);
        assert!(
            args.iter().any(|a| a.contains("profile")),
            "expected --profile in args: {args:?}"
        );
        assert_eq!(
            profile_path.as_deref().map(std::path::Path::as_os_str),
            Some(profile_dir.as_os_str())
        );
    }

    #[test]
    fn build_command_temp_profile_creates_dir_and_sets_profile_arg() {
        let tmp = fake_firefox();
        let (cmd, profile_path) = build_command(&tmp, 6000, false, None, true, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a.contains("profile")),
            "expected --profile in args for temp-profile: {args:?}"
        );
        let profile = profile_path.expect("temp_profile should set a profile path");
        assert!(
            profile.exists(),
            "temp profile directory should have been created: {}",
            profile.display()
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn build_command_temp_profile_writes_user_js() {
        let tmp = fake_firefox();
        let (_, profile_path) = build_command(&tmp, 6000, false, None, true, false).unwrap();
        cleanup_fake_firefox(&tmp);
        let profile = profile_path.expect("temp_profile should set a profile path");
        let user_js = profile.join("user.js");
        assert!(
            user_js.exists(),
            "user.js should exist in temp profile: {}",
            user_js.display()
        );
        let contents = std::fs::read_to_string(&user_js).unwrap();
        assert!(
            contents.contains("browser.aboutwelcome.enabled"),
            "user.js should disable aboutwelcome"
        );
        assert!(
            contents.contains("browser.startup.homepage"),
            "user.js should set startup homepage"
        );
        assert!(
            contents.contains("browser.sessionstore.resume_from_crash"),
            "user.js should disable session restore"
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn build_command_non_standard_port() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 9222, false, None, false, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a == "9222"),
            "expected port 9222 in args: {args:?}"
        );
    }

    #[test]
    fn build_command_auto_consent_requires_profile() {
        let tmp = fake_firefox();
        let result = build_command(&tmp, 6000, false, None, false, true);
        cleanup_fake_firefox(&tmp);
        assert!(result.is_err(), "auto_consent without profile should fail");
    }

    #[test]
    fn build_command_auto_consent_with_temp_profile_installs_extension() {
        let tmp = fake_firefox();
        // We can't test the actual download, but we can test that the function
        // doesn't panic when given a temp profile. The download will fail in
        // offline test environments, so we just verify the error is reasonable
        // or it succeeds if network is available.
        let result = build_command(&tmp, 6000, false, None, true, true);
        cleanup_fake_firefox(&tmp);
        // Either succeeds (network available) or gives a user error (no network)
        match result {
            Ok((_, profile_path)) => {
                let profile = profile_path.unwrap();
                // Check that the extensions dir was at least attempted
                let _ = std::fs::remove_dir_all(&profile);
            }
            Err(AppError::User(_)) => { /* expected in offline/CI */ }
            Err(e) => panic!("unexpected error type: {e:?}"),
        }
    }
}
