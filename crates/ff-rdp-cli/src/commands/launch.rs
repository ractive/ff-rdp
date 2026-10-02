mod language_initialization;
mod replace;
mod startup;

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
pub(super) fn open_user_js_append(profile: &Path, what: &str) -> Result<std::fs::File, AppError> {
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
pub(super) const USER_JS: &str = r#"// Suppress first-run / onboarding pages
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
    url: Option<&str>,
) -> Result<(std::process::Command, Option<PathBuf>), AppError> {
    // iter-175: every `?` below the temp-profile creation used to return past a
    // directory this function had already created on disk. Arming this guard
    // the moment the directory exists makes those returns remove it again; it
    // is disarmed only on the success return, where the caller takes over (see
    // `run`, which re-arms one of its own across the spawn).
    let mut managed_guard = crate::util::profile_dir::ManagedProfileGuard::disarmed();
    let mut cmd = browser_command(firefox, port, headless, window_size, None);

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
        // Cleanup is not "on process exit": `prune_orphan_profiles` below
        // removes *orphaned* siblings older than `FF_RDP_PROFILE_PRUNE_DAYS`
        // (iter-96 Theme B), and `ff-rdp profiles prune` removes the rest
        // on demand.
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

        // iter-175: claim the directory *before* anything else can fail.
        //
        // Two independent leak paths converge here. The first is an early
        // `return Err` from the rest of this function or from `run` — the
        // guard below removes the directory for those. The second is the
        // process simply ceasing to exist (SIGKILL, a CI timeout, a live sweep
        // interrupted mid-test), where no `Drop` ever runs; iteration 171's
        // postmortem hit exactly that, and iteration 175 found eight
        // directories on disk holding nothing but `user.js` and no marker.
        // Recording *our own* PID makes an unmarked managed directory
        // impossible, so a killed launch leaves an attributable directory that
        // the iter-142 dead-owner rule reclaims on the next `launch` (this
        // process is gone by then, and the start token beside the PID stops a
        // recycled PID from resurrecting the claim — iter-171).
        //
        // The post-spawn write in `run` overwrites this pair with Firefox's
        // PID and token; `write_owner_pid_marker` clears the old token first so
        // the two halves can never describe different processes.
        crate::util::profile_dir::write_owner_pid_marker(&tmp, std::process::id());

        managed_guard = crate::util::profile_dir::ManagedProfileGuard::armed(&tmp);
        std::fs::write(tmp.join("user.js"), USER_JS).map_err(|e| {
            let error = AppError::User(format!(
                "failed to write user.js to temporary profile {}: {e}",
                tmp.display()
            ));
            report_failed_profile_cleanup(error, &mut managed_guard)
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
            ensure_extension_autoinstall(p)
                .map_err(|error| report_failed_profile_cleanup(error, &mut managed_guard))?;
            super::auto_consent::install(p)
                .map_err(|error| report_failed_profile_cleanup(error, &mut managed_guard))?;
        }
    }

    // Preserve the existing startup-URL argv position after --profile.
    if let Some(url) = url {
        cmd.arg("--url").arg(url);
    }
    // Nothing below can fail: hand the directory to the caller intact.
    managed_guard.disarm();
    Ok((cmd, profile_path))
}

// Pure command construction, shared by the explicit initializer and the final
// launch. Profile creation/staging stays in build_command and happens once.
fn browser_command(
    firefox: &Path,
    port: u16,
    headless: bool,
    window_size: Option<(u32, u32)>,
    url: Option<&str>,
) -> std::process::Command {
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
    if let Some(url) = url {
        // Browser startup uses a privileged URL loader. Do not replace this
        // with content-target navigateTo (about:support can crash Firefox).
        // The public caller validates before any profile/replace side effects.
        cmd.arg("--url").arg(url);
    }

    // Retain the existing locale environment hints. They do not guarantee the
    // language of engine messages without the relevant Firefox providers.
    // On Windows the LANG env var
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
    // `launch --replace`'s SIGTERM/SIGKILL on the process group does not blast
    // back up to the caller's shell. Without this, the pgid escalation
    // introduced in iter-95 Theme A would target whatever group launched
    // ff-rdp — including the user's interactive shell.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    #[cfg(target_os = "macos")]
    crate::util::child_fds::exclude_inherited(&mut cmd);

    cmd
}

/// Preserve the primary launch failure while making a failed managed-profile
/// cleanup visible in that same JSON error document.
fn report_failed_profile_cleanup(
    error: AppError,
    guard: &mut crate::util::profile_dir::ManagedProfileGuard,
) -> AppError {
    match guard.cleanup() {
        Some((path, reason)) => error.with_warning(format!(
            "could not remove managed Firefox profile {} after launch failed \
             (profile_cleanup_skip_reason: {})",
            path.display(),
            reason.as_str()
        )),
        None => error,
    }
}

/// A spawned browser remains this command's responsibility until its launch
/// result has been delivered. Never remove its profile while it may still run.
fn stop_failed_launch(
    error: AppError,
    child: &mut std::process::Child,
    profile_guard: &mut crate::util::profile_dir::ManagedProfileGuard,
    stderr: &mut startup::StderrCapture,
) -> AppError {
    stderr.pump(None);
    let error = error.with_warning(stderr.summary());
    // `try_wait` may already have collected an immediately-exited child. Never
    // turn its now-reusable numeric PID into authority to signal a new process.
    let observed = child.try_wait();
    let state = match &observed {
        Ok(Some(status)) => format!("natural exit observed before cleanup signals: {status}"),
        Ok(None) => "alive before cleanup signals; subsequent exit cause unknown".to_owned(),
        Err(error) => format!("status unknown before cleanup signals: {error}"),
    };
    stderr.trace(child.id(), "before_cleanup_signals", &state, None);
    let error = error.with_warning(state);
    if matches!(observed, Ok(Some(_))) {
        return report_failed_profile_cleanup(error, profile_guard);
    }
    let pid = child.id();
    // build_command puts Firefox in a new group. Validate that fact before
    // touching descendants; injected spawners may use the caller's group.
    let group = crate::util::process::get_process_group_id(pid)
        .filter(|group| i64::from(*group) == i64::from(pid));
    crate::util::process::kill_process_tree(pid, group);
    let kill_result = child.kill();
    if let Err(e) = kill_result {
        // The process may have exited between the failed operation and kill.
        // Only a collected status authorizes profile removal in that case.
        if !matches!(child.try_wait(), Ok(Some(_))) {
            profile_guard.disarm();
            return error.with_warning(format!(
                "could not stop failed Firefox launch (pid {pid}): {e}; profile retained"
            ));
        }
    }
    match child.wait() {
        Ok(status) => {
            stderr.pump(None);
            stderr.trace(pid, "cleanup_wait", &status.to_string(), None);
            report_failed_profile_cleanup(error, profile_guard)
        }
        Err(e) => {
            profile_guard.disarm();
            error.with_warning(format!(
                "could not wait for failed Firefox launch (pid {pid}): {e}; profile retained"
            ))
        }
    }
}

struct PendingLaunch {
    child: std::process::Child,
    profile: crate::util::profile_dir::ManagedProfileGuard,
    armed: bool,
    stderr: startup::StderrCapture,
}

impl PendingLaunch {
    fn fail(&mut self, error: AppError) -> AppError {
        self.armed = false;
        stop_failed_launch(error, &mut self.child, &mut self.profile, &mut self.stderr)
    }

    fn disarm(&mut self) {
        self.armed = false;
        self.profile.disarm();
    }
}

impl Drop for PendingLaunch {
    fn drop(&mut self) {
        if self.armed {
            // Output uses stdout's printing macros, which can unwind on a
            // broken pipe. Keep the child alive only after successful delivery.
            let error = self.fail(AppError::User("Firefox launch unwound".to_owned()));
            tracing::warn!("{error}");
        }
    }
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
    Exited(std::process::ExitStatus),
    StatusFailed(String),
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
            Self::Exited(status) => Some(AppError::User(format!(
                "Firefox (pid {pid}) exited during startup with {status}"
            ))),
            Self::StatusFailed(error) => Some(AppError::Internal(anyhow::anyhow!(
                "failed to check Firefox status during startup: {error}"
            ))),
        }
    }
}

/// The injectable operations `launch` performs against the outside world.
///
/// A struct of plain function pointers, no dynamic dispatch, real implementations in [`LaunchHooks::real`] and
/// stubs in tests. It exists so the two failure branches Theme A splits apart
/// — port occupied before the spawn, and Firefox never binding after it — are
/// testable without a real Firefox.
type InitializeLanguagePack = fn(
    &Path,
    u16,
    &Path,
    &super::english_language_pack::Pack,
    crate::util::profile_dir::ManagedProfileGuard,
    &LaunchHooks,
    &language_initialization::Runtime,
) -> Result<
    (
        crate::util::profile_dir::ManagedProfileGuard,
        serde_json::Value,
    ),
    AppError,
>;

pub(crate) struct LaunchHooks {
    initialize_language_pack: InitializeLanguagePack,
    emit_launch: fn(&Cli, &serde_json::Value) -> Result<(), AppError>,
    /// Fast probe: does *anything* accept TCP on `port` right now?
    pub(crate) is_port_in_use: fn(u16) -> bool,
    /// Identify the process listening on `port`, if the OS query succeeds.
    pub(crate) find_listener: fn(u16) -> Option<port_owner::PortOwner>,
    /// Poll `host:port` until it accepts a connection or the bound elapses.
    pub(crate) probe_port:
        fn(&str, u16, Duration, &mut startup::Observation<'_>) -> PortWaitOutcome,
    /// Spawn the prepared Firefox command.
    pub(crate) spawn: fn(&mut std::process::Command) -> std::io::Result<std::process::Child>,
    /// Read the owned child's status; injectable for the OS error branch.
    pub(crate) try_wait:
        fn(&mut std::process::Child) -> std::io::Result<Option<std::process::ExitStatus>>,
    /// Locate the Firefox binary (iter-175).
    ///
    /// Injected so the failure paths *past* this point — the ones that create
    /// a profile directory and then fail — are reachable in a unit test on a
    /// machine with no Firefox installed. Without it every such test would be
    /// gated on the real browser being present, which is exactly the reason
    /// this leak went four iterations without a regression test.
    pub(crate) locate_firefox: fn() -> Result<PathBuf, AppError>,
    /// Does an owner-PID marker under ff-rdp's managed profile root name this
    /// PID? The fail-closed ownership proof `launch --replace` requires before
    /// it signals a port owner.
    pub(crate) pid_is_ff_rdp_spawned: fn(u32) -> bool,
}

impl LaunchHooks {
    /// Production hooks that call the real helpers.
    pub(crate) fn real() -> Self {
        Self {
            initialize_language_pack: language_initialization::run,
            emit_launch: |cli, envelope| {
                let hint_ctx = HintContext::new(HintSource::Launch);
                OutputPipeline::from_cli(cli)
                    .and_then(|pipeline| pipeline.finalize_with_hints(envelope, Some(&hint_ctx)))
            },
            is_port_in_use: port_owner::is_port_in_use,
            find_listener: |port| port_owner::find_listener(port).ok().flatten(),
            probe_port: wait_for_port,
            spawn: std::process::Command::spawn,
            try_wait: std::process::Child::try_wait,
            locate_firefox: find_firefox,
            pid_is_ff_rdp_spawned: crate::util::profile_dir::pid_is_ff_rdp_spawned,
        }
    }
}

#[cfg(test)]
impl LaunchHooks {
    /// Hooks whose iter-210 Theme D ownership probes all answer "no ff-rdp
    /// instance here".
    ///
    /// Tests that plant a fake port listener predate Theme D and assert the
    /// port-occupied ERROR path; without this they would silently take the new
    /// no-op path instead. Spread it last (`..LaunchHooks::none_running()`) and
    /// override whatever the test actually cares about.
    fn none_running() -> Self {
        Self {
            pid_is_ff_rdp_spawned: |_pid| false,
            ..Self::real()
        }
    }
}

/// An ff-rdp-launched Firefox already listening on the requested debug port
/// (iter-210 Theme D).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunningInstance {
    pub(crate) pid: u32,
}

/// Identify an ff-rdp-launched Firefox already holding `port`, if there is one.
///
/// Theme D exists because `ff-rdp launch` was the first browser command an
/// agent ran and it failed with exit 1 in 3 of 42 benchmark runs — always
/// because a previous run's Firefox was still up. "The browser you asked for is
/// already running" is not an error; it is the requested state.
///
/// The ownership bar is deliberately the SAME one `--replace` clears before it
/// is allowed to kill anything ([`replace::stop_prior_instance`]): a port
/// listener named by an owner-PID marker under ff-rdp's managed profile root.
/// It fails closed. A Firefox the user started by hand on port 6000, or any
/// other listener, is a foreign owner and still gets the error — reporting
/// someone else's browser as "the instance we launched" would be a lie, and
/// `results.pid` would name a process this command has no claim on.
fn identify_running_instance(port: u16, hooks: &LaunchHooks) -> Option<RunningInstance> {
    let owner = (hooks.find_listener)(port)?;
    if !(hooks.pid_is_ff_rdp_spawned)(owner.pid) {
        return None;
    }
    Some(RunningInstance { pid: owner.pid })
}

/// The envelope a no-op `launch` emits (iter-210 Theme D).
///
/// Same keys as a real launch so a caller can read `results.pid` /
/// `results.port` without branching (`headless`/`profile` are `null`: ff-rdp
/// keeps no record of how an earlier launch was configured), plus
/// `already_running: true` — which is `false` on the launching path, never
/// absent, so `--jq '.results.already_running'` answers on both.
fn emit_already_running(
    cli: &Cli,
    host: &str,
    port: u16,
    instance: &RunningInstance,
) -> Result<(), AppError> {
    let result = json!({
        "already_running": true,
        "pid": instance.pid,
        "host": host,
        "port": port,
        "headless": serde_json::Value::Null,
        "profile": serde_json::Value::Null,
        "profile_path": serde_json::Value::Null,
    });
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(&mut meta, host, port, None, cli.is_verbose());
    let envelope = output::envelope(&result, 1, &meta);
    let hint_ctx = HintContext::new(HintSource::Launch);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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
fn wait_for_port(
    host: &str,
    port: u16,
    timeout: Duration,
    observation: &mut startup::Observation<'_>,
) -> PortWaitOutcome {
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
        observation.stderr.pump(Some(deadline));
        match observation.status() {
            Ok(Some(status)) => {
                observation.terminal("port_wait", "natural_exit");
                return PortWaitOutcome::Exited(status);
            }
            Err(error) => {
                observation.terminal("port_wait", "status_unknown");
                return PortWaitOutcome::StatusFailed(error.to_string());
            }
            Ok(None) => {}
        }
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
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            if std::net::TcpStream::connect_timeout(addr, per_addr.min(remaining)).is_ok() {
                let result = match observation.status() {
                    Ok(None) => PortWaitOutcome::Opened,
                    Ok(Some(status)) => PortWaitOutcome::Exited(status),
                    Err(error) => PortWaitOutcome::StatusFailed(error.to_string()),
                };
                observation.terminal("port_wait", &format!("{result:?}"));
                return result;
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

    // The turn checked actual child status at the absolute deadline. A later
    // cleanup check records any natural exit racing with this observation.
    observation.terminal("port_wait", "alive_when_deadline_checked");
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
    pub(crate) english_language_pack: Option<&'a Path>,
    pub(crate) restart_after_language_pack_install: bool,
    pub(crate) url: Option<&'a str>,
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
        english_language_pack,
        restart_after_language_pack_install,
        url,
        headless,
        profile,
        temp_profile,
        debug_port,
        auto_consent,
        replace,
        window_size,
        launch_timeout,
    } = opts;
    language_initialization::validate_options(
        restart_after_language_pack_install,
        english_language_pack.is_some(),
        &cli.host,
    )?;
    if let Some(url) = url {
        super::url_validation::validate_startup_url_with_opts(
            url,
            cli.allow_file_urls,
            cli.allow_unsafe_urls,
        )?;
    }
    // Validate immutable local input and binary compatibility before housekeeping,
    // port replacement/stop, or profile creation. The default path is unchanged.
    let mut preflight_firefox = None;
    let mut initialization_runtime = None;
    let pack = if let Some(path) = english_language_pack {
        if profile.is_some() || auto_consent {
            return Err(AppError::User("--english-language-pack requires a fresh managed profile and conflicts with --profile and --auto-consent".to_owned()));
        }
        let pack = super::english_language_pack::Pack::read(path)?;
        let firefox = (hooks.locate_firefox)()?;
        pack.check_firefox(&firefox)?;
        if restart_after_language_pack_install {
            initialization_runtime = Some(
                language_initialization::Runtime::read(&firefox).map_err(|e| {
                    AppError::User(format!("language-pack initialization preflight: {e:#}"))
                })?,
            );
        }
        preflight_firefox = Some(firefox);
        Some(pack)
    } else {
        None
    };
    let port = debug_port.unwrap_or(cli.port);
    let host = &cli.host;

    // iter-158 Theme A: resolve the post-spawn debug-port wait bound up front
    // so it can be reported in the envelope (`meta.launch_wait_secs`) whether
    // or not the wait is ever reached.
    let port_wait_bound = resolve_port_wait_bound(
        launch_timeout,
        std::env::var(LAUNCH_TIMEOUT_ENV).ok().as_deref(),
    );

    // iter-133 Theme A: parse --window-size up front so a malformed value
    // fails fast, before any port-collision check or Firefox spawn.
    let window_size: Option<(u32, u32)> = window_size
        .map(crate::util::window_size::parse_window_size)
        .transpose()?;

    // Detect port collision before spawning Firefox. A new --start-debugger-server
    // <port> Firefox silently no-ops when the port is already held by another
    // listener, so we surface the conflict ourselves with a hint that points
    // at `doctor` for follow-up diagnosis.
    // iter-153: captured (not printed) here and folded into this command's
    // own `meta.replaced`, so `launch --replace` always emits exactly one
    // document and `results.pid` always means the process THIS command
    // started.
    let mut replaced: Option<replace::StopOutcome> = None;
    if (hooks.is_port_in_use)(port) {
        if replace {
            // --replace / --force: stop the prior instance, then proceed.
            replaced = Some(replace::stop_prior_instance(port)?);
        } else if let Some(instance) = identify_running_instance(port, hooks) {
            if url.is_some() || pack.is_some() {
                return Err(AppError::User(format!(
                    "{} requires a new Firefox launch; the owned port is already running. Choose a free --debug-port or explicitly use --replace",
                    if pack.is_some() {
                        "--english-language-pack"
                    } else {
                        "--url"
                    }
                )));
            }
            // iter-210 Theme D: the port is held by a Firefox ff-rdp launched.
            // The caller asked for a debuggable Firefox on this port and there
            // is one — report it and exit 0 instead of failing. `--replace`
            // still restarts it; a foreign owner still falls through to the
            // error below.
            return emit_already_running(cli, host, port, &instance);
        } else {
            // iter-158 Theme A: the *pre-spawn* occupancy failure. This is the
            // only branch allowed to say "already in use", and it names the
            // occupying process so the user can act on it.
            reject_if_port_occupied(port, hooks)?;
        }
    }

    let firefox = match preflight_firefox {
        Some(path) => path,
        None => (hooks.locate_firefox)()?,
    };

    let (mut cmd, profile_path) = build_command(
        &firefox,
        port,
        headless,
        profile,
        auto_consent,
        window_size,
        url,
    )?;

    // iter-175: `build_command` handed the managed profile directory back
    // intact; take responsibility for it again until this launch is known to
    // have succeeded. Every `return Err` below — spawn failure, Firefox exiting
    // immediately, the debug port never opening, an unreadable child status —
    // now removes the directory on the way out instead of leaving it for a
    // seven-day age gate. A user-supplied `--profile` directory is never
    // guarded: `should_write_owner_marker` is exactly the "we created it"
    // predicate, and deleting a directory the user chose would be far worse
    // than leaking one we made.
    let mut profile_guard = match profile_path.as_deref() {
        Some(dir) if should_write_owner_marker(profile) => {
            crate::util::profile_dir::ManagedProfileGuard::armed(dir)
        }
        _ => crate::util::profile_dir::ManagedProfileGuard::disarmed(),
    };

    if let Some(pack) = &pack {
        let staged = profile_path
            .as_deref()
            .ok_or_else(|| AppError::User("managed profile absent for language pack".to_owned()))
            .and_then(|path| pack.stage(path, USER_JS));
        if let Err(error) = staged {
            return Err(report_failed_profile_cleanup(error, &mut profile_guard));
        }
    }
    let initialization = if restart_after_language_pack_install {
        let (Some(path), Some(selected_pack), Some(runtime)) = (
            profile_path.as_deref(),
            pack.as_ref(),
            initialization_runtime.as_ref(),
        ) else {
            let error = AppError::Internal(anyhow::anyhow!(
                "language-pack initialization preparation incomplete: managed profile, pack and runtime required"
            ));
            return Err(report_failed_profile_cleanup(error, &mut profile_guard));
        };
        let (guard, receipt) = (hooks.initialize_language_pack)(
            &firefox,
            port,
            path,
            selected_pack,
            profile_guard,
            hooks,
            runtime,
        )?;
        profile_guard = guard;
        Some(receipt)
    } else {
        None
    };
    let child = (hooks.spawn)(&mut cmd).map_err(|e| {
        let error = AppError::User(format!(
            "failed to start Firefox at {}: {e}",
            firefox.display()
        ));
        report_failed_profile_cleanup(error, &mut profile_guard)
    })?;
    let mut pending = PendingLaunch {
        child,
        profile: profile_guard,
        armed: true,
        stderr: startup::StderrCapture::default(),
    };
    if let Err(error) = pending.stderr.attach(pending.child.stderr.take()) {
        return Err(pending.fail(AppError::Internal(anyhow::anyhow!(
            "failed to configure startup stderr: {error}"
        ))));
    }
    let child = &mut pending.child;

    // iter-171: mark ownership *here*, the instant the PID exists — not after
    // the port probe below, which can legitimately spend tens of seconds under
    // contention (iter-158). If the caller is killed inside that window (a
    // live sweep interrupted mid-test, Ctrl-C, a CI timeout), the profile
    // directory is already on disk and Firefox is already running, so an
    // unmarked directory means a leaked profile nobody can attribute or
    // reclaim. The iter-168 postmortem hit exactly this: four abandoned
    // profiles, none of which named an owner.
    //
    // See `should_write_owner_marker` — a `--profile <path>` dir the user owns
    // never receives a marker. Warn-not-fail inside the helpers: a marker
    // write must never fail a launch.
    if should_write_owner_marker(profile)
        && let Some(dir) = profile_path.as_deref()
    {
        crate::util::profile_dir::write_owner_pid_marker(dir, child.id());

        // iter-151 Theme A: if the caller identifies itself (the live-test
        // harness sets this env var on every `ff-rdp launch` it spawns — see
        // `tests/common/mod.rs`), record it alongside the owner PID so a
        // leaked profile can be traced back to the exact test that spawned
        // it, instead of a bisection hunt across ~200 live tests. Absent for
        // a normal interactive `ff-rdp launch` — no marker is written.
        if let Ok(test_name) = std::env::var(crate::util::profile_dir::SPAWNING_TEST_ENV)
            && !test_name.trim().is_empty()
        {
            crate::util::profile_dir::write_owner_test_marker(dir, test_name.trim());
        }
    }

    let mut observation = startup::Observation {
        child,
        stderr: &mut pending.stderr,
        try_wait: hooks.try_wait,
        started: std::time::Instant::now(),
    };
    match observation.initial_interval() {
        Ok(Some(status)) => {
            observation.terminal("initial_interval", "natural_exit");
            let error = AppError::User(format!("Firefox exited immediately with {status}"));
            Err(pending.fail(error))
        }
        Ok(None) => {
            // Still running — verify the debug port is actually reachable
            // before reporting success. Always probe localhost since we
            // just spawned a local Firefox, regardless of --host.
            let pid = observation.child.id();
            let outcome = (hooks.probe_port)("localhost", port, port_wait_bound, &mut observation);
            if let Some(e) = outcome.into_error(pid, port, port_wait_bound) {
                return Err(pending.fail(e));
            }

            // iter-97 Theme A wrote the owner markers here, after the port
            // probe; iter-171 moved them up to immediately after the spawn so
            // an interrupted launch still leaves an attributable profile —
            // see the write site above `try_wait`. Nothing to do here.

            // The language-pack relaunch must have produced a new process,
            // not the initializer that installed the pack.
            let final_start_token = crate::util::process::process_start_token(pid);
            if let Some(initializer) = &initialization
                && (final_start_token.is_none()
                    || (initializer["pid"].as_u64() == Some(u64::from(pid))
                        && initializer["start_token"].as_str() == final_start_token.as_deref()))
            {
                return Err(pending.fail(AppError::User(
                    "operational Firefox identity is unqualified".into(),
                )));
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
                // iter-210 Theme D: always present, so
                // `--jq '.results.already_running'` answers on both paths.
                "already_running": false,
                "pid": pid,
                "host": host,
                "port": port,
                "headless": headless,
                "profile": profile_path_str,
                // iter-96: explicit alias of "profile".
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
            if let Some(pack) = &pack {
                result["english_language_pack"] = pack.staged_result();
                if let Some(receipt) = &initialization {
                    result["english_language_pack"]["mode"] = json!("install-then-relaunch");
                    result["english_language_pack"]["status"] = json!("initialized-and-relaunched");
                    result["english_language_pack"]["initialization"] = receipt.clone();
                    result["english_language_pack"]["relaunch_count"] = json!(1);
                }
            }
            if let Some(url) = url {
                // Forwarded request only; port readiness does not attest that
                // the requested document has loaded.
                result["requested_url"] = json!(url);
            }
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
            let emitted = (hooks.emit_launch)(cli, &envelope);
            if let Err(error) = emitted {
                return Err(pending.fail(error));
            }
            // Only successful output transfers ownership. A bad jq filter or
            // rendering failure must not leave a browser behind an error result.
            pending.disarm();
            Ok(())
        }
        Err(e) => {
            let error = AppError::Internal(anyhow::anyhow!("failed to check Firefox status: {e}"));
            Err(pending.fail(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_147_startup_url_is_one_argument_and_managed_prefs_stay_unchanged() {
        // Inspect actual prepared commands without spawning a browser. Metacharacters
        // must remain inside one URL, never extra argv or a shell invocation.
        let url = "https://example.com/a path?x=1&y=2|literal#--profile";
        let executable = Path::new("owned-fixture-firefox");
        let (default, default_profile) =
            build_command(executable, 7347, true, None, false, None, None).unwrap();
        let (with_url, url_profile) =
            build_command(executable, 7347, true, None, false, None, Some(url)).unwrap();
        let default_profile = default_profile.unwrap();
        let url_profile = url_profile.unwrap();
        let args: Vec<_> = with_url.get_args().collect();
        let position = args.iter().position(|a| *a == "--url").unwrap();
        assert_eq!(args[position + 1], url);
        assert_eq!(args.iter().filter(|a| **a == "--url").count(), 1);
        let normalized = |cmd: &std::process::Command, profile: &Path| {
            cmd.get_args()
                .map(|a| {
                    if a == profile.as_os_str() {
                        "<profile>".to_owned()
                    } else {
                        a.to_string_lossy().into_owned()
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut changed = normalized(&with_url, &url_profile);
        drop(changed.drain(position..position + 2));
        assert_eq!(changed, normalized(&default, &default_profile));
        assert_eq!(default.get_program(), with_url.get_program());
        assert_eq!(
            default.get_envs().collect::<Vec<_>>(),
            with_url.get_envs().collect::<Vec<_>>()
        );
        for profile in [&default_profile, &url_profile] {
            assert_eq!(
                std::fs::read_to_string(profile.join("user.js")).unwrap(),
                USER_JS
            );
            std::fs::remove_dir_all(profile).unwrap();
        }
    }

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

    // ── iter-210 Theme D: idempotent `launch` ───────────────────────────────

    /// AC `live_launch_twice_is_a_noop` (foreign-owner half): a listener with
    /// no ownership proof is NOT reported as ours. This is the check that keeps
    /// a hand-started Firefox on port 6000 producing the port-occupied error
    /// instead of a fabricated "already running" success naming someone else's
    /// process.
    #[test]
    fn unit_210_foreign_port_owner_is_not_a_running_instance() {
        let hooks = LaunchHooks {
            find_listener: |_port| {
                Some(port_owner::PortOwner {
                    pid: 51234,
                    process_name: "firefox".to_owned(),
                    uptime_s: None,
                })
            },
            pid_is_ff_rdp_spawned: |_pid| false,
            ..LaunchHooks::none_running()
        };
        assert_eq!(
            identify_running_instance(6000, &hooks),
            None,
            "a listener with no owner-PID marker must not be claimed as ff-rdp's"
        );
    }

    /// A port listener that DOES carry ff-rdp's owner-PID marker is ours.
    #[test]
    fn unit_210_marked_port_owner_is_a_running_instance() {
        let hooks = LaunchHooks {
            find_listener: |_port| {
                Some(port_owner::PortOwner {
                    pid: 777,
                    process_name: "firefox".to_owned(),
                    uptime_s: None,
                })
            },
            pid_is_ff_rdp_spawned: |_pid| true,
            ..LaunchHooks::none_running()
        };
        let found = identify_running_instance(6000, &hooks)
            .expect("an owner-PID-marked listener is an ff-rdp instance");
        assert_eq!(found.pid, 777);
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
            build_command(&tmp, 6000, false, Some(user_profile_str), false, None, None).unwrap();
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
        let bound = resolve_port_wait_bound(Some(30), None);
        let outcome = PortWaitOutcome::TimedOut;
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
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::Opened,
            spawn: |_cmd| {
                SPAWNS.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other(
                    "the spawn hook must never be reached",
                ))
            },
            locate_firefox: || {
                Err(AppError::Internal(anyhow::anyhow!(
                    "the firefox lookup must never be reached either"
                )))
            },
            ..LaunchHooks::none_running()
        };

        let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"])
            .expect("parse a bare `launch`");
        let opts = LaunchOpts {
            english_language_pack: None,
            restart_after_language_pack_install: false,
            url: None,
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
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let (cmd, profile_path) =
            build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let (cmd, _) = build_command(&tmp, 6000, true, None, false, None, None).unwrap();
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
        let (cmd, _) = build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
            build_command(&tmp, 6000, false, Some(profile_str), false, None, None).unwrap();
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
        let (cmd, profile_path) =
            build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let (_, profile_path) = build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let (cmd, _) = build_command(&tmp, 9222, false, None, false, None, None).unwrap();
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
            build_command(&tmp, 6000, true, None, false, Some((390, 844)), None).unwrap();
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
        let (cmd, profile) = build_command(&tmp, 6000, false, None, false, None, None).unwrap();
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
        let result = build_command(&tmp, 6000, false, None, true, None, None);
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
        let result = build_command(&tmp, 6000, false, None, true, None, None);
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

// ---------------------------------------------------------------------------
// iter-175: a launch that fails after creating its profile directory
// ---------------------------------------------------------------------------

#[cfg(test)]
mod iter_175_tests {
    use super::*;
    use std::process::Stdio;
    use std::sync::Mutex;

    /// The `--profile <path>` argument `build_command` put on the command the
    /// spawn hook was handed. This is how a test learns *which* directory the
    /// launch created without the (failing) call ever returning it.
    fn profile_arg_of(cmd: &std::process::Command) -> Option<PathBuf> {
        let mut args = cmd.get_args();
        while let Some(arg) = args.next() {
            if arg == "--profile" {
                return args.next().map(PathBuf::from);
            }
        }
        None
    }

    /// A child that is already gone by the time `run`'s 500 ms grace elapses —
    /// stands in for a Firefox that dies on startup (bad flags, missing libs).
    fn spawn_exiting_child() -> std::io::Result<std::process::Child> {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.arg("/c").arg("exit 3");
            c
        } else {
            let mut c = std::process::Command::new("/bin/sh");
            c.arg("-c").arg("exit 3");
            c
        };
        cmd.stdout(Stdio::null()).stderr(Stdio::piped()).spawn()
    }

    /// A child that outlives the grace period but never binds a port — stands
    /// in for the debug-port deadline branch, which kills the child and fails.
    fn spawn_lingering_child() -> std::io::Result<std::process::Child> {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.arg("/c").arg("ping -n 31 127.0.0.1");
            c
        } else {
            let mut c = std::process::Command::new("/bin/sh");
            c.arg("-c").arg("sleep 30");
            c
        };
        cmd.stdout(Stdio::null()).stderr(Stdio::piped()).spawn()
    }

    fn bare_launch_cli() -> Cli {
        <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"])
            .expect("a bare `launch` must parse")
    }

    fn managed_opts(port: u16) -> LaunchOpts<'static> {
        LaunchOpts {
            english_language_pack: None,
            restart_after_language_pack_install: false,
            url: None,
            headless: true,
            // `None` is the managed-profile case — the only one that ever
            // creates (and so can leak) a directory of ours.
            profile: None,
            temp_profile: false,
            debug_port: Some(port),
            auto_consent: false,
            replace: false,
            window_size: None,
            launch_timeout: Some(0),
        }
    }

    #[test]
    fn unit_147_langpack_conflicts_and_invalid_input_precede_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let invalid = root.path().join("invalid.xpi");
        std::fs::write(&invalid, b"not a ZIP").unwrap();
        let hooks = LaunchHooks {
            is_port_in_use: |_| panic!("invalid pack reached port / replacement"),
            locate_firefox: || panic!("invalid pack reached Firefox lookup"),
            spawn: |_| panic!("invalid pack reached spawn"),
            ..LaunchHooks::none_running()
        };
        for (profile, auto_consent) in [
            (None, false),
            (Some("uncreated-profile"), false),
            (None, true),
        ] {
            let opts = LaunchOpts {
                english_language_pack: Some(&invalid),
                restart_after_language_pack_install: false,
                profile,
                auto_consent,
                replace: true,
                ..managed_opts(7350)
            };
            assert!(run_with_hooks(&bare_launch_cli(), &opts, &hooks).is_err());
        }
        for conflicting in ["--auto-consent", "--profile=uncreated-profile"] {
            assert!(
                <Cli as clap::Parser>::try_parse_from([
                    "ff-rdp",
                    "launch",
                    "--english-language-pack",
                    "local.xpi",
                    conflicting
                ])
                .is_err()
            );
        }
    }

    #[test]
    fn unit_147_langpack_owned_port_and_stage_failure_preserve_ownership() {
        use super::super::english_language_pack::{fixture, fixture_manifest};
        thread_local! { static BINARY: std::cell::RefCell<PathBuf> = const { std::cell::RefCell::new(PathBuf::new()) }; }
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("firefox");
        std::fs::write(&binary, b"never executed").unwrap();
        std::fs::write(
            root.path().join("application.ini"),
            "[App]\nVersion=156.0.1\n",
        )
        .unwrap();
        BINARY.with(|p| *p.borrow_mut() = binary);
        let xpi = root.path().join("local.xpi");
        std::fs::write(&xpi, fixture(&fixture_manifest(), &[])).unwrap();
        let hooks = LaunchHooks {
            locate_firefox: || BINARY.with(|p| Ok(p.borrow().clone())),
            is_port_in_use: |_| true,
            find_listener: |_port| {
                Some(port_owner::PortOwner {
                    pid: 17350,
                    process_name: "firefox".to_owned(),
                    uptime_s: None,
                })
            },
            pid_is_ff_rdp_spawned: |_| true,
            spawn: |_| panic!("occupied pack launch must not spawn"),
            ..LaunchHooks::none_running()
        };
        let opts = LaunchOpts {
            english_language_pack: Some(&xpi),
            restart_after_language_pack_install: false,
            ..managed_opts(7350)
        };
        assert!(
            run_with_hooks(&bare_launch_cli(), &opts, &hooks)
                .unwrap_err()
                .to_string()
                .contains("--english-language-pack requires a new Firefox launch")
        );
        thread_local! { static STAGED: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
        let failed_spawn = LaunchHooks {
            is_port_in_use: |_| false,
            spawn: |cmd| {
                let args: Vec<_> = cmd.get_args().collect();
                let index = args.iter().position(|a| *a == "--profile").unwrap();
                let profile = PathBuf::from(args[index + 1]);
                assert!(
                    profile
                        .join("extensions/langpack-en-US@firefox.mozilla.org.xpi")
                        .is_file()
                );
                assert_eq!(
                    std::fs::read_to_string(profile.join("user.js")).unwrap(),
                    format!(
                        "{USER_JS}{}",
                        super::super::english_language_pack::PROFILE_ACTIVATION
                    )
                );
                STAGED.with(|p| *p.borrow_mut() = Some(profile));
                Err(std::io::Error::other(
                    "injected failure after actual staging",
                ))
            },
            ..hooks
        };
        assert!(run_with_hooks(&bare_launch_cli(), &opts, &failed_spawn).is_err());
        assert!(!STAGED.with(|p| p.borrow().clone().unwrap()).exists());
        // The actual cleanup helper used by the pre-spawn staging failure path
        // removes only the freshly guarded profile, retaining its neighbor.
        let (_, profile) = build_command(
            &BINARY.with(|p| p.borrow().clone()),
            7350,
            true,
            None,
            false,
            None,
            None,
        )
        .unwrap();
        let profile = profile.unwrap();
        std::fs::create_dir(profile.join("extensions")).unwrap();
        let mut guard = crate::util::profile_dir::ManagedProfileGuard::armed(&profile);
        let pack = super::super::english_language_pack::Pack::read(&xpi).unwrap();
        let error = pack.stage(&profile, USER_JS).unwrap_err();
        let _ = report_failed_profile_cleanup(error, &mut guard);
        assert!(!profile.exists());
        assert!(xpi.is_file());
    }

    #[test]
    fn unit_147_invalid_startup_url_precedes_replace_and_profile_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("must-not-exist");
        let hooks = LaunchHooks {
            is_port_in_use: |_| panic!("invalid URL reached a port probe or replace"),
            locate_firefox: || panic!("invalid URL reached Firefox lookup"),
            spawn: |_| panic!("invalid URL reached spawn"),
            ..LaunchHooks::none_running()
        };
        for url in [
            "--profile",
            "about:support\n",
            "file:///private/report",
            "javascript:void(0)",
        ] {
            let opts = LaunchOpts {
                url: Some(url),
                profile: profile.to_str(),
                replace: true,
                ..managed_opts(7348)
            };
            let err = run_with_hooks(&bare_launch_cli(), &opts, &hooks).unwrap_err();
            assert!(matches!(err, AppError::User(_)));
            assert!(!profile.exists());
        }
    }

    #[test]
    fn unit_147_owned_existing_launch_cannot_silently_ignore_startup_url() {
        let hooks = LaunchHooks {
            is_port_in_use: |_| true,
            find_listener: |_port| {
                Some(port_owner::PortOwner {
                    pid: 17347,
                    process_name: "firefox".to_owned(),
                    uptime_s: None,
                })
            },
            pid_is_ff_rdp_spawned: |_| true,
            locate_firefox: || panic!("existing instance should not spawn"),
            spawn: |_| panic!("existing instance should not spawn"),
            ..LaunchHooks::none_running()
        };
        let opts = LaunchOpts {
            url: Some("about:support"),
            ..managed_opts(7349)
        };
        let err = run_with_hooks(&bare_launch_cli(), &opts, &hooks).unwrap_err();
        assert!(matches!(err, AppError::User(_)));
        assert!(
            err.to_string()
                .contains("--url requires a new Firefox launch")
        );
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn assert_failed_launch_reaps_child(status_failure: bool, managed: bool, unwind: bool) {
        // A real direct child, not a pid-aliveness surrogate. The waitpid
        // observation proves whether launch actually collected its status.
        use std::cell::RefCell;
        thread_local! {
            static SPAWNED: RefCell<Option<(u32, PathBuf)>> = const { RefCell::new(None) };
        }
        fn spawn(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
            use std::os::unix::process::CommandExt;
            let child = std::process::Command::new("/bin/sleep")
                .arg("30")
                .process_group(0)
                .spawn()?;
            SPAWNED.with(|slot| {
                *slot.borrow_mut() = Some((child.id(), profile_arg_of(cmd).unwrap()));
            });
            Ok(child)
        }
        {
            let mut sibling = std::process::Command::new("/bin/sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let user_profile = tempfile::tempdir().unwrap();
            std::fs::write(user_profile.path().join("retain-me"), b"user data").unwrap();
            let mut opts = managed_opts(7628);
            if !managed {
                opts.profile = Some(user_profile.path().to_str().unwrap());
            }
            let hooks = LaunchHooks {
                locate_firefox: || Ok(PathBuf::from("/unused/firefox")),
                is_port_in_use: |_| false,
                spawn,
                probe_port: |_, _, _, _| PortWaitOutcome::Opened,
                try_wait: if unwind {
                    |_| panic!("injected launch unwind")
                } else if status_failure {
                    |_| Err(std::io::Error::other("injected status failure"))
                } else {
                    std::process::Child::try_wait
                },
                ..LaunchHooks::none_running()
            };
            let mut cli = bare_launch_cli();
            if !status_failure {
                cli.jq = Some("this is not valid %%%".to_owned());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_hooks(&cli, &opts, &hooks)
            }));
            let sibling_status = sibling.try_wait().unwrap();
            sibling.kill().unwrap();
            sibling.wait().unwrap();
            let (pid, profile) = SPAWNED.with(|slot| slot.borrow_mut().take().unwrap());
            let native_pid = i32::try_from(pid).unwrap();
            let mut status = 0;
            // SAFETY: pid is the direct child just spawned by this test. WNOHANG
            // never waits on another test's child and does not signal anything.
            let waited = unsafe { libc::waitpid(native_pid, &raw mut status, libc::WNOHANG) };
            let wait_error = std::io::Error::last_os_error().raw_os_error();
            if waited == 0 {
                // Regression/before arm: clean up our still-owned, unreaped
                // child before failing the assertion (no detached test child).
                unsafe {
                    libc::kill(native_pid, libc::SIGKILL);
                    libc::waitpid(native_pid, &raw mut status, 0);
                }
            }
            if unwind {
                assert!(result.is_err(), "injected launch must unwind");
            } else {
                assert!(result.unwrap().is_err(), "injected launch must fail");
            }
            assert_eq!(waited, -1, "launch left child uncollected");
            assert_eq!(
                wait_error,
                Some(libc::ECHILD),
                "actual launch wait required"
            );
            assert!(
                sibling_status.is_none(),
                "unrelated owned control was killed"
            );
            if managed {
                assert!(!profile.exists(), "managed failed-launch profile survives");
            } else {
                assert_eq!(
                    std::fs::read(profile.join("retain-me")).unwrap(),
                    b"user data"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unit_282_failed_status_reaps_the_owned_child() {
        assert_failed_launch_reaps_child(true, true, false);
    }

    #[cfg(unix)]
    #[test]
    fn unit_282_failed_output_reaps_the_owned_child() {
        assert_failed_launch_reaps_child(false, true, false);
    }

    #[cfg(unix)]
    #[test]
    fn unit_282_failed_output_preserves_user_profile_and_unrelated_child() {
        assert_failed_launch_reaps_child(false, false, false);
    }

    #[cfg(unix)]
    #[test]
    fn unit_282_unwinding_launch_reaps_the_owned_child() {
        assert_failed_launch_reaps_child(false, true, true);
    }

    /// AC 2 (`unit_175_failed_spawn_leaves_no_profile_dir`): `build_command`
    /// creates the profile directory *before* the spawn, so a spawn that fails
    /// used to return straight past it, leaving an `ff-rdp-profile-*` directory
    /// with no owner marker for the seven-day mtime gate to sit on. Fails on
    /// `main`, where the directory is still there after the error.
    #[test]
    fn unit_175_failed_spawn_leaves_no_profile_dir() {
        static PROFILE: Mutex<Option<PathBuf>> = Mutex::new(None);

        let hooks = LaunchHooks {
            is_port_in_use: |_port| false,
            find_listener: |_port| None,
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::Opened,
            spawn: |cmd| {
                *PROFILE.lock().expect("profile slot") = profile_arg_of(cmd);
                Err(std::io::Error::other("simulated spawn failure"))
            },
            locate_firefox: || Ok(PathBuf::from("/nonexistent/ff-rdp-fake-firefox")),
            ..LaunchHooks::none_running()
        };

        let err = run_with_hooks(&bare_launch_cli(), &managed_opts(7601), &hooks)
            .expect_err("a failing spawn must fail the launch");
        assert!(
            matches!(&err, AppError::User(m) if m.contains("failed to start Firefox")),
            "expected the spawn-failure message, got {err:?}"
        );

        let dir = PROFILE
            .lock()
            .expect("profile slot")
            .clone()
            .expect("build_command must have put --profile on the command");
        assert!(
            !dir.exists(),
            "iter-175: a launch whose spawn failed must not leave its profile directory behind: {}",
            dir.display()
        );
    }

    #[test]
    fn unit_261_failed_cleanup_reaches_launch_error_json() {
        static PROFILE: Mutex<Option<PathBuf>> = Mutex::new(None);

        let hooks = LaunchHooks {
            is_port_in_use: |_port| false,
            find_listener: |_port| None,
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::Opened,
            spawn: |cmd| {
                let profile = profile_arg_of(cmd).expect("managed profile argument");
                std::fs::remove_dir_all(&profile).expect("replace profile directory");
                std::fs::write(&profile, b"force remove_dir_all to fail")
                    .expect("replace profile with file");
                *PROFILE.lock().expect("profile slot") = Some(profile);
                Err(std::io::Error::other("simulated spawn failure"))
            },
            locate_firefox: || Ok(PathBuf::from("/nonexistent/ff-rdp-fake-firefox")),
            ..LaunchHooks::none_running()
        };

        let err = run_with_hooks(&bare_launch_cli(), &managed_opts(7611), &hooks)
            .expect_err("a failing spawn must fail the launch");
        assert_eq!(
            err.exit_code(),
            1,
            "cleanup reporting must preserve User exit semantics"
        );
        let json = err.to_error_json();
        assert_eq!(json["error_type"], "User");
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("failed to start Firefox")
        );

        let profile = PROFILE.lock().expect("profile slot").take().unwrap();
        let warning = json["warnings"][0].as_str().expect("warning string");
        assert!(
            warning.contains(&profile.display().to_string()),
            "{warning}"
        );
        assert!(
            warning.contains("profile_cleanup_skip_reason: remove-failed"),
            "{warning}"
        );
        assert!(
            profile.exists(),
            "the forced cleanup failure must leave evidence behind"
        );
        std::fs::remove_file(profile).expect("clean up forced survivor");
    }

    /// AC 2, second error path: Firefox spawns but exits immediately. `run`
    /// reports the exit status; the directory must not survive it.
    #[test]
    fn unit_175_immediate_exit_leaves_no_profile_dir() {
        static PROFILE: Mutex<Option<PathBuf>> = Mutex::new(None);

        let hooks = LaunchHooks {
            is_port_in_use: |_port| false,
            find_listener: |_port| None,
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::Opened,
            spawn: |cmd| {
                *PROFILE.lock().expect("profile slot") = profile_arg_of(cmd);
                spawn_exiting_child()
            },
            locate_firefox: || Ok(PathBuf::from("/nonexistent/ff-rdp-fake-firefox")),
            ..LaunchHooks::none_running()
        };

        let err = run_with_hooks(&bare_launch_cli(), &managed_opts(7602), &hooks)
            .expect_err("a browser that exits immediately must fail the launch");
        assert!(
            err.to_error_json()["error_type"] == "User"
                && err.to_string().contains("exited immediately"),
            "expected the immediate-exit message, got {err:?}"
        );

        let dir = PROFILE
            .lock()
            .expect("profile slot")
            .clone()
            .expect("build_command must have put --profile on the command");
        assert!(
            !dir.exists(),
            "iter-175: a Firefox that exited immediately must not leave a profile behind: {}",
            dir.display()
        );
    }

    /// AC 2, third error path: the child stays up but never opens the debug
    /// port. `run` kills it and fails — and must reclaim the profile too.
    #[test]
    fn unit_175_port_wait_timeout_leaves_no_profile_dir() {
        static PROFILE: Mutex<Option<PathBuf>> = Mutex::new(None);

        let hooks = LaunchHooks {
            is_port_in_use: |_port| false,
            find_listener: |_port| None,
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::TimedOut,
            spawn: |cmd| {
                *PROFILE.lock().expect("profile slot") = profile_arg_of(cmd);
                spawn_lingering_child()
            },
            locate_firefox: || Ok(PathBuf::from("/nonexistent/ff-rdp-fake-firefox")),
            ..LaunchHooks::none_running()
        };

        let err = run_with_hooks(&bare_launch_cli(), &managed_opts(7603), &hooks)
            .expect_err("a debug port that never opens must fail the launch");
        assert!(
            err.to_error_json()["error_type"] == "User"
                && err.to_string().contains("did not open debug port"),
            "expected the port-deadline message, got {err:?}"
        );

        let dir = PROFILE
            .lock()
            .expect("profile slot")
            .clone()
            .expect("build_command must have put --profile on the command");
        assert!(
            !dir.exists(),
            "iter-175: a launch that timed out waiting for the debug port must not leave its \
             profile behind: {}",
            dir.display()
        );
    }

    /// A user-supplied `--profile` directory is never ours to delete, however
    /// the launch fails. The guard must stay disarmed for that whole branch.
    #[test]
    fn unit_175_user_profile_dir_survives_a_failed_launch() {
        let user_profile = std::env::temp_dir().join(format!(
            "ff-rdp-175-user-profile-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&user_profile);
        std::fs::create_dir_all(&user_profile).expect("create the user's own profile dir");
        let keeper = user_profile.join("keep-me.txt");
        std::fs::write(&keeper, b"the user's data").expect("seed the user's profile dir");

        let hooks = LaunchHooks {
            is_port_in_use: |_port| false,
            find_listener: |_port| None,
            probe_port: |_host, _port, _timeout, _observation| PortWaitOutcome::Opened,
            spawn: |_cmd| Err(std::io::Error::other("simulated spawn failure")),
            locate_firefox: || Ok(PathBuf::from("/nonexistent/ff-rdp-fake-firefox")),
            ..LaunchHooks::none_running()
        };

        let profile_str = user_profile
            .to_str()
            .expect("temp dir path must be valid UTF-8")
            .to_owned();
        let opts = LaunchOpts {
            profile: Some(&profile_str),
            ..managed_opts(7604)
        };
        let _ = run_with_hooks(&bare_launch_cli(), &opts, &hooks)
            .expect_err("a failing spawn must fail the launch");

        assert!(
            keeper.exists(),
            "iter-175: a --profile directory the user supplied must survive a failed launch: {}",
            user_profile.display()
        );
        let _ = std::fs::remove_dir_all(&user_profile);
    }

    /// The other half of the fix, for the case `Drop` cannot cover (SIGKILL, a
    /// CI timeout): the directory carries an owner marker naming *this* process
    /// from the moment `build_command` returns, so it can never be the
    /// unattributable `user.js`-only directory iterations 171 and 175 found.
    /// Fails on `main`, where no marker exists until after the spawn.
    #[test]
    fn unit_175_build_command_marks_profile_with_own_pid_before_spawn() {
        let firefox = fake_firefox_for_175();
        let (_cmd, profile_path) = build_command(&firefox, 6000, false, None, false, None, None)
            .expect("build_command with a managed profile must succeed");
        let _ = std::fs::remove_file(&firefox);

        let dir = profile_path.expect("a managed launch must have a profile path");
        let marker = dir.join(crate::util::profile_dir::OWNER_PID_MARKER);
        let recorded = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("owner marker {} must exist: {e}", marker.display()));
        assert_eq!(
            recorded.trim().parse::<u32>().ok(),
            Some(std::process::id()),
            "the pre-spawn marker must name the launching process itself"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Local copy of the outer module's `fake_firefox` helper — the two test
    /// modules are siblings, and duplicating four lines beats making a test
    /// fixture `pub(super)`.
    fn fake_firefox_for_175() -> PathBuf {
        let id = std::thread::current().id();
        let name = format!("fake-firefox-175-{id:?}").replace(['(', ')', ' '], "-");
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write the fake firefox");
        path
    }
    #[cfg(unix)]
    #[test]
    fn unit_147_restart_rejects_before_external_effects() {
        let hooks = LaunchHooks {
            is_port_in_use: |_| panic!("invalid restart reached port"),
            locate_firefox: || panic!("invalid restart reached binary"),
            initialize_language_pack: |_, _, _, _, _, _, _| {
                panic!("invalid restart reached initializer")
            },
            spawn: |_| panic!("invalid restart reached spawn"),
            ..LaunchHooks::none_running()
        };
        let opts = LaunchOpts {
            restart_after_language_pack_install: true,
            replace: true,
            ..managed_opts(7350)
        };
        assert!(
            run_with_hooks(&bare_launch_cli(), &opts, &hooks)
                .unwrap_err()
                .to_string()
                .contains("requires --english-language-pack")
        );
        let mut cli = bare_launch_cli();
        cli.host = "example.invalid".into();
        let opts = LaunchOpts {
            english_language_pack: Some(std::path::Path::new("unread.xpi")),
            ..opts
        };
        assert!(
            run_with_hooks(&cli, &opts, &hooks)
                .unwrap_err()
                .to_string()
                .contains("loopback")
        );
        assert!(
            <Cli as clap::Parser>::try_parse_from([
                "ff-rdp",
                "launch",
                "--restart-after-language-pack-install"
            ])
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unit_147_restart_handoff_reuses_profile_and_never_retries() {
        use super::super::english_language_pack::{fixture, fixture_manifest};
        thread_local! {
            static BINARY: std::cell::RefCell<PathBuf> = const { std::cell::RefCell::new(PathBuf::new()) };
            static PROFILE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
            static INITIALIZATIONS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
            static FINAL_ATTEMPTS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
        }
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("firefox");
        std::fs::write(&binary, b"fixture only").unwrap();
        std::fs::write(
            root.path().join("application.ini"),
            "[App]\nVersion=156.0.1\nBuildID=test\n",
        )
        .unwrap();
        BINARY.with(|v| *v.borrow_mut() = binary);
        let xpi = root.path().join("pack.xpi");
        std::fs::write(&xpi, fixture(&fixture_manifest(), &[])).unwrap();
        let hooks = LaunchHooks {
            locate_firefox: || BINARY.with(|v| Ok(v.borrow().clone())),
            is_port_in_use: |_| false,
            initialize_language_pack: |_, _, profile, pack, guard, _, _| {
                INITIALIZATIONS.with(|v| {
                    v.set(v.get() + 1);
                    assert_eq!(v.get(), 1);
                });
                pack.verify_staged(profile, USER_JS).unwrap();
                PROFILE.with(|v| *v.borrow_mut() = Some(profile.to_owned()));
                Ok((guard, json!({"pid":1,"start_token":"fixture"})))
            },
            spawn: |cmd| {
                FINAL_ATTEMPTS.with(|v| {
                    v.set(v.get() + 1);
                    assert_eq!(v.get(), 1);
                });
                let args: Vec<_> = cmd.get_args().collect();
                let i = args.iter().position(|v| *v == "--profile").unwrap();
                PROFILE.with(|v| assert_eq!(Path::new(args[i + 1]), v.borrow().as_ref().unwrap()));
                assert!(args.iter().any(|v| *v == "https://example.invalid/final"));
                Err(std::io::Error::other("deliberate final spawn failure"))
            },
            ..LaunchHooks::none_running()
        };
        let opts = LaunchOpts {
            english_language_pack: Some(&xpi),
            restart_after_language_pack_install: true,
            url: Some("https://example.invalid/final"),
            ..managed_opts(7350)
        };
        assert!(
            run_with_hooks(&bare_launch_cli(), &opts, &hooks)
                .unwrap_err()
                .to_string()
                .contains("deliberate final spawn failure")
        );
        PROFILE.with(|v| assert!(!v.borrow().as_ref().unwrap().exists()));
        INITIALIZATIONS.with(|v| assert_eq!(v.get(), 1));
        FINAL_ATTEMPTS.with(|v| assert_eq!(v.get(), 1));
        let hooks = LaunchHooks {
            initialize_language_pack: |_, _, _, _, _guard, _, _| {
                Err(AppError::User("initializer refused".into()))
            },
            spawn: |_| panic!("failed initializer reached final spawn"),
            ..hooks
        };
        assert!(
            run_with_hooks(&bare_launch_cli(), &opts, &hooks)
                .unwrap_err()
                .to_string()
                .contains("initializer refused")
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "isolated restart-control executor; parent supplies private state"]
    #[allow(unsafe_code)]
    fn restart_success_isolated_executor() {
        use super::super::english_language_pack::{fixture, fixture_manifest};
        let isolated =
            PathBuf::from(std::env::var_os("FF_RDP_147_CONTROL_HOME").expect("owned control home"));
        assert!(isolated.is_absolute());
        assert_eq!(
            std::env::var_os("FF_RDP_HOME"),
            Some(isolated.clone().into_os_string()),
            "147 isolation home mismatch before launch"
        );
        assert_eq!(
            std::fs::read(isolated.join("147-control-owner")).unwrap(),
            b"owned restart control"
        );
        thread_local! {
            static BINARY: std::cell::RefCell<PathBuf> = const { std::cell::RefCell::new(PathBuf::new()) };
            static PROFILE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
            static SPAWNS: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
            static ENVELOPES: std::cell::RefCell<Vec<serde_json::Value>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("firefox");
        std::fs::write(&binary, b"never executed").unwrap();
        std::fs::write(
            root.path().join("application.ini"),
            "[App]\nVersion=156.0.1\nBuildID=test\n",
        )
        .unwrap();
        BINARY.with(|v| *v.borrow_mut() = binary);
        let xpi = root.path().join("pack.xpi");
        std::fs::write(&xpi, fixture(&fixture_manifest(), &[])).unwrap();
        let hooks = LaunchHooks {
            locate_firefox: || BINARY.with(|v| Ok(v.borrow().clone())),
            is_port_in_use: |_| false,
            probe_port: |_, _, _, _| PortWaitOutcome::Opened,
            emit_launch: |_, value| {
                ENVELOPES.with(|v| v.borrow_mut().push(value.clone()));
                Ok(())
            },
            initialize_language_pack: |firefox, port, profile, pack, guard, hooks, _| {
                pack.verify_staged(profile, USER_JS).unwrap();
                PROFILE.with(|v| *v.borrow_mut() = Some(profile.to_owned()));
                let mut cmd = browser_command(firefox, port, true, None, Some("about:blank"));
                cmd.arg("--profile").arg(profile);
                let mut child = (hooks.spawn)(&mut cmd).unwrap();
                let pid = child.id();
                let status = child.wait().unwrap();
                assert!(status.success());
                // This hook models the independently tested readiness/state gates;
                // the harmless initializer Child really ran and was waited here.
                pack.verify_staged(profile, USER_JS).unwrap();
                Ok((
                    guard,
                    json!({"pid":pid,"start_token":"test-initializer","actual_child_wait":true,"exit_code":0}),
                ))
            },
            spawn: |cmd| {
                let args: Vec<_> = cmd.get_args().collect();
                let i = args.iter().position(|v| *v == "--profile").unwrap();
                PROFILE.with(|v| assert_eq!(Path::new(args[i + 1]), v.borrow().as_ref().unwrap()));
                let first = SPAWNS.with(|v| v.borrow().is_empty());
                if first {
                    assert!(args.iter().any(|v| *v == "about:blank"));
                    assert!(!args.iter().any(|v| *v == "https://example.invalid/final"));
                } else {
                    assert!(args.iter().any(|v| *v == "https://example.invalid/final"));
                }
                let child = if first {
                    std::process::Command::new("/bin/sh")
                        .args(["-c", "exit 0"])
                        .spawn()?
                } else {
                    std::process::Command::new("/bin/sleep").arg("2").spawn()?
                };
                SPAWNS.with(|v| v.borrow_mut().push(child.id()));
                Ok(child)
            },
            ..LaunchHooks::none_running()
        };
        let opts = LaunchOpts {
            english_language_pack: Some(&xpi),
            restart_after_language_pack_install: true,
            url: Some("https://example.invalid/final"),
            ..managed_opts(7363)
        };
        let result = run_with_hooks(&bare_launch_cli(), &opts, &hooks);
        let pids = SPAWNS.with(|v| v.borrow().clone());
        let last = *pids.last().unwrap();
        let native = i32::try_from(last).unwrap();
        let birth = crate::util::process::process_start_token(last);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut status = 0;
        let mut waited = loop {
            // SAFETY: last is our direct harmless child, never a foreign PID.
            // WNOHANG never blocks. The production success path intentionally
            // drops its Child; this test still owns the parent wait obligation.
            let observed = unsafe { libc::waitpid(native, &raw mut status, libc::WNOHANG) };
            if observed != 0 {
                break observed;
            }
            if std::time::Instant::now() >= deadline {
                break 0;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if waited == 0 {
            if birth.is_some() && crate::util::process::process_start_token(last) == birth {
                // SAFETY: matching birth and unreaped direct-child identity bind
                // this timeout cleanup to the exact harmless test child.
                unsafe {
                    libc::kill(native, libc::SIGKILL);
                }
            }
            let cleanup = std::time::Instant::now() + Duration::from_secs(2);
            while waited == 0 && std::time::Instant::now() < cleanup {
                // SAFETY: same direct child, nonblocking actual wait.
                waited = unsafe { libc::waitpid(native, &raw mut status, libc::WNOHANG) };
                if waited == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        assert_ne!(
            waited, 0,
            "owned child cleanup uncertain; retain profile for recovery, PID {last}"
        );
        PROFILE.with(|v| std::fs::remove_dir_all(v.borrow().as_ref().unwrap()).unwrap());
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(pids.len(), 2);
        assert_ne!(pids[0], pids[1]);
        assert_eq!(
            waited, native,
            "final harmless child was not actually waited"
        );
        assert_eq!(status, 0);
        ENVELOPES.with(|v| {
            let values = v.borrow();
            assert_eq!(values.len(), 1);
            assert_eq!(values[0]["results"]["pid"], last);
            assert_eq!(
                values[0]["results"]["english_language_pack"]["initialization"]["pid"],
                pids[0]
            );
            assert_eq!(
                values[0]["results"]["english_language_pack"]["relaunch_count"],
                1
            );
        });
        let receipt = json!({"pids":pids,"initial_actual_wait":true,"initial_exit":0,
            "final_actual_wait_pid":waited,"final_wait_status":status,"envelope_count":1,
            "profile_removed":true,"home":isolated});
        std::fs::write(
            isolated.join("147-success-control.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }

    // Test-only ownership of the isolated executor, including assertion/I/O
    // unwinding. No blocking wait occurs until try_wait has actually reaped it.
    #[cfg(unix)]
    struct RestartExecutor {
        child: std::process::Child,
        reaped: bool,
        cleanup_attempted: bool,
    }

    #[cfg(unix)]
    impl RestartExecutor {
        fn new(child: std::process::Child) -> Self {
            Self {
                child,
                reaped: false,
                cleanup_attempted: false,
            }
        }

        fn poll_until(
            &mut self,
            deadline: std::time::Instant,
        ) -> std::io::Result<Option<std::process::ExitStatus>> {
            while std::time::Instant::now() < deadline {
                if let Some(found) = self.child.try_wait()? {
                    // try_wait already reaped this exact direct child. wait is
                    // now a cached actual outcome, not an unbounded live wait.
                    self.reaped = true;
                    let waited = self.child.wait()?;
                    assert_eq!(waited, found);
                    return Ok(Some(waited));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None)
        }

        fn cleanup(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            self.cleanup_attempted = true;
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            // Check actual exit before signalling; a reaped PID must never be
            // treated as a live owned child. This child owns its process group.
            if let Ok(Some(_)) = self.child.try_wait() {
                self.reaped = true;
                return self.child.wait().map(Some);
            }
            // A polling error must not skip the owned kill/reap attempt. Any
            // subsequent wait error is propagated as uncertainty, never success.
            let pid = self.child.id();
            let group = crate::util::process::get_process_group_id(pid)
                .filter(|g| i64::from(*g) == i64::from(pid));
            crate::util::process::kill_process_tree(pid, group);
            let _ = self.child.kill();
            self.poll_until(deadline)
        }

        fn finish(
            mut self,
            active: Duration,
        ) -> std::io::Result<(Option<std::process::ExitStatus>, bool)> {
            if let Some(status) = self.poll_until(std::time::Instant::now() + active)? {
                return Ok((Some(status), false));
            }
            // Exactly one cleanup reserve; Drop must not start a second one.
            self.cleanup().map(|status| (status, true))
        }
    }

    #[cfg(unix)]
    impl Drop for RestartExecutor {
        fn drop(&mut self) {
            if !self.reaped && !self.cleanup_attempted {
                // An I/O error/assertion cannot silently drop the owned child.
                // Uncertain cleanup is never converted into a successful wait.
                let pid = self.child.id();
                let outcome = self.cleanup();
                // stderr-ok: test-only cleanup outcome, not a success claim.
                eprintln!("147 executor guard drop pid={pid} actual_cleanup_outcome={outcome:?}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unit_147_restart_success_has_two_owned_starts_and_one_final_envelope() {
        use std::io::{Read, Seek};
        use std::os::unix::process::CommandExt;
        // Exercise the guard's actual timeout/kill/reap path without a browser,
        // profile or socket. A zero active deadline forces cleanup, not a sleep.
        let timeout_executor = RestartExecutor::new(
            std::process::Command::new("/bin/sleep")
                .arg("30")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let timeout_pid = timeout_executor.child.id();
        let (timeout_status, timed_out) = timeout_executor.finish(Duration::ZERO).unwrap();
        assert!(timed_out);
        assert!(
            timeout_status.is_some_and(|status| !status.success()),
            "forced cleanup did not actually reap owned executor {timeout_pid}"
        );
        // stderr-ok: owned harmless-child actual-wait evidence only.
        eprintln!(
            "147 executor guard timeout pid={timeout_pid} actual_wait=true status={timeout_status:?}"
        );
        let parent_override = std::env::var_os("FF_RDP_HOME");
        let root = tempfile::tempdir().unwrap().keep();
        let private = root.join("executor-home");
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("147-control-owner"), b"owned restart control").unwrap();
        // Model an external state home without touching any actual user's.
        // The executor also checks its exact state root before invoking launch.
        let external_home = root.join("external-home");
        let external = external_home.join(".ff-rdp");
        std::fs::create_dir_all(&external).unwrap();
        let sentinel = external.join("external-state.7363.json");
        let sentinel_bytes = b"external state must survive unchanged";
        std::fs::write(&sentinel, sentinel_bytes).unwrap();
        // Negative arm gives the executor the external home but a mismatching
        // ownership token. It must fail before invoking product code.
        for (case, selected_home, success_expected) in [
            ("reject-unisolated", &external_home, false),
            ("isolated", &private, true),
        ] {
            let mut log = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .read(true)
                .open(root.join(format!("{case}.log")))
                .unwrap();
            let executor = RestartExecutor::new(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "commands::launch::iter_175_tests::restart_success_isolated_executor",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("FF_RDP_HOME", selected_home)
                    .env("FF_RDP_147_CONTROL_HOME", &private)
                    .stdin(std::process::Stdio::null())
                    .stdout(log.try_clone().unwrap())
                    .stderr(log.try_clone().unwrap())
                    .process_group(0)
                    .spawn()
                    .unwrap(),
            );
            let (status, timed_out) = executor.finish(Duration::from_secs(15)).unwrap();
            assert_eq!(std::fs::read(&sentinel).unwrap(), sentinel_bytes);
            log.rewind().unwrap();
            let mut text = String::new();
            log.take(65537).read_to_string(&mut text).unwrap();
            assert!(
                !timed_out && status.is_some_and(|s| s.success() == success_expected),
                "isolated restart control failed; actual executor wait {status:?}; retained {}: {text}",
                root.display()
            );
            assert!(
                if success_expected {
                    text.contains("test result: ok. 1 passed; 0 failed;")
                } else {
                    text.contains("test result: FAILED. 0 passed; 1 failed;")
                },
                "executor selection/outcome mismatch: {text}"
            );
            if !success_expected {
                assert!(
                    text.contains("147 isolation home mismatch before launch"),
                    "wrong negative path: {text}"
                );
                assert!(!private.join("147-success-control.json").exists());
            }
            // stderr-ok: finite test-only actual-wait outcomes; no user record content.
            eprintln!(
                "147 isolation case={case} actual_executor_wait=true exit={:?} sentinel_unchanged=true",
                status.and_then(|s| s.code())
            );
        }
        let receipt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(private.join("147-success-control.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["pids"].as_array().unwrap().len(), 2);
        assert_eq!(receipt["initial_actual_wait"], true);
        assert_eq!(receipt["initial_exit"], 0);
        assert_eq!(receipt["final_actual_wait_pid"], receipt["pids"][1]);
        assert_eq!(receipt["final_wait_status"], 0);
        assert_eq!(receipt["envelope_count"], 1);
        assert_eq!(receipt["profile_removed"], true);
        assert_eq!(receipt["home"], json!(private));
        assert_eq!(std::fs::read(&sentinel).unwrap(), sentinel_bytes);
        assert_eq!(std::env::var_os("FF_RDP_HOME"), parent_override);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
