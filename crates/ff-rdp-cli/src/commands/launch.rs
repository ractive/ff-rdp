use std::io::Read as _;
use std::net::ToSocketAddrs as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;
use crate::port_owner;

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

/// Open `<profile>/user.js` for appending, creating `profile` first if it does
/// not exist yet (iter-158 Theme E).
///
/// Pre-iter-158 both `user.js` writers opened the file without ever creating
/// its parent, so `launch --profile /does/not/exist/prof` failed with
/// `failed to write devtools prefs to …/user.js: No such file or directory`
/// before Firefox was ever spawned — a user pointing `--profile` at a path they
/// intend ff-rdp to populate got a filesystem errno instead of a profile.
///
/// Security: a user-supplied `--profile` directory is the user's own choice and
/// gets no owner-PID marker (see [`should_write_owner_marker`]), so creating it
/// is fine. The *leaf* is different: appending through a symlinked `user.js`
/// would let a same-UID process redirect our write to an arbitrary file, which
/// is the same-UID plant the managed temp-profile path defeats with
/// unpredictable directory names (see `build_command`). Refuse a symlinked leaf
/// rather than following it.
fn open_user_js_append(profile: &Path, what: &str) -> Result<std::fs::File, AppError> {
    std::fs::create_dir_all(profile).map_err(|e| {
        AppError::User(format!(
            "failed to create profile directory {}: {e}",
            profile.display()
        ))
    })?;

    let user_js = profile.join("user.js");
    if let Ok(meta) = std::fs::symlink_metadata(&user_js)
        && meta.file_type().is_symlink()
    {
        return Err(AppError::User(format!(
            "refusing to write {what} through a symlinked {} — \
             remove the symlink or point --profile at a real directory",
            user_js.display()
        )));
    }

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&user_js)
        .map_err(|e| {
            AppError::User(format!(
                "failed to write {what} to {}: {e}",
                user_js.display()
            ))
        })
}

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
        let mut f = open_user_js_append(profile, "devtools prefs")?;
        f.write_all(additions.as_bytes()).map_err(|e| {
            AppError::User(format!(
                "failed to write devtools prefs to {}: {e}",
                user_js.display()
            ))
        })?;
    }
    Ok(())
}

/// Ensure the `extensions.autoDisableScopes` pref is set to `0` in the
/// profile's `user.js` so that sideloaded extensions (installed via the
/// profile `extensions/` directory) are not auto-disabled by Firefox.
fn ensure_extension_autoinstall(profile: &Path) -> Result<(), AppError> {
    use std::io::Write as IoWrite;

    let user_js = profile.join("user.js");
    let existing = std::fs::read_to_string(&user_js).unwrap_or_default();
    if !existing.contains("extensions.autoDisableScopes") {
        let mut f = open_user_js_append(profile, "extension prefs")?;
        f.write_all(b"user_pref(\"extensions.autoDisableScopes\", 0);\n")
            .map_err(|e| {
                AppError::User(format!(
                    "failed to write extension prefs to {}: {e}",
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
// Disable auto-updates so Firefox cannot restart mid-session and break the RDP connection
user_pref("app.update.enabled", false);
// Enable remote debugging server (required since Firefox ~149)
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
user_pref("devtools.chrome.enabled", true);
// Pin UI language to English so console/error messages are predictable for LLM agents.
// Without this, Firefox picks up the OS locale which produces non-English stack traces,
// error descriptions, and DevTools messages that agents cannot reliably parse.
user_pref("intl.accept_languages", "en-US, en");
user_pref("intl.locale.requested", "en-US");
// Prevent Firefox from overriding the above locale pin with the OS locale.
user_pref("intl.locale.matchOS", false);
"#;

/// Build a `Command` ready to spawn Firefox, and return the effective profile
/// path if one is in use (useful for reporting in the output JSON).
///
/// `-no-remote` is always passed first so the new instance is fully
/// independent of any already-running Firefox.
///
/// When `profile` is `None`, a fresh temp profile is created under the OS
/// temp dir with a `user.js` that enables the remote debugger and suppresses
/// first-run UI. The profile path is included in the returned value so
/// callers can surface it.
pub(crate) fn build_command(
    firefox: &Path,
    port: u16,
    headless: bool,
    profile: Option<&str>,
    auto_consent: bool,
    window_size: Option<(u32, u32)>,
) -> Result<(std::process::Command, Option<PathBuf>), AppError> {
    let mut cmd = std::process::Command::new(firefox);

    // Always launch as an independent instance.
    cmd.arg("-no-remote");

    cmd.arg("--start-debugger-server").arg(port.to_string());

    if headless {
        cmd.arg("--headless");
    }

    // iter-133 Theme A: `-width`/`-height` are real Firefox window-feature
    // flags (not the headless-shell `--window-size` arg, which a
    // `--start-debugger-server` instance ignores — see
    // kb/research/viewport-emulation.md). Honored but clamped to a ~500px
    // live floor below that width; the caller (`run`) reports the requested
    // size and a below-floor warning in the envelope.
    if let Some((width, height)) = window_size {
        cmd.arg("-width").arg(width.to_string());
        cmd.arg("-height").arg(height.to_string());
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
    } else {
        // --temp-profile or no profile: create a fresh temporary profile with
        // devtools prefs so the debugger server actually starts.
        //
        // We use tempfile::Builder with 16 random bytes so the directory name
        // is unpredictable.  A predictable name like
        // `/tmp/ff-rdp-profile-{pid}-{micros}` would allow a same-UID
        // process to pre-create the directory and plant a malicious `user.js`
        // symlink that rides our `fs::write` to overwrite arbitrary files.
        //
        // `.keep()` persists the directory past this process's exit so
        // Firefox (a separate process) can keep reading it while it runs.
        // Cleanup happens in two places, not "on process exit":
        //   - `daemon stop` removes the *active* profile once the
        //     SIGTERM→SIGKILL→killpg ladder confirms Firefox is actually
        //     gone (see `crate::daemon::client::run_daemon_stop`, iter-96
        //     Theme A).
        //   - `prune_orphan_profiles` below removes *orphaned* siblings
        //     older than `FF_RDP_PROFILE_PRUNE_DAYS` to catch crashes,
        //     `kill -9`, and reboots that never reach `daemon stop`
        //     (iter-96 Theme B).
        // iter-75 H-1: place the temp profile under the per-user state
        // directory (`~/.local/state/ff-rdp/profiles` on Linux,
        // `~/Library/Application Support/ff-rdp/profiles` on macOS,
        // `%LOCALAPPDATA%\ff-rdp\profiles` on Windows) instead of the
        // world-writable system temp directory.  See
        // `crate::util::profile_dir::secure_profile_root` for the threat
        // model and Windows ACL rationale.
        let profile_root = crate::util::profile_dir::secure_profile_root()?;

        // iter-96 Theme B: prune stale orphan profile dirs before creating a
        // new one. Bounded (FF_RDP_PROFILE_PRUNE_MAX) so a large backlog
        // can't add latency to this launch — later launches pick up the
        // rest. Env vars are read here, not inside the helper, so the
        // helper stays unit-testable without env-var juggling.
        let prune_age_days: u64 = std::env::var("FF_RDP_PROFILE_PRUNE_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);
        let prune_max: usize = std::env::var("FF_RDP_PROFILE_PRUNE_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);
        let pruned = crate::util::profile_dir::prune_orphan_profiles(
            &profile_root,
            Duration::from_secs(prune_age_days.saturating_mul(24 * 60 * 60)),
            prune_max,
        );
        if !pruned.removed.is_empty() {
            tracing::debug!(
                "launch: pruned {} stale orphan profile dir(s) under {}",
                pruned.removed.len(),
                profile_root.display()
            );
        }

        let tmp = tempfile::Builder::new()
            .prefix("ff-rdp-profile-")
            .rand_bytes(16)
            .tempdir_in(&profile_root)
            .map_err(|e| {
                AppError::User(format!(
                    "failed to create temporary profile directory under {}: {e}",
                    profile_root.display()
                ))
            })?
            .keep();
        std::fs::write(tmp.join("user.js"), USER_JS).map_err(|e| {
            AppError::User(format!(
                "failed to write user.js to temporary profile {}: {e}",
                tmp.display()
            ))
        })?;
        cmd.arg("--profile").arg(&tmp);
        Some(tmp)
    };

    // Install Consent-O-Matic if requested. Requires a profile directory so
    // Firefox can pick up the extension on next startup.
    if auto_consent {
        // profile_path is always Some at this point (either explicit, temp, or
        // the auto-created profile from the else branch above).
        if let Some(p) = &profile_path {
            // Prevent Firefox from auto-disabling the sideloaded extension.
            ensure_extension_autoinstall(p)?;
            super::auto_consent::install(p)?;
        }
    }

    // Pin the locale for the child process so Firefox console/error messages
    // are in English regardless of the OS locale.  This makes error strings and
    // DevTools output predictable for LLM agents.  On Windows the LANG env var
    // is not meaningful (Windows uses code pages / ICU), but it is harmless to
    // set it there too.
    cmd.env("LANG", "en_US.UTF-8");
    cmd.env("LC_ALL", "en_US.UTF-8");

    // Detach from the terminal so the spawned browser doesn't inherit our
    // stdin/stdout. Capture stderr so we can surface early crash messages.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    // Put Firefox into its own process group (pgid = child pid) so that
    // `daemon stop`'s SIGTERM/SIGKILL on the process group does not blast
    // back up to the caller's shell. Without this, the pgid escalation
    // introduced in iter-95 Theme A would target whatever group launched
    // ff-rdp — including the user's interactive shell.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }

    Ok((cmd, profile_path))
}

// ---------------------------------------------------------------------------
// iter-158 Theme A: the launch port-wait bound
// ---------------------------------------------------------------------------

/// Environment variable overriding the post-spawn debug-port wait bound.
/// Value is whole seconds; a malformed or empty value falls back to
/// [`DEFAULT_PORT_WAIT`].
pub(crate) const LAUNCH_TIMEOUT_ENV: &str = "FF_RDP_LAUNCH_TIMEOUT_SECS";

/// Default bound `launch` waits for Firefox to open its debug port.
///
/// Pre-iter-158 this was a hardcoded `Duration::from_secs(5)`. Firefox was
/// measured binding its debug port at **7 s** under load on 2026-08-13
/// (`ff-rdp launch` failed 5/5 attempts at load average 6.8), so the 5 s bound
/// turned every contended launch into a failure — including inside the live
/// suite, where it surfaced as `live_153_replace_emits_single_envelope`
/// failing on a defect that had nothing to do with `--replace`.
///
/// 30 s mirrors the bound the *test harness* already used
/// (`tests/common/mod.rs::launch_wait_timeout`), which had this right since
/// iter-113. The global `--timeout` is deliberately **not** the source here:
/// it is a socket-operation deadline (`DEFAULT_TIMEOUT_MS = 10_000`) and at
/// 10 s would still be too small.
const DEFAULT_PORT_WAIT: Duration = Duration::from_secs(30);

/// Resolve the effective debug-port wait bound from the `--launch-timeout`
/// flag and the [`LAUNCH_TIMEOUT_ENV`] environment variable.
///
/// Precedence: flag → env → [`DEFAULT_PORT_WAIT`]. A malformed or empty env
/// value falls back to the default rather than erroring — a bad env var must
/// never break a launch.
///
/// Pure (both inputs are parameters) so the precedence rules are unit-testable
/// without mutating process-wide env, exactly as the harness's
/// `parse_launch_timeout` already is.
pub(crate) fn resolve_port_wait_bound(flag: Option<u64>, env: Option<&str>) -> Duration {
    if let Some(secs) = flag {
        return Duration::from_secs(secs);
    }
    match env.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => v
            .parse::<u64>()
            .map_or(DEFAULT_PORT_WAIT, Duration::from_secs),
        None => DEFAULT_PORT_WAIT,
    }
}

/// The result of waiting for Firefox to open its remote-debugging port.
///
/// Exists so the *deadline* failure and the *port already occupied* failure
/// can no longer share one message. Pre-iter-158 both collapsed into
/// `"debug port {port} is not reachable after 5s — is the port already in
/// use?"`, which blamed a port conflict for what is almost always the
/// opposite condition: the port is unbound and Firefox has not reached it yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortWaitOutcome {
    /// The port accepted a connection within the bound.
    Opened,
    /// The bound elapsed with the port still refusing connections.
    TimedOut,
    /// `host:port` could not be resolved at all — a configuration error, not a
    /// timing one.
    Unresolvable(String),
}

impl PortWaitOutcome {
    /// Map a non-[`Opened`](PortWaitOutcome::Opened) outcome onto the
    /// user-facing error. Returns `None` when the port opened.
    ///
    /// The deadline message names Firefox's own failure to bind and the knobs
    /// that raise the bound. It deliberately mentions neither "already in use"
    /// nor a hardcoded "5s" — an occupied port is rejected *before* the spawn
    /// by [`reject_if_port_occupied`], with its own message.
    pub(crate) fn into_error(self, pid: u32, port: u16, bound: Duration) -> Option<AppError> {
        match self {
            Self::Opened => None,
            Self::TimedOut => Some(AppError::User(format!(
                "Firefox (pid {pid}) did not open debug port {port} within {}s — \
                 raise --launch-timeout or set {LAUNCH_TIMEOUT_ENV}",
                bound.as_secs()
            ))),
            Self::Unresolvable(msg) => Some(AppError::User(msg)),
        }
    }
}

/// The injectable operations `launch` performs against the outside world.
///
/// Mirrors the `EscalationHooks` fn-pointer pattern in
/// `daemon::client` (`client.rs:63-96`): a struct of plain function pointers,
/// no dynamic dispatch, real implementations in [`LaunchHooks::real`] and
/// stubs in tests. It exists so the two failure branches Theme A splits apart
/// — port occupied before the spawn, and Firefox never binding after it — are
/// testable without a real Firefox.
pub(crate) struct LaunchHooks {
    /// Fast probe: does *anything* accept TCP on `port` right now?
    pub(crate) is_port_in_use: fn(u16) -> bool,
    /// Identify the process listening on `port`, if the OS query succeeds.
    pub(crate) find_listener: fn(u16) -> Option<port_owner::PortOwner>,
    /// Poll `host:port` until it accepts a connection or the bound elapses.
    pub(crate) probe_port: fn(&str, u16, Duration) -> PortWaitOutcome,
    /// Spawn the prepared Firefox command.
    pub(crate) spawn: fn(&mut std::process::Command) -> std::io::Result<std::process::Child>,
}

impl LaunchHooks {
    /// Production hooks that call the real helpers.
    pub(crate) fn real() -> Self {
        Self {
            is_port_in_use: port_owner::is_port_in_use,
            find_listener: |port| port_owner::find_listener(port).ok().flatten(),
            probe_port: wait_for_port,
            spawn: std::process::Command::spawn,
        }
    }
}

/// Reject a launch whose debug port is already held by another process,
/// **before** Firefox is spawned (iter-158 Theme A).
///
/// The message names the occupying process and PID so the user can act on it.
/// Contrast the post-spawn deadline path, which must never suggest a port
/// conflict — see [`PortWaitOutcome::into_error`].
fn reject_if_port_occupied(port: u16, hooks: &LaunchHooks) -> Result<(), AppError> {
    let owner = (hooks.find_listener)(port);
    // Suggest a nearby port that always differs from the conflicting one,
    // even at the u16 upper bound where +10 would overflow.
    let suggested = port
        .checked_add(10)
        .unwrap_or_else(|| port.saturating_sub(10));
    let detail = match &owner {
        Some(o) if !o.process_name.is_empty() => {
            format!("by {} (PID {})", o.process_name, o.pid)
        }
        Some(o) => format!("by PID {}", o.pid),
        None => "by another process".to_owned(),
    };
    Err(AppError::User(format!(
        "port {port} is already in use {detail} — pass --debug-port {suggested} to pick another, \
         pass --replace to stop the existing instance, \
         or run `ff-rdp doctor` for a full report."
    )))
}

/// Poll until the TCP port at `host:port` accepts a connection or `timeout`
/// elapses. Tries all resolved addresses (IPv4 + IPv6) each iteration so
/// Firefox is found regardless of which address family it binds.
/// Retries every 200 ms.
fn wait_for_port(host: &str, port: u16, timeout: Duration) -> PortWaitOutcome {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = match addr_str.to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => {
            return PortWaitOutcome::Unresolvable(format!("invalid host/port {addr_str}: {e}"));
        }
    };
    if addrs.is_empty() {
        return PortWaitOutcome::Unresolvable(format!("could not resolve address {addr_str}"));
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
                return PortWaitOutcome::Opened;
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

    PortWaitOutcome::TimedOut
}

/// Whether `launch` should drop an [`OWNER_PID_MARKER`](crate::util::profile_dir::OWNER_PID_MARKER)
/// into the effective profile after a successful spawn.
///
/// `true` only for a managed (auto-created) profile — i.e. when no
/// `--profile <user-path>` was given. `build_command` creates a temp profile
/// under `secure_profile_root()` iff `profile.is_none()`, so that condition is
/// exactly the managed case. A user-supplied `--profile` directory is theirs
/// and must never receive a marker.
fn should_write_owner_marker(profile: Option<&str>) -> bool {
    profile.is_none()
}

/// Everything `launch` needs from the command line, bundled so the
/// hook-injected entry point ([`run_with_hooks`]) does not carry a nine-argument
/// signature.
pub(crate) struct LaunchOpts<'a> {
    pub(crate) headless: bool,
    pub(crate) profile: Option<&'a str>,
    pub(crate) temp_profile: bool,
    pub(crate) debug_port: Option<u16>,
    pub(crate) auto_consent: bool,
    pub(crate) replace: bool,
    pub(crate) window_size: Option<&'a str>,
    /// `--launch-timeout <secs>`: how long to wait for Firefox to open its
    /// debug port. See [`resolve_port_wait_bound`].
    pub(crate) launch_timeout: Option<u64>,
}

/// Launch Firefox with remote debugging.
///
/// `opts.replace` — if `true` and the port is already in use, stop the prior
/// instance before launching (implements `--replace` / `--force`).
pub(crate) fn run(cli: &Cli, opts: &LaunchOpts<'_>) -> Result<(), AppError> {
    run_with_hooks(cli, opts, &LaunchHooks::real())
}

/// [`run`] with the outside world injected — see [`LaunchHooks`].
pub(crate) fn run_with_hooks(
    cli: &Cli,
    opts: &LaunchOpts<'_>,
    hooks: &LaunchHooks,
) -> Result<(), AppError> {
    let &LaunchOpts {
        headless,
        profile,
        temp_profile,
        debug_port,
        auto_consent,
        replace,
        window_size,
        launch_timeout,
    } = opts;
    let port = debug_port.unwrap_or(cli.port);
    let host = &cli.host;

    // iter-158 Theme A: resolve the post-spawn debug-port wait bound up front
    // so it can be reported in the envelope (`meta.launch_wait_secs`) whether
    // or not the wait is ever reached.
    let port_wait_bound = resolve_port_wait_bound(
        launch_timeout,
        std::env::var(LAUNCH_TIMEOUT_ENV).ok().as_deref(),
    );

    // iter-142 Theme B: opportunistically sweep `~/.ff-rdp/` housekeeping
    // files on every `launch`, not just the rare daemon-autostart path
    // (`resolve_connection_target`'s spawn branch, which only runs when no
    // daemon is already up for the target port). Dogfooding session 63
    // observed stale spawn locks and throttle-state files accumulate for
    // exactly this reason: a session that reuses an already-running daemon
    // across every command never takes the spawn path at all, so the
    // existing iter-132 sweep never fires. `launch` is the one command every
    // session actually runs, so anchoring the sweep here gives it real
    // coverage. Each call is independently best-effort (see their own doc
    // comments) and must never fail or slow down the launch.
    crate::daemon::registry::gc_stale_spawn_locks();
    crate::daemon::registry::gc_legacy_spawn_lock();
    crate::daemon::throttle_state::gc_stale_throttle_states();

    // iter-133 Theme A: parse --window-size up front so a malformed value
    // fails fast, before any port-collision check or Firefox spawn.
    let window_size: Option<(u32, u32)> = window_size
        .map(crate::util::window_size::parse_window_size)
        .transpose()?;

    // Detect port collision before spawning Firefox. A new --start-debugger-server
    // <port> Firefox silently no-ops when the port is already held by another
    // listener, so we surface the conflict ourselves with a hint that points
    // at `doctor` for follow-up diagnosis.
    // iter-153: captured (not printed) here — `stop_prior_instance` no
    // longer prints its own envelope, since doing so wrote a second
    // top-level JSON document to `launch --replace`'s stdout ahead of the
    // launch envelope below. Folded into this command's own `meta.replaced`
    // instead, so `launch --replace` always emits exactly one document and
    // `results.pid` always means the process THIS command started.
    let mut replaced: Option<crate::daemon::client::StopOutcome> = None;
    if (hooks.is_port_in_use)(port) {
        if replace {
            // --replace / --force: stop the prior instance gracefully, then proceed.
            replaced = Some(crate::daemon::client::stop_prior_instance(cli, port)?);
        } else {
            // iter-158 Theme A: the *pre-spawn* occupancy failure. This is the
            // only branch allowed to say "already in use", and it names the
            // occupying process so the user can act on it.
            reject_if_port_occupied(port, hooks)?;
        }
    }

    let firefox = find_firefox()?;

    let (mut cmd, profile_path) =
        build_command(&firefox, port, headless, profile, auto_consent, window_size)?;

    let mut child = (hooks.spawn)(&mut cmd).map_err(|e| {
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
            let outcome = (hooks.probe_port)("localhost", port, port_wait_bound);
            if let Some(e) = outcome.into_error(pid, port, port_wait_bound) {
                let _ = child.kill();
                return Err(e);
            }

            // iter-97 Theme A: drop an owner-PID marker into the managed temp
            // profile so the prune paths can positively confirm this Firefox
            // is still alive before any age-based deletion. Only managed
            // (auto-created) profiles get the marker — a `--profile <path>`
            // dir the user owns must never receive one (see
            // `should_write_owner_marker`). Warn-not-fail (handled inside the
            // helper): a marker write must never fail the launch.
            if should_write_owner_marker(profile)
                && let Some(dir) = profile_path.as_deref()
            {
                crate::util::profile_dir::write_owner_pid_marker(dir, pid);

                // iter-151 Theme A: if the caller identifies itself (the
                // live-test harness sets this env var on every `LiveFirefox`
                // launch — see `tests/common/mod.rs`), record it alongside
                // the owner PID so a leaked profile can be traced back to
                // the exact test that spawned it, instead of a bisection
                // hunt across ~200 live tests. Absent for a normal
                // interactive `ff-rdp launch` — no marker is written.
                if let Ok(test_name) = std::env::var(crate::util::profile_dir::SPAWNING_TEST_ENV)
                    && !test_name.trim().is_empty()
                {
                    crate::util::profile_dir::write_owner_test_marker(dir, test_name.trim());
                }
            }

            // Write the shared daemon record so `daemon stop` and
            // `launch --replace` can find and terminate this instance.
            // `launch` is fire-and-forget (it spawns Firefox and returns), so
            // no Ctrl-C cleanup is needed here — the record is cleaned up by
            // whichever stop path runs next.
            let daemon_rec = crate::daemon_record::DaemonRecord {
                pid,
                port,
                headless,
                launched_at: chrono::Utc::now(),
                profile_dir: profile_path.clone().unwrap_or_default(),
            };
            if let Err(e) = crate::daemon_record::write(&daemon_rec) {
                // stderr-ok: (b) warn-and-continue — launch still succeeds,
                // just without a `daemon stop` handle for this instance.
                eprintln!("warning: could not write daemon record: {e:#}");
            }

            // `temp_profile` is true when the caller requested --temp-profile
            // OR when we auto-created one because no profile flag was given.
            let effective_temp_profile = temp_profile || profile.is_none();
            let profile_path_str = profile_path
                .as_ref()
                .map(|p| p.to_string_lossy().as_ref().to_owned());

            // iter-133 Theme A: report the requested window size (if any) and
            // whether it is below the documented ~500px live-viewport floor.
            // `below_floor` looks only at width — the floor is a width clamp,
            // not a height clamp (see kb/research/viewport-emulation.md).
            // Computed once here and reused below so the envelope's
            // `window_size.below_floor` and the presence of `warnings` can
            // never disagree.
            let below_floor = window_size
                .is_some_and(|(w, _)| w < crate::util::window_size::LIVE_VIEWPORT_FLOOR_PX);
            let window_size_json = window_size.map(|(w, h)| {
                json!({
                    "requested": {"width": w, "height": h},
                    "below_floor": below_floor,
                })
            });

            let mut result = json!({
                "pid": pid,
                "host": host,
                "port": port,
                "headless": headless,
                "profile": profile_path_str,
                // iter-96: explicit alias of "profile" so `daemon stop`'s
                // `profile_removed_path` can be compared against the same
                // field name (see live_daemon_stop_profile_path_matches_launch_json).
                "profile_path": profile_path_str,
                "temp_profile": effective_temp_profile,
                // iter-144 Theme C: renamed from "auto_consent" — `launch`
                // returns before any page loads, so this field can only
                // ever attest that the Consent-O-Matic extension was
                // *installed* into the profile, never that a consent
                // banner was actually dismissed (kb/iterations/
                // iteration-142-session-hygiene.md found `auto_consent:
                // true` reported while a banner still covered the page).
                // A real dismiss attestation lives in `results.consent`
                // from `navigate --auto-consent` / `consent accept`
                // (`{"cmp": ..., "action": ...}`, iter-129) — those run
                // after a page has loaded and can check the DOM.
                "auto_consent_extension_installed": auto_consent,
                "window_size": window_size_json,
            });
            if below_floor && let (Some((w, h)), Some(obj)) = (window_size, result.as_object_mut())
            {
                let floor = crate::util::window_size::LIVE_VIEWPORT_FLOOR_PX;
                obj.insert(
                    "warnings".to_owned(),
                    json!([format!(
                        "requested width {w}px (window-size {w}x{h}) is below the ~{floor}px \
                         live-viewport floor observed for a headless debugger-server Firefox \
                         instance; effective innerWidth typically clamps up to ~{floor}px \
                         (confirm with `ff-rdp eval innerWidth`). For a true sub-{floor}px \
                         raster, use `ff-rdp screenshot --window-size {w}x{h}` after navigating \
                         instead of relying on this live session's viewport."
                    )]),
                );
            }
            let mut meta = json!({
                "firefox": firefox.to_string_lossy().as_ref().to_owned(),
                // iter-158 Theme A: the effective debug-port wait bound, so a
                // caller can see which of --launch-timeout /
                // FF_RDP_LAUNCH_TIMEOUT_SECS / the 30 s default actually
                // applied. Reporting it makes the bound a real knob rather
                // than an invisible constant.
                "launch_wait_secs": port_wait_bound.as_secs(),
            });
            // iter-153: fold the --replace stop outcome into this envelope's
            // meta instead of `stop_prior_instance` printing a second
            // top-level JSON document. `replaced.pid` is the STOPPED
            // instance's PID — never to be confused with `results.pid`
            // above, which is always the instance THIS command launched.
            if let (Some(r), Some(obj)) = (replaced, meta.as_object_mut()) {
                obj.insert(
                    "replaced".to_owned(),
                    json!({"stopped": r.stopped, "pid": r.pid}),
                );
            }
            crate::connection_meta::merge_into_if_verbose(
                &mut meta,
                host,
                port,
                None,
                cli.is_verbose(),
            );
            let envelope = output::envelope(&result, 1, &meta);
            let hint_ctx = HintContext::new(HintSource::Launch);
            OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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

    /// AC: `unit_owner_pid_marker_written_only_for_managed_profiles` — the
    /// owner-PID marker is written only for a managed (auto-created) profile.
    /// A `--profile <user-path>` launch (`profile = Some(_)`) never triggers
    /// a marker write.
    #[test]
    fn unit_owner_pid_marker_written_only_for_managed_profiles() {
        // No --profile: managed temp profile → marker written.
        assert!(
            should_write_owner_marker(None),
            "an auto-created managed profile must receive an owner-PID marker"
        );
        // Explicit --profile: user-owned dir → never marked.
        assert!(
            !should_write_owner_marker(Some("/home/user/my-firefox-profile")),
            "a user --profile directory must never receive an owner-PID marker"
        );

        // End-to-end shape check: build_command with an explicit --profile
        // returns exactly that path and does NOT write a marker into it (the
        // marker write lives in run(), gated by should_write_owner_marker).
        let tmp = fake_firefox();
        let user_profile = std::env::temp_dir().join(format!(
            "ff-rdp-user-profile-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&user_profile).unwrap();
        let user_profile_str = user_profile.to_str().unwrap();
        let (_, returned) =
            build_command(&tmp, 6000, false, Some(user_profile_str), false, None).unwrap();
        cleanup_fake_firefox(&tmp);

        assert_eq!(returned.as_deref(), Some(user_profile.as_path()));
        assert!(
            !user_profile
                .join(crate::util::profile_dir::OWNER_PID_MARKER)
                .exists(),
            "build_command must not plant an owner-PID marker in a user --profile dir"
        );
        let _ = std::fs::remove_dir_all(&user_profile);
    }

    // -----------------------------------------------------------------------
    // iter-158 Theme A — the launch port-wait bound and its error text
    // -----------------------------------------------------------------------

    /// AC `unit_158_resolve_port_wait_bound`: the precedence rules for the
    /// debug-port wait bound. Flag beats env, env beats the 30 s default, and
    /// a malformed or empty env value falls back to the default rather than
    /// erroring — a bad env var must never break a launch.
    #[test]
    fn unit_158_resolve_port_wait_bound() {
        assert_eq!(
            resolve_port_wait_bound(None, None),
            Duration::from_secs(30),
            "neither flag nor env ⇒ the 30 s default (NOT the pre-158 hardcoded 5 s)"
        );
        assert_eq!(
            resolve_port_wait_bound(Some(45), None),
            Duration::from_secs(45)
        );
        assert_eq!(
            resolve_port_wait_bound(None, Some("7")),
            Duration::from_secs(7)
        );
        assert_eq!(
            resolve_port_wait_bound(Some(45), Some("7")),
            Duration::from_secs(45),
            "the --launch-timeout flag must beat FF_RDP_LAUNCH_TIMEOUT_SECS"
        );
        assert_eq!(
            resolve_port_wait_bound(None, Some("abc")),
            Duration::from_secs(30),
            "a non-numeric env value falls back to the default"
        );
        assert_eq!(
            resolve_port_wait_bound(None, Some("")),
            Duration::from_secs(30),
            "an empty env value falls back to the default"
        );
    }

    /// AC `unit_158_port_wait_error_names_bind_timeout`: with a prober that
    /// never connects, the resulting error blames Firefox's failure to bind —
    /// not a port conflict, and never the pre-158 hardcoded "5s".
    #[test]
    fn unit_158_port_wait_error_names_bind_timeout() {
        let hooks = LaunchHooks {
            probe_port: |_host, _port, _timeout| PortWaitOutcome::TimedOut,
            ..LaunchHooks::real()
        };
        let bound = resolve_port_wait_bound(Some(30), None);
        let outcome = (hooks.probe_port)("localhost", 6123, bound);
        let err = outcome
            .into_error(4242, 6123, bound)
            .expect("a TimedOut outcome must produce an error");
        let AppError::User(msg) = err else {
            panic!("expected AppError::User, got {err:?}");
        };
        assert!(
            msg.contains("did not open debug port"),
            "message must name the bind timeout: {msg:?}"
        );
        assert!(
            msg.contains("30s"),
            "message must carry the resolved bound in seconds: {msg:?}"
        );
        assert!(
            !msg.contains("already in use"),
            "the deadline path must NOT blame a port conflict: {msg:?}"
        );
        assert!(
            !msg.contains("after 5s"),
            "the 5 s bound is gone; no message may still quote it: {msg:?}"
        );
    }

    /// AC `unit_158_launch_rejects_occupied_port_before_spawn`: an occupied
    /// port fails immediately, naming the occupying process and PID, and
    /// Firefox is never spawned.
    #[test]
    fn unit_158_launch_rejects_occupied_port_before_spawn() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static SPAWNS: AtomicUsize = AtomicUsize::new(0);
        SPAWNS.store(0, Ordering::SeqCst);

        let hooks = LaunchHooks {
            is_port_in_use: |_port| true,
            find_listener: |_port| {
                Some(port_owner::PortOwner {
                    pid: 51234,
                    process_name: "nc".to_owned(),
                    uptime_s: None,
                })
            },
            probe_port: |_host, _port, _timeout| PortWaitOutcome::Opened,
            spawn: |_cmd| {
                SPAWNS.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other(
                    "the spawn hook must never be reached",
                ))
            },
        };

        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"])
            .expect("parse a bare `launch`");
        let opts = LaunchOpts {
            headless: true,
            profile: None,
            temp_profile: false,
            debug_port: Some(7107),
            auto_consent: false,
            replace: false,
            window_size: None,
            launch_timeout: None,
        };

        let err = run_with_hooks(&cli, &opts, &hooks).expect_err("an occupied port must fail");
        let AppError::User(msg) = err else {
            panic!("expected AppError::User, got {err:?}");
        };
        assert!(
            msg.contains("port 7107 is already in use by nc (PID 51234)"),
            "message must name the occupying process and PID: {msg:?}"
        );
        assert_eq!(
            SPAWNS.load(Ordering::SeqCst),
            0,
            "Firefox must not be spawned when the port is already occupied"
        );
    }

    /// AC `live_158_launch_creates_missing_profile_dir` (unit half): the
    /// `--profile` path is created rather than erroring with ENOENT, and the
    /// devtools prefs land in it. Theme E.
    #[test]
    fn unit_158_profile_dir_created_when_absent() {
        let root = std::env::temp_dir().join(format!(
            "ff-rdp-158-absent-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let nested = root.join("absent").join("prof");
        assert!(
            !nested.exists(),
            "precondition: the profile dir must not exist"
        );

        ensure_devtools_prefs(&nested).expect("a missing --profile directory must be created");

        let user_js = nested.join("user.js");
        assert!(user_js.exists(), "user.js should have been written");
        let contents = std::fs::read_to_string(&user_js).unwrap();
        assert!(
            contents.contains("devtools.debugger.remote-enabled"),
            "devtools prefs must be present: {contents:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The `user.js` leaf must not be followed when it is a symlink — a
    /// same-UID process could otherwise redirect our append to any file the
    /// user can write (Theme E's security note).
    #[cfg(unix)]
    #[test]
    fn unit_158_profile_user_js_symlink_is_refused() {
        let root = std::env::temp_dir().join(format!(
            "ff-rdp-158-symlink-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let victim = root.join("victim.txt");
        std::fs::write(&victim, "untouched").unwrap();
        std::os::unix::fs::symlink(&victim, root.join("user.js")).unwrap();

        let err = ensure_devtools_prefs(&root).expect_err("a symlinked user.js must be refused");
        let AppError::User(msg) = err else {
            panic!("expected AppError::User, got {err:?}");
        };
        assert!(
            msg.contains("symlinked"),
            "the refusal must say why: {msg:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "untouched",
            "the symlink target must not have been written through"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_command_always_includes_no_remote() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, None).unwrap();
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
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false, None).unwrap();
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
        // With no profile flags, an auto-created temp profile is returned.
        let profile = profile.expect("auto-created temp profile should be returned");
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn build_command_no_profile_auto_creates_temp_profile() {
        let tmp = fake_firefox();
        let (cmd, profile_path) = build_command(&tmp, 6000, false, None, false, None).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        let profile = profile_path.expect("auto-created temp profile should be returned");
        assert!(
            profile.exists(),
            "auto-created profile directory should exist: {}",
            profile.display()
        );
        let user_js = profile.join("user.js");
        assert!(
            user_js.exists(),
            "user.js should exist in auto-created profile"
        );
        let contents = std::fs::read_to_string(&user_js).unwrap();
        assert!(
            contents.contains("devtools.debugger.remote-enabled"),
            "devtools prefs should be present in auto-created profile"
        );
        assert!(
            args.iter().any(|a| a == "--profile"),
            "should pass --profile to Firefox: {args:?}"
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn build_command_headless_flag() {
        let tmp = fake_firefox();
        let (cmd, _) = build_command(&tmp, 6000, true, None, false, None).unwrap();
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
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, None).unwrap();
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
            build_command(&tmp, 6000, false, Some(profile_str), false, None).unwrap();
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
        let (cmd, profile_path) = build_command(&tmp, 6000, false, None, false, None).unwrap();
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
        let (_, profile_path) = build_command(&tmp, 6000, false, None, false, None).unwrap();
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
        let (cmd, _) = build_command(&tmp, 9222, false, None, false, None).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        assert!(
            args.iter().any(|a| a == "9222"),
            "expected port 9222 in args: {args:?}"
        );
    }

    #[test]
    fn build_command_window_size_forwards_width_and_height() {
        let tmp = fake_firefox();
        let (cmd, profile) =
            build_command(&tmp, 6000, true, None, false, Some((390, 844))).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        if let Some(p) = profile {
            let _ = std::fs::remove_dir_all(&p);
        }
        let width_idx = args.iter().position(|a| a == "-width");
        let height_idx = args.iter().position(|a| a == "-height");
        assert!(
            width_idx.is_some() && height_idx.is_some(),
            "expected -width and -height in args: {args:?}"
        );
        assert_eq!(
            args.get(width_idx.unwrap() + 1).map(String::as_str),
            Some("390"),
            "expected -width 390 in args: {args:?}"
        );
        assert_eq!(
            args.get(height_idx.unwrap() + 1).map(String::as_str),
            Some("844"),
            "expected -height 844 in args: {args:?}"
        );
    }

    #[test]
    fn build_command_no_window_size_omits_width_height_flags() {
        let tmp = fake_firefox();
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false, None).unwrap();
        let args = command_args(&cmd);
        cleanup_fake_firefox(&tmp);
        if let Some(p) = profile {
            let _ = std::fs::remove_dir_all(&p);
        }
        assert!(
            !args.iter().any(|a| a == "-width" || a == "-height"),
            "unexpected -width/-height in args when --window-size was not given: {args:?}"
        );
    }

    #[test]
    fn build_command_auto_consent_uses_auto_created_profile() {
        // auto_consent no longer requires an explicit profile flag: when neither
        // --profile nor --temp-profile is given, build_command auto-creates a
        // temp profile that Consent-O-Matic can be installed into.
        // The extension download may fail in CI (no network), so we accept both
        // Ok and a User-level error; we just verify it is not an Internal error.
        let tmp = fake_firefox();
        let result = build_command(&tmp, 6000, false, None, true, None);
        cleanup_fake_firefox(&tmp);
        match result {
            Ok((_, profile_path)) => {
                let profile = profile_path.expect("auto-created profile should be returned");
                let _ = std::fs::remove_dir_all(&profile);
            }
            Err(AppError::User(_)) => { /* expected in offline/CI */ }
            Err(e) => panic!("unexpected error type: {e:?}"),
        }
    }

    #[test]
    #[ignore = "may perform a real network download depending on cache state"]
    fn build_command_auto_consent_with_temp_profile_installs_extension() {
        let tmp = fake_firefox();
        // We can't test the actual download, but we can test that the function
        // doesn't panic when given a temp profile. The download will fail in
        // offline test environments, so we just verify the error is reasonable
        // or it succeeds if network is available.
        let result = build_command(&tmp, 6000, false, None, true, None);
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
