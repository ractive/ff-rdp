//! Explicit installation lifecycle, never an automatic locale retry.
//!
//! Firefox revision 6f2c158dfc7e9693f880fad2510ceb51a158c069:
//! preferences/config/languages.mjs:158–178 supports apply-and-restart;
//! XPIProvider.sys.mjs:1839–1883 finalizes browser-generated startup state at
//! normal shutdown. intl/docs/locale_startup.md does NOT guarantee first-bundle
//! ordering. Only a real engine test establishes the resulting diagnostic locale.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use ff_rdp_core::transport::RdpTransport;
use ff_rdp_core::types::{ActorId, Grip};
use ff_rdp_core::{RootActor, TabActor, WebConsoleActor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::english_language_pack::{self, Pack, read_regular};
use super::{LaunchHooks, USER_JS, startup};
use crate::error::AppError;
use crate::util::process;
use crate::util::profile_dir::ManagedProfileGuard;

const INITIALIZATION: Duration = Duration::from_secs(120);
const READY: Duration = Duration::from_secs(15);
const QUIT: Duration = Duration::from_secs(30);
const DOM_MEMBER: &str = "chrome/en-US/locale/en-US/global/dom/dom.properties";

pub(super) fn validate_options(enabled: bool, pack: bool, host: &str) -> Result<(), AppError> {
    if !enabled {
        return Ok(());
    }
    if !pack {
        return Err(AppError::User(
            "--restart-after-language-pack-install requires --english-language-pack".into(),
        ));
    }
    // This first implementation needs the existing Unix birth/group queries.
    // Do not silently degrade ownership on another platform.
    if !cfg!(unix) {
        return Err(AppError::User(
            "language-pack initialization currently requires Unix process ownership support".into(),
        ));
    }
    if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]") {
        return Err(AppError::User(
            "language-pack initialization requires a local loopback host".into(),
        ));
    }
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    ensure!(
        !left.is_zero(),
        "language-pack initialization deadline expired"
    );
    Ok(left)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) struct Runtime {
    binary: PathBuf,
    binary_hash: String,
    ini: PathBuf,
    ini_hash: String,
    version: String,
    build: String,
}

impl Runtime {
    pub(super) fn read(binary: &Path) -> Result<Self> {
        let binary = fs::canonicalize(binary)?;
        let parent = binary.parent().context("Firefox directory absent")?;
        let ini = if parent.file_name().is_some_and(|p| p == "MacOS") {
            parent
                .parent()
                .context("Firefox Contents absent")?
                .join("Resources/application.ini")
        } else {
            parent.join("application.ini")
        };
        let bytes = read_regular(&ini, 64 * 1024)?;
        let mut fields = BTreeMap::new();
        let mut app = false;
        for line in std::str::from_utf8(&bytes)?.lines().map(str::trim) {
            if line.starts_with('[') {
                app = line == "[App]";
            }
            if app
                && let Some((key, value)) = line.split_once('=')
                && matches!(key, "Version" | "BuildID")
            {
                ensure!(
                    fields.insert(key.to_owned(), value.to_owned()).is_none(),
                    "duplicate runtime field"
                );
            }
        }
        let version = fields
            .remove("Version")
            .filter(|s| !s.is_empty())
            .context("runtime Version absent")?;
        let build = fields
            .remove("BuildID")
            .filter(|s| !s.is_empty())
            .context("runtime BuildID absent")?;
        Ok(Self {
            binary_hash: digest(&read_regular(&binary, 256 * 1024 * 1024)?),
            binary,
            ini,
            ini_hash: digest(&bytes),
            version,
            build,
        })
    }

    fn verify(&self) -> Result<()> {
        ensure!(
            digest(&read_regular(&self.binary, 256 * 1024 * 1024)?) == self.binary_hash
                && digest(&read_regular(&self.ini, 64 * 1024)?) == self.ini_hash,
            "Firefox runtime identity changed during initialization"
        );
        Ok(())
    }
}

// These helpers are OS queries only. Files avoid pipe backpressure; every query
// child has a finite actual wait, including failure cleanup. File limits are
// acceptance/read limits, not an assertion that the OS cannot emit more bytes.
fn bounded_output(program: &str, args: &[&str], deadline: Instant) -> Result<(ExitStatus, String)> {
    let end = deadline
        .checked_sub(Duration::from_secs(2))
        .context("query cleanup reserve absent")?
        .min(Instant::now() + Duration::from_secs(3));
    remaining(end)?;
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    let status = match wait_child(&mut child, end, None) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let waited = wait_child(
                &mut child,
                deadline.min(Instant::now() + Duration::from_secs(2)),
                None,
            );
            return Err(error.context(format!("query cleanup actual wait: {waited:?}")));
        }
    };
    ensure!(
        stdout.metadata()?.len() <= 1024 * 1024 && stderr.metadata()?.len() <= 65536,
        "ownership query output exceeded bound"
    );
    stdout.rewind()?;
    stderr.rewind()?;
    let mut out = String::new();
    let mut err = String::new();
    stdout.take(1024 * 1024 + 1).read_to_string(&mut out)?;
    stderr.take(65537).read_to_string(&mut err)?;
    ensure!(err.trim().is_empty(), "ownership query reported an error");
    Ok((status, out))
}

fn wait_child(
    child: &mut Child,
    deadline: Instant,
    mut stderr: Option<&mut startup::StderrCapture>,
) -> Result<ExitStatus> {
    loop {
        if let Some(capture) = stderr.as_deref_mut() {
            capture.pump(Some(deadline));
        }
        if let Some(status) = child.try_wait()? {
            // try_wait has collected this child's actual status; wait is now
            // immediate and retains the ordinary Child wait contract.
            let waited = child.wait()?;
            ensure!(status == waited, "child status changed after reap");
            return Ok(waited);
        }
        let left = remaining(deadline)?;
        std::thread::sleep(left.min(Duration::from_millis(10)));
    }
}

fn listener(port: u16, deadline: Instant) -> Result<Option<u32>> {
    let port = format!("-iTCP:{port}");
    let (status, out) = bounded_output("lsof", &["-nP", &port, "-sTCP:LISTEN", "-Fp"], deadline)?;
    parse_listener(status.code(), &out).with_context(|| {
        format!(
            "listener query {port}: exit={:?}, stdout_bytes={}, stdout_sha256={}",
            status.code(),
            out.len(),
            digest(out.as_bytes())
        )
    })
}

fn parse_listener(code: Option<i32>, out: &str) -> Result<Option<u32>> {
    if code == Some(1) && out.trim().is_empty() {
        return Ok(None);
    }
    ensure!(code == Some(0), "listener query failed");
    let mut pids = BTreeSet::new();
    // lsof -Fp also emits file-set starts (f), even when only p was requested.
    // TCP listener records use decimal descriptors; never treat them as PIDs
    // or silently discard fields outside this query's narrow p/f grammar.
    for (index, line) in out.lines().enumerate() {
        let (tag, value) = line
            .as_bytes()
            .split_first()
            .context("empty listener field")?;
        ensure!(
            !value.is_empty() && value.iter().all(u8::is_ascii_digit),
            "nondecimal listener field at line {}",
            index + 1
        );
        let number = std::str::from_utf8(value)?.parse::<u32>()?;
        match tag {
            b'p' => {
                ensure!(number > 0, "listener PID must be positive");
                pids.insert(number);
            }
            b'f' => ensure!(!pids.is_empty(), "listener descriptor precedes process"),
            _ => bail!("unexpected listener tag 0x{tag:02x} at line {}", index + 1),
        }
    }
    ensure!(pids.len() == 1, "listener owner missing or ambiguous");
    Ok(pids.into_iter().next())
}

#[derive(Clone)]
struct Row {
    parent: u32,
    group: u32,
}

fn processes(deadline: Instant) -> Result<BTreeMap<u32, Row>> {
    let (status, text) = bounded_output("ps", &["-axo", "pid=,ppid=,pgid="], deadline)?;
    ensure!(status.success(), "process query failed");
    parse_processes(&text)
}

fn parse_processes(text: &str) -> Result<BTreeMap<u32, Row>> {
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(fields.len() == 3, "incomplete process row");
        let pid = fields[0].parse()?;
        ensure!(
            rows.insert(
                pid,
                Row {
                    parent: fields[1].parse()?,
                    group: fields[2].parse()?
                }
            )
            .is_none(),
            "duplicate process row"
        );
    }
    ensure!(!rows.is_empty(), "empty process census");
    Ok(rows)
}

fn is_descendant(rows: &BTreeMap<u32, Row>, mut pid: u32, ancestor: u32) -> bool {
    let mut seen = BTreeSet::new();
    while let Some(row) = rows.get(&pid) {
        if pid == ancestor {
            return true;
        }
        if !seen.insert(pid) {
            return false;
        }
        pid = row.parent;
    }
    false
}

struct Owner {
    pid: u32,
    birth: String,
    members: BTreeMap<u32, String>,
}

impl Owner {
    fn capture(pid: u32, port: u16, profile: &Path, deadline: Instant) -> Result<Self> {
        let birth = process::process_start_token(pid).context("initializer birth unavailable")?;
        let mut owner = Self {
            pid,
            birth,
            members: BTreeMap::new(),
        };
        owner.verify(port, profile, deadline)?;
        owner.sample_descendants(deadline)?;
        Ok(owner)
    }

    fn verify(&self, port: u16, profile: &Path, deadline: Instant) -> Result<()> {
        ensure!(
            process::process_start_token(self.pid).as_deref() == Some(self.birth.as_str()),
            "initializer identity changed"
        );
        ensure!(
            process::get_process_group_id(self.pid).map(i64::from) == Some(i64::from(self.pid)),
            "initializer isolated group absent"
        );
        ensure!(
            listener(port, deadline)? == Some(self.pid),
            "initializer does not own listener"
        );
        let marker = read_regular(&profile.join(".ff-rdp-owner-pid"), 64)?;
        ensure!(
            std::str::from_utf8(&marker)?.trim() == self.pid.to_string(),
            "initializer profile marker mismatch"
        );
        Ok(())
    }

    fn sample_descendants(&mut self, deadline: Instant) -> Result<()> {
        let rows = processes(deadline)?;
        ensure!(
            rows.get(&self.pid).is_some_and(|r| r.group == self.pid),
            "initializer missing from census"
        );
        for (&pid, row) in &rows {
            if row.group == self.pid || is_descendant(&rows, pid, self.pid) {
                let birth = process::process_start_token(pid)
                    .context("owned member identity unavailable")?;
                self.members.insert(pid, birth);
            }
        }
        Ok(())
    }

    fn absent(&self, port: u16, deadline: Instant) -> Result<()> {
        let rows = processes(deadline)?;
        ensure!(
            !rows.values().any(|r| r.group == self.pid),
            "initializer group still present"
        );
        for (pid, birth) in &self.members {
            if rows.contains_key(pid) {
                let current = process::process_start_token(*pid)
                    .context("recorded member identity unknown")?;
                ensure!(&current != birth, "initializer descendant survives");
            }
        }
        ensure!(
            listener(port, deadline)?.is_none(),
            "initializer listener still present or replaced"
        );
        Ok(())
    }
}

// Keep284's bounded nonblocking pump running across synchronous ownership and
// RDP operations. The worker borrows the capture inside a scope, is stopped on
// every return/unwind, and is actually joined before any handoff or cleanup.
// The sleep schedules a pipe reader, not a readiness retry or browser warm-up.
fn with_stderr_pump<T>(
    stderr: &mut startup::StderrCapture,
    deadline: Instant,
    operation: impl FnOnce() -> Result<T>,
) -> Result<(T, String)> {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    remaining(deadline)?;
    let stopped = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let stopped_ref = &stopped;
        let reader = std::thread::Builder::new()
            .name("ff-rdp-initializer-stderr".into())
            .spawn_scoped(scope, move || {
                while !stopped_ref.load(Ordering::Acquire) && Instant::now() < deadline {
                    stderr.pump(Some(deadline));
                    let left = deadline.saturating_duration_since(Instant::now());
                    if !left.is_zero() {
                        std::thread::sleep(left.min(Duration::from_millis(5)));
                    }
                }
                stderr.pump(Some(deadline));
                stderr.summary()
            })
            .context("spawn initializer stderr pump")?;
        let stop = Stop(&stopped);
        let result = operation();
        drop(stop);
        let summary = reader.join().map_err(|_| {
            anyhow::anyhow!("initializer stderr pump panicked; actual join completed")
        })?;
        let value = result?;
        remaining(deadline)?;
        Ok((value, summary))
    })
}

struct Parent {
    transport: RdpTransport,
    console: ActorId,
}

impl Parent {
    fn connect(port: u16, deadline: Instant) -> Result<Self> {
        let mut transport = RdpTransport::connect_raw(
            "127.0.0.1",
            port,
            remaining(deadline)?.min(Duration::from_secs(2)),
        )?;
        transport
            .try_clone_stream()?
            .set_write_timeout(Some(remaining(deadline)?.min(Duration::from_secs(2))))?;
        let console = transport.with_read_deadline(deadline, |t| {
            t.recv()?;
            let descriptor = RootActor::get_process(t, 0)?;
            Ok(TabActor::get_process_target(t, &descriptor)?.console_actor)
        })?;
        Ok(Self { transport, console })
    }

    fn evaluate(&mut self, text: &str, deadline: Instant) -> Result<Value> {
        self.transport
            .try_clone_stream()?
            .set_write_timeout(Some(remaining(deadline)?.min(Duration::from_secs(2))))?;
        let result = self.transport.with_read_deadline(deadline, |t| {
            WebConsoleActor::evaluate_js_async(t, &self.console, text)
        })?;
        ensure!(
            result.exception.is_none(),
            "initializer parent evaluation exception (no fallback)"
        );
        let Grip::Value(Value::String(text)) = result.result else {
            bail!("initializer result is not a primitive string");
        };
        ensure!(text.len() <= 8192, "initializer result exceeds bound");
        remaining(deadline)?;
        Ok(serde_json::from_str(&text)?)
    }
}

fn binding_js(pid: u32, profile: &Path) -> Result<String> {
    let profile = serde_json::to_string(profile.to_str().context("profile path is not Unicode")?)?;
    Ok(format!(
        r#"if (Services.appinfo.processType !== Components.interfaces.nsIXULRuntime.PROCESS_TYPE_DEFAULT || Services.appinfo.processID !== {pid} || Services.dirsvc.get("ProfD", Components.interfaces.nsIFile).path !== {profile}) throw new Error("owned initializer binding mismatch");"#
    ))
}

fn readiness_js(pid: u32, profile: &Path) -> Result<String> {
    Ok(format!(
        r#"(async () => {{
      {}
      const {{AddonManager}} = ChromeUtils.importESModule("resource://gre/modules/AddonManager.sys.mjs");
      await AddonManager.readyPromise;
      const addon = await AddonManager.getAddonByID("{}");
      if (!addon) throw new Error("English language pack absent");
      const chrome = Components.classes["@mozilla.org/chrome/chrome-registry;1"].getService(Components.interfaces.nsIXULChromeRegistry);
      return JSON.stringify({{schema:1, pid:Services.appinfo.processID,
        build:Services.appinfo.appBuildID, version:Services.appinfo.version, ready:AddonManager.isReady,
        id:addon.id, pack_version:addon.version, type:addon.type, scope:addon.scope,
        signed:addon.signedState, active:addon.isActive, user_disabled:addon.userDisabled,
        app_disabled:addon.appDisabled, root:addon.getResourceURI("").spec,
        dom:chrome.convertChromeURL(Services.io.newURI("chrome://global/locale/dom/dom.properties")).spec}});
    }})()"#,
        binding_js(pid, profile)?,
        english_language_pack::ID
    ))
}

fn expected_root(profile: &Path) -> Result<String> {
    let file = url::Url::from_file_path(
        profile
            .join("extensions")
            .join(format!("{}.xpi", english_language_pack::ID)),
    )
    .map_err(|()| anyhow::anyhow!("cannot encode owned extension path"))?;
    Ok(format!("jar:{file}!/"))
}

fn qualify_ready(
    v: &Value,
    pid: u32,
    profile: &Path,
    pack: &Pack,
    runtime: &Runtime,
) -> Result<()> {
    ensure!(
        v["schema"] == 1
            && v["pid"] == pid
            && v["build"] == runtime.build
            && v["version"] == runtime.version,
        "initializer parent/build mismatch"
    );
    ensure!(
        v["ready"] == true
            && v["id"] == english_language_pack::ID
            && v["pack_version"] == pack.version()
            && v["type"] == "locale"
            && v["scope"] == 1
            && v["active"] == true
            && v["user_disabled"] == false
            && v["app_disabled"] == false
            && matches!(v["signed"].as_i64(), Some(1..=4)),
        "initializer addon state unqualified"
    );
    let root = expected_root(profile)?;
    ensure!(
        v["root"] == root && v["dom"] == format!("{root}{DOM_MEMBER}"),
        "initializer owned provider unqualified"
    );
    Ok(())
}

fn persisted(profile: &Path, pack: &Pack) -> Result<Value> {
    pack.verify_staged(profile, USER_JS)?;
    let bytes = read_regular(&profile.join("extensions.json"), 2 * 1024 * 1024)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    let addons = value["addons"]
        .as_array()
        .context("extensions database addons absent")?;
    let entries: Vec<_> = addons
        .iter()
        .filter(|a| a["id"] == english_language_pack::ID)
        .collect();
    ensure!(
        entries.len() == 1,
        "persisted language pack absent/ambiguous"
    );
    let a = entries[0];
    let path = profile
        .join("extensions")
        .join(format!("{}.xpi", english_language_pack::ID));
    ensure!(
        a["version"] == pack.version()
            && a["type"] == "locale"
            && a["location"] == "app-profile"
            && a["path"].as_str() == path.to_str()
            && a["active"] == true
            && a["visible"] == true
            && a["userDisabled"] == false
            && a["appDisabled"] == false
            && matches!(a["signedState"].as_i64(), Some(1..=4)),
        "persisted language pack unqualified"
    );
    let entries = a["startupData"]["chromeEntries"]
        .as_array()
        .context("persisted chrome entries absent")?;
    ensure!(
        entries
            .iter()
            .filter(|v| *v
                == &json!([
                    "locale",
                    "global",
                    "en-US",
                    "chrome/en-US/locale/en-US/global/"
                ]))
            .count()
            == 1,
        "persisted global registration missing/ambiguous"
    );
    let startup = read_regular(&profile.join("addonStartup.json.lz4"), 2 * 1024 * 1024)?;
    ensure!(!startup.is_empty(), "browser startup state empty");
    Ok(
        json!({"extensions_sha256":digest(&bytes),"addon_startup_sha256":digest(&startup),
        "startup_cache_contents_attested":false}),
    )
}

// Only a receive EOF may race the normal quit acknowledgement. An exception,
// send error, malformed response or timeout is terminal even if Firefox exits.
fn qualify_quit(result: Result<Value>) -> Result<Value> {
    match result {
        Ok(value) => {
            ensure!(
                value["accepted"] == true,
                "initializer quit cancelled or malformed"
            );
            Ok(json!({"acknowledged":true}))
        }
        Err(error) => {
            let eof = matches!(error.downcast_ref::<ff_rdp_core::error::ProtocolError>(),
                Some(ff_rdp_core::error::ProtocolError::RecvFailed(io))
                    if io.kind() == std::io::ErrorKind::UnexpectedEof);
            ensure!(eof, "initializer quit failed: {error:#}");
            Ok(json!({"acknowledged":false,"receive_eof":true}))
        }
    }
}

// Distinct from the ordinary launch guard only because this retained first
// child has a bounded initialization/cleanup contract. Forced cleanup NEVER
// yields a successful initialization or authorizes an operational spawn.
struct Initializer {
    child: Child,
    stderr: startup::StderrCapture,
    armed: bool,
}
impl Initializer {
    fn disarm(&mut self) {
        self.armed = false;
    }
    fn fail(&mut self, error: AppError, deadline: Instant) -> AppError {
        self.armed = false;
        self.stderr.pump(Some(deadline));
        let error = error.with_warning(self.stderr.summary());
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                let pid = self.child.id();
                let group = process::get_process_group_id(pid)
                    .filter(|group| i64::from(*group) == i64::from(pid));
                process::kill_process_tree(pid, group);
                if let Err(kill) = self.child.kill() {
                    // Still attempt the finite actual wait: exit may race kill.
                    tracing::warn!("initializer cleanup kill: {kill}");
                }
            }
            Err(status) => {
                return error.with_warning(format!(
                    "initializer status unknown; profile retained: {status}"
                ));
            }
        }
        match wait_child(&mut self.child, deadline, Some(&mut self.stderr)) {
            Ok(status) => {
                error.with_warning(format!("initializer cleanup actual Child wait: {status}"))
            }
            Err(wait) => error.with_warning(format!(
                "initializer cleanup wait unqualified; profile retained: {wait:#}"
            )),
        }
    }
}
impl Drop for Initializer {
    fn drop(&mut self) {
        if self.armed {
            let error = self.fail(
                AppError::User("language-pack initializer unwound".into()),
                Instant::now() + Duration::from_secs(20),
            );
            tracing::warn!("{error}");
        }
    }
}

// A first child is never detached. The profile guard is kept separately until
// the process and its captured group/descendants are proven absent. On unknown
// cleanup, disarm the guard and leave the attributable profile for recovery.
pub(super) fn run(
    firefox: &Path,
    port: u16,
    profile: &Path,
    pack: &Pack,
    mut profile_guard: ManagedProfileGuard,
    hooks: &LaunchHooks,
    runtime: &Runtime,
) -> Result<(ManagedProfileGuard, Value), AppError> {
    let started = Instant::now();
    let deadline = started + INITIALIZATION;
    runtime.verify().map_err(|e| {
        super::report_failed_profile_cleanup(
            AppError::User(format!("language-pack initialization preflight: {e:#}")),
            &mut profile_guard,
        )
    })?;
    let mut cmd = super::browser_command(firefox, port, true, None, Some("about:blank"));
    cmd.arg("--profile").arg(profile);
    let child = (hooks.spawn)(&mut cmd).map_err(|e| {
        super::report_failed_profile_cleanup(
            AppError::User(format!("language-pack initializer spawn: {e}")),
            &mut profile_guard,
        )
    })?;
    let pid = child.id();
    // Initializer can stop/reap the Child but cannot delete this profile.
    let mut pending = Initializer {
        child,
        armed: true,
        stderr: startup::StderrCapture::default(),
    };
    // Prevent an unwinding caller from removing a live initializer's profile.
    profile_guard.disarm();
    let mut owner: Option<Owner> = None;
    let outcome = (|| -> Result<Value> {
        pending.stderr.attach(pending.child.stderr.take())?;
        crate::util::profile_dir::write_owner_pid_marker(profile, pid);
        let mut observation = startup::Observation {
            child: &mut pending.child,
            stderr: &mut pending.stderr,
            try_wait: hooks.try_wait,
            started,
        };
        ensure!(
            observation.initial_interval()?.is_none(),
            "initializer exited immediately"
        );
        let bound = remaining(deadline)?.min(Duration::from_secs(60));
        if let Some(error) = (hooks.probe_port)("127.0.0.1", port, bound, &mut observation)
            .into_error(pid, port, bound)
        {
            bail!("{error}");
        }
        let (mut receipt, stderr_summary) = with_stderr_pump(
            &mut pending.stderr,
            deadline,
            || {
                owner = Some(Owner::capture(pid, port, profile, deadline)?);
                let ready_deadline = deadline.min(Instant::now() + READY);
                let mut parent = Parent::connect(port, ready_deadline)?;
                let readiness = parent.evaluate(&readiness_js(pid, profile)?, ready_deadline)?;
                qualify_ready(&readiness, pid, profile, pack, runtime)?;
                let identity = owner.as_mut().context("initializer owner missing")?;
                identity.verify(port, profile, deadline)?;
                identity.sample_descendants(deadline)?;
                let quit_deadline = deadline.min(Instant::now() + QUIT);
                // nsIAppStartup.idl: eAttemptQuit is ordinary shutdown, no eRestart.
                // EOF can race the response; it is recorded, never called an ack.
                let expression = format!(
                    "(() => {{ {} return JSON.stringify({{accepted:Services.startup.quit(Components.interfaces.nsIAppStartup.eAttemptQuit,0)}}); }})()",
                    binding_js(pid, profile)?
                );
                let quit = parent.evaluate(&expression, quit_deadline);
                let quit_receipt = qualify_quit(quit)?;
                let status = wait_child(&mut pending.child, quit_deadline, None)?;
                ensure!(
                    status.success(),
                    "initializer actual child exit was {status}"
                );
                identity.absent(port, deadline)?;
                runtime.verify()?;
                let persisted = persisted(profile, pack)?;
                identity.absent(port, deadline)?;
                remaining(deadline)?;
                Ok(
                    json!({"pid":pid,"start_token":identity.birth,"actual_child_wait":true,"exit_code":status.code(),
            "profile":profile,"port":port,"owned_members":identity.members,
            "owned_descendants_absent":true,"listener_absent":true,
            "runtime":{"binary_sha256":runtime.binary_hash,"application_ini_sha256":runtime.ini_hash},
            "quit":quit_receipt,"persisted":persisted,"elapsed_ms":started.elapsed().as_millis(),
            "readiness":readiness,"engine_locale_attested":false}),
                )
            },
        )?;
        receipt["elapsed_ms"] = json!(started.elapsed().as_millis());
        receipt["stderr_pump_actual_join"] = json!(true);
        receipt["stderr_summary"] = json!(stderr_summary);
        Ok(receipt)
    })();
    match outcome {
        Ok(receipt) => {
            pending.disarm();
            // The caller continues the same managed profile lifecycle. No
            // creation, staging, or USER_JS append occurs on the second start.
            Ok((ManagedProfileGuard::armed(profile), receipt))
        }
        Err(error) => {
            let cleanup_deadline = Instant::now() + Duration::from_secs(20);
            let mut error = pending.fail(
                AppError::User(format!(
                    "language-pack initialization failed (no relaunch): {error:#}"
                )),
                cleanup_deadline,
            );
            let absent = owner
                .as_ref()
                .is_some_and(|owner| owner.absent(port, cleanup_deadline).is_ok());
            if absent && matches!(pending.child.try_wait(), Ok(Some(_))) {
                let mut guard = ManagedProfileGuard::armed(profile);
                error = super::report_failed_profile_cleanup(error, &mut guard);
            } else {
                error = error.with_warning(format!(
                    "initializer ownership/descendant absence unqualified; profile retained: {}",
                    profile.display()
                ));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
