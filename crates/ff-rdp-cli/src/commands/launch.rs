use std::io::Read as _;
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

/// Build a `Command` ready to spawn Firefox, and return the effective profile
/// path if one is in use (useful for reporting in the output JSON).
///
/// For `temp_profile`, a new directory is created under the OS temp dir.
/// Its path is included in the output so the caller knows where it is.
pub(crate) fn build_command(
    firefox: &Path,
    port: u16,
    headless: bool,
    profile: Option<&str>,
    temp_profile: bool,
) -> Result<(std::process::Command, Option<PathBuf>), AppError> {
    let mut cmd = std::process::Command::new(firefox);

    cmd.arg("--start-debugger-server").arg(port.to_string());

    if headless {
        cmd.arg("--headless");
    }

    // Resolve the effective profile path. `profile` and `temp_profile` are
    // mutually exclusive (enforced at the CLI level), so we handle them in
    // order of precedence.
    let profile_path: Option<PathBuf> = if let Some(p) = profile {
        let path = PathBuf::from(p);
        cmd.arg("--profile").arg(&path);
        Some(path)
    } else if temp_profile {
        let tmp = std::env::temp_dir().join(format!("ff-rdp-profile-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| {
            AppError::User(format!(
                "failed to create temporary profile directory {}: {e}",
                tmp.display()
            ))
        })?;
        cmd.arg("--profile").arg(&tmp);
        Some(tmp)
    } else {
        None
    };

    // Detach from the terminal so the spawned browser doesn't inherit our
    // stdin/stdout. Capture stderr so we can surface early crash messages.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    Ok((cmd, profile_path))
}

pub fn run(
    cli: &Cli,
    headless: bool,
    profile: Option<&str>,
    temp_profile: bool,
    debug_port: Option<u16>,
) -> Result<(), AppError> {
    let port = debug_port.unwrap_or(cli.port);
    let host = &cli.host;

    let firefox = find_firefox()?;

    let (mut cmd, profile_path) = build_command(&firefox, port, headless, profile, temp_profile)?;

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
            // Still running — report connection details and leave it running.
            let result = json!({
                "pid": child.id(),
                "host": host,
                "port": port,
                "headless": headless,
                "profile": profile_path.as_ref().map(|p| p.to_string_lossy().as_ref().to_owned()),
                "temp_profile": temp_profile,
            });
            let meta = json!({
                "host": host,
                "port": port,
                "firefox": firefox.to_string_lossy().as_ref().to_owned(),
            });
            let envelope = output::envelope(&result, 1, &meta);
            OutputPipeline::new(cli.jq.clone())
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
    use std::ffi::OsStr;

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
    fn build_command_includes_debugger_server_port() {
        let tmp = fake_firefox();
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false).unwrap();
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
        assert!(profile.is_none());
    }

    #[test]
    fn build_command_headless_flag() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, true, None, false).unwrap();
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
        let (cmd, _) = build_command(&tmp, 6000, false, None, false).unwrap();
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
        let (cmd, profile_path) =
            build_command(&tmp, 6000, false, Some("/my/profile"), false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a.contains("profile")),
            "expected --profile in args: {args:?}"
        );
        assert!(
            args.iter()
                .any(|a| a.contains("my") || a.contains("profile")),
            "expected profile path in args: {args:?}"
        );
        assert_eq!(
            profile_path.as_deref().map(std::path::Path::as_os_str),
            Some(OsStr::new("/my/profile"))
        );
    }

    #[test]
    fn build_command_temp_profile_creates_dir_and_sets_profile_arg() {
        let tmp = fake_firefox();
        let (cmd, profile_path) = build_command(&tmp, 6000, false, None, true).unwrap();
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
    fn build_command_non_standard_port() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 9222, false, None, false).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a == "9222"),
            "expected port 9222 in args: {args:?}"
        );
    }
}
