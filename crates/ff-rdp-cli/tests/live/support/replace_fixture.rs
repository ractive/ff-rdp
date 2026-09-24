//! Iter153 paired ownership fixture. Every home and command receipt survives unwind.
//! Native start-token lookup mirrors daemon/process.rs because the CLI has no lib target.
use crate::common::{self, LIVE_LAUNCH_LOG_ENV, OWNER_PID_MARKER, OWNER_TEST_MARKER};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

pub fn process_start_token(pid: u32) -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: `proc_pidinfo` writes at most `size` bytes into the buffer we
        // pass, and `size` is exactly `size_of::<proc_bsdinfo>()`. The pointer
        // comes from a live, correctly-aligned `MaybeUninit<proc_bsdinfo>` that
        // outlives the call. The only side effect is filling that buffer; a
        // non-existent or inaccessible PID is reported through the return
        // value, which we check against the full struct size before reading.
        #[allow(clippy::cast_possible_wrap)]
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast::<libc::c_void>(),
                size,
            )
        };
        if written != size {
            return None;
        }
        // SAFETY: `proc_pidinfo` returned exactly `size_of::<proc_bsdinfo>()`
        // bytes written, so the buffer is fully initialised.
        let info = unsafe { info.assume_init() };
        Some(format!(
            "{}.{:06}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        ))
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Field 22 of /proc/<pid>/stat is `starttime`. Fields 1 and 2 are the
        // PID and the comm, and comm is parenthesised and may itself contain
        // spaces and ')' — so split after the LAST ')' rather than tokenising
        // the whole line.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_comm = &stat[stat.rfind(')')? + 1..];
        // After the comm, field 3 is `state`; `starttime` is field 22, i.e.
        // the 20th whitespace-separated token of this remainder.
        let starttime = after_comm.split_whitespace().nth(19)?;
        (!starttime.is_empty()).then(|| starttime.to_owned())
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: `OpenProcess` only returns a handle (or NULL); we close it on
        // every path below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut creation = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exit = creation;
        let mut kernel = creation;
        let mut user = creation;
        // SAFETY: `handle` is a valid handle we just opened, and all four
        // out-pointers reference live, initialised `FILETIME` locals that
        // outlive the call.
        let ok = unsafe {
            GetProcessTimes(
                handle,
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
        };
        // SAFETY: `handle` is the valid handle opened above and is not used
        // again after this call.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        (ticks != 0).then(|| ticks.to_string())
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    )))]
    {
        let _ = pid;
        None
    }
}

fn save(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap_or_else(|e| panic!("retain {}: {e}", path.display()));
    file.write_all(bytes)
        .expect("write retained fixture receipt");
    file.sync_all().expect("sync retained fixture receipt");
}
fn save_json(path: &Path, value: &Value) {
    save(
        path,
        &serde_json::to_vec_pretty(value).expect("serialize receipt"),
    );
}
pub fn home() -> PathBuf {
    let ledger = std::env::var_os(LIVE_LAUNCH_LOG_ENV).map(PathBuf::from);
    let home = common::retained_failed_launch_home(ledger.as_deref()).expect("retained153 home");
    eprintln!("live153 retained home={}", home.display());
    home
}

/// A test-owned incarnation. Missing native identity never authorizes cleanup.
pub struct ProcessGuard {
    pid: u32,
    token: String,
    home: PathBuf,
    role: &'static str,
}
impl ProcessGuard {
    pub fn new(pid: u32, home: &Path) -> Self {
        assert!(pid > 1, "refuse non-process cleanup target");
        let token = process_start_token(pid)
            .expect("native birth required before fixture cleanup ownership");
        let root = home.join("ff-rdp/profiles");
        trusted_root(&root);
        let owned = fs::read_dir(&root)
            .expect("private ownership root")
            .any(|entry| {
                let Ok(entry) = entry else {
                    return false;
                };
                let path = entry.path();
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ff-rdp-profile-")
                    && fs::read_to_string(path.join(OWNER_PID_MARKER))
                        .is_ok_and(|v| v.trim() == pid.to_string())
                    && fs::read_to_string(path.join(".ff-rdp-owner-start"))
                        .is_ok_and(|v| v.trim() == token)
            });
        assert!(
            owned,
            "cleanup requires the real marker/token in this fixture home"
        );
        Self {
            pid,
            token,
            home: home.to_path_buf(),
            role: "browser",
        }
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let current = process_start_token(self.pid);
        let matches = current.as_ref() == Some(&self.token);
        // Never panic during unwind or erase evidence when cleanup cannot run.
        let receipt = self.home.join(format!("cleanup-{}.jsonl", self.pid));
        let mut log = match OpenOptions::new().create(true).append(true).open(&receipt) {
            Ok(log) => log,
            Err(e) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "cannot retain cleanup; no signal sent: {e}; pid={}",
                    self.pid
                );
                return;
            }
        };
        let row = json!({"pid":self.pid,"expected_birth":self.token,"role":self.role,"observed_birth":current,"signal_authorized":matches,"worker_return_inferred":false});
        if writeln!(log, "{row}")
            .and_then(|()| log.sync_all())
            .is_err()
        {
            let _ = writeln!(
                std::io::stderr(),
                "cleanup receipt failed; no signal sent; pid={}",
                self.pid
            );
            return;
        }
        if matches {
            let rechecked = process_start_token(self.pid);
            let signal = if rechecked.as_ref() == Some(&self.token) {
                signal_pid(self.pid)
                    .map_err(|e| json!({"error":e.to_string(),"os_error":e.raw_os_error()}))
            } else {
                Err(
                    json!({"refused":"birth changed or unavailable before signal","observed":rechecked}),
                )
            };
            let action = json!({"pid":self.pid,"role":self.role,"expected_birth":self.token,"action":if cfg!(windows){"TerminateProcess(1)"}else{"SIGKILL"},"signal_result":signal});
            if writeln!(log, "{action}")
                .and_then(|()| log.sync_all())
                .is_err()
            {
                let _ = writeln!(std::io::stderr(), "cleanup signal receipt failed: {action}");
            }
            let gone = common::wait_for_pid_exit(self.pid, common::kill_wait_timeout()).is_some();
            let result = json!({"pid":self.pid,"exit_observed":gone,"after_birth":process_start_token(self.pid),"pid_alive":common::pid_alive(self.pid),"actual_child_wait":"not our direct child","worker_return_inferred":false});
            if writeln!(log, "{result}")
                .and_then(|()| log.sync_all())
                .is_err()
            {
                let _ = writeln!(
                    std::io::stderr(),
                    "cleanup result write failed; pid={}",
                    self.pid
                );
            }
        }
    }
}

fn signal_pid(pid: u32) -> std::io::Result<i32> {
    #[cfg(unix)]
    {
        let pid = libc::pid_t::try_from(pid).map_err(std::io::Error::other)?;
        // SAFETY: caller just revalidated the fixture-owned PID/birth; this
        // targets only that PID, never a group or a name-based process set.
        let rc = unsafe { libc::kill(pid, libc::SIGKILL) };
        if rc == 0 {
            Ok(rc)
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };
        // SAFETY: caller validated fixture ownership; a null handle is rejected
        // and the valid handle is closed on every path after TerminateProcess.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: handle is valid and targets the revalidated fixture PID.
        let rc = unsafe { TerminateProcess(handle, 1) };
        let result = if rc == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(rc)
        };
        // SAFETY: handle is valid and is not used again.
        unsafe { CloseHandle(handle) };
        result
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no scoped signal API",
        ))
    }
}

pub fn guard_launched_firefox(bytes: &[u8], home: &Path) -> Option<ProcessGuard> {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| v["results"]["pid"].as_u64())
        .and_then(|pid| u32::try_from(pid).ok())
        .map(|pid| ProcessGuard::new(pid, home))
}

#[derive(serde::Deserialize)]
struct LaunchRecord {
    pid: u32,
    port: u16,
    profile_dir: PathBuf,
    start_token: String,
}
#[derive(serde::Deserialize)]
struct ProxyRecord {
    pid: u32,
    firefox_port: u16,
    firefox_host: String,
    proxy_port: u16,
    start_token: String,
}

pub struct Browser {
    guard: ProcessGuard,
    port: u16,
    home: PathBuf,
    profile: PathBuf,
    record: Vec<u8>,
    markers: Vec<(String, Vec<u8>)>,
    proxies: Vec<ProcessGuard>,
}
impl Browser {
    pub fn pid(&self) -> u32 {
        self.guard.pid
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    fn record_path(&self) -> PathBuf {
        self.home
            .join(format!(".ff-rdp/launch-record.{}.json", self.port))
    }
    pub fn assert_survives(&self, label: &str) {
        assert_eq!(
            process_start_token(self.pid()).as_ref(),
            Some(&self.guard.token),
            "original Firefox incarnation must survive"
        );
        assert!(self.profile.is_dir(), "original profile must survive");
        assert_eq!(
            fs::read(self.record_path()).expect("original launch record survives"),
            self.record
        );
        self.assert_markers();
        assert_listener(self.port, self.pid(), &self.home, label);
        assert_eq!(
            process_start_token(self.pid()).as_ref(),
            Some(&self.guard.token),
            "same incarnation after listener observation"
        );
        save_json(
            &self.home.join(format!("{label}-survival.json")),
            &json!({"pid":self.pid(),"birth":self.guard.token,"profile":self.profile,"listener_pid":self.pid(),"record_and_markers_unchanged":true,"before_cleanup":true}),
        );
    }
    fn assert_markers(&self) {
        for (name, bytes) in &self.markers {
            assert_eq!(
                &fs::read(self.profile.join(name)).expect("ownership marker survives"),
                bytes,
                "marker {name} changed"
            );
        }
    }
    pub fn assert_stopped(&self) {
        let current = process_start_token(self.pid());
        assert!(
            current.as_ref().is_some_and(|t| t != &self.guard.token)
                || !common::pid_alive(self.pid()),
            "prior Firefox incarnation remains or cannot be proved ended"
        );
        save_json(
            &self.home.join("prior-incarnation-ended.json"),
            &json!({"pid":self.pid(),"prior_birth":self.guard.token,"observed_birth":current,"pid_alive":common::pid_alive(self.pid()),"worker_return_inferred":false}),
        );
    }
    fn archive_lookup(&self) {
        self.assert_survives("before-record-removal");
        archive_record(
            &self.record_path(),
            &self.home.join("fixture-launch-record.archive.json"),
            &self.record,
        );
        assert!(
            !self.record_path().exists(),
            "registry branch requires no active launch record"
        );
        self.assert_markers();
        save_json(
            &self.home.join("registry-fallback-precondition.json"),
            &json!({"arrangement":"archive and remove only fixture-generated active lookup","pid":self.pid(),"birth":self.guard.token,"profile":self.profile,"launch_record_absent":true,"markers_unchanged":true}),
        );
    }
}
fn archive_record(active: &Path, archive: &Path, expected: &[u8]) {
    assert_eq!(
        fs::read(active).expect("read own active record"),
        expected,
        "active record changed; refuse fixture removal"
    );
    save(archive, expected);
    assert_eq!(fs::read(archive).expect("reread durable archive"), expected);
    assert_eq!(
        fs::read(active).expect("recheck active record"),
        expected,
        "active record changed; refuse fixture removal"
    );
    fs::remove_file(active).expect("remove only fixture's archived active launch record");
}
fn trusted_root(root: &Path) {
    let meta = fs::symlink_metadata(root).expect("configured profile root");
    assert!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "trusted root must be a real directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        assert_eq!(meta.permissions().mode() & 0o077, 0, "private profile root");
        // SAFETY: geteuid only returns the caller identity.
        assert_eq!(
            meta.uid(),
            unsafe { libc::geteuid() },
            "profile root owned by fixture user"
        );
    }
}
fn assert_listener(port: u16, pid: u32, home: &Path, label: &str) {
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new("lsof");
        c.args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fp"]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("netstat");
        c.args(["-ano", "-p", "tcp"]);
        c
    };
    let out = common::bounded_command_output(
        &mut command,
        Duration::from_secs(5),
        "153 listener observation",
    )
    .expect("bounded native listener lookup");
    save_json(
        &home.join(format!("{label}-listener.json")),
        &json!({"status":out.status.to_string(),"stdout":out.stdout,"stderr":out.stderr,"port":port,"expected_pid":pid}),
    );
    assert!(out.status.success(), "native listener observation failed");
    let text = String::from_utf8(out.stdout).expect("native listener UTF8");
    #[cfg(not(windows))]
    let found = text
        .lines()
        .any(|line| line.strip_prefix('p').and_then(|s| s.parse::<u32>().ok()) == Some(pid));
    #[cfg(windows)]
    let found = text.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() >= 5
            && fields[0] == "TCP"
            && fields[1].ends_with(&format!(":{port}"))
            && fields[3] == "LISTENING"
            && fields[4].parse::<u32>().ok() == Some(pid)
    });
    assert!(
        found,
        "native listener must name original Firefox {pid}, output={text}"
    );
}
fn recorded(home: &Path, ledger: &str, attempt: u8, port: u16, command: &mut Command) -> Output {
    common::recorded_launch_output(
        command.env("FF_RDP_HOME", home),
        &home.join(ledger),
        attempt,
        port,
    )
    .expect("record exact command outcome")
}
pub fn launch(home: &Path) -> Browser {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("fresh debug port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let out = recorded(
        home,
        "initial.attempts.jsonl",
        0,
        port,
        common::ff_rdp_launch_command().args([
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
        ]),
    );
    let guard = guard_launched_firefox(&out.stdout, home);
    assert!(out.status.success(), "initial launch failed: {out:?}");
    let guard = guard.expect("initial Firefox PID and native birth");
    let value: Value = serde_json::from_slice(&out.stdout).expect("whole initial launch envelope");
    let profile = PathBuf::from(
        value["results"]["profile"]
            .as_str()
            .expect("managed profile receipt"),
    );
    common::wait_for_debugger_port_within(
        &common::ff_rdp_bin(),
        port,
        common::launch_wait_timeout(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut attempt = 0u8;
    loop {
        let out = recorded(
            home,
            "tabs.attempts.jsonl",
            attempt,
            port,
            Command::new(common::ff_rdp_bin())
                .args(common::base_args(port))
                .arg("tabs"),
        );
        if out.status.success()
            && serde_json::from_slice::<Value>(&out.stdout)
                .ok()
                .and_then(|v| v["total"].as_u64())
                .unwrap_or(0)
                >= 1
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initial tabs readiness failed: {out:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
        attempt = attempt.checked_add(1).expect("bounded tabs attempt count");
    }
    let record = fs::read(home.join(format!(".ff-rdp/launch-record.{port}.json")))
        .expect("fixture-generated browser record");
    let rec: LaunchRecord = serde_json::from_slice(&record).expect("launch record type");
    assert_eq!(rec.pid, guard.pid);
    assert_eq!(rec.port, port);
    assert_eq!(rec.start_token, guard.token);
    assert_eq!(rec.profile_dir, profile);
    let root = home.join("ff-rdp/profiles");
    trusted_root(&root);
    assert_eq!(
        fs::canonicalize(profile.parent().expect("profile parent")).unwrap(),
        fs::canonicalize(&root).unwrap()
    );
    assert!(
        profile
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ff-rdp-profile-")
    );
    let markers = [OWNER_PID_MARKER, ".ff-rdp-owner-start", OWNER_TEST_MARKER]
        .iter()
        .map(|n| {
            (
                (*n).to_owned(),
                fs::read(profile.join(n)).expect("real ownership marker"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        String::from_utf8(markers[0].1.clone()).unwrap().trim(),
        guard.pid.to_string()
    );
    assert_eq!(
        String::from_utf8(markers[1].1.clone()).unwrap().trim(),
        guard.token
    );
    let browser = Browser {
        guard,
        port,
        home: home.to_path_buf(),
        profile,
        record,
        markers,
        proxies: Vec::new(),
    };
    browser.assert_survives("initial");
    browser
}
pub fn establish_registry(browser: &mut Browser, home: &Path, eval_stdout: &[u8], owned: bool) {
    let eval: Value = serde_json::from_slice(eval_stdout).expect("complete eval output");
    assert_eq!(
        eval["meta"]["route"], "daemon",
        "setup must actually route through daemon"
    );
    let bytes = fs::read(home.join(format!(".ff-rdp/daemon.{}.json", browser.port)))
        .expect("real daemon registry");
    let proxy: ProxyRecord = serde_json::from_slice(&bytes).expect("proxy record type");
    assert!(proxy.pid > 1);
    assert_ne!(proxy.pid, browser.pid());
    assert_eq!(proxy.firefox_port, browser.port);
    assert_eq!(proxy.firefox_host, "127.0.0.1");
    assert_ne!(proxy.proxy_port, 0);
    assert_eq!(
        process_start_token(proxy.pid).as_ref(),
        Some(&proxy.start_token),
        "native proxy identity"
    );
    // Separate authority: this fixture-created registry and native token own
    // only the proxy, never the Firefox to which it is connected.
    browser.proxies.push(ProcessGuard {
        pid: proxy.pid,
        token: proxy.start_token.clone(),
        home: home.to_path_buf(),
        role: "proxy",
    });
    save(&home.join("fixture-proxy-registry.archive.json"), &bytes);
    save_json(
        &home.join("daemon-precondition.json"),
        &json!({"browser_pid":browser.pid(),"proxy_pid":proxy.pid,"proxy_birth":proxy.start_token,"route":"daemon","owned_configured_root":owned}),
    );
    if owned {
        assert_eq!(home, browser.home);
        browser.archive_lookup();
    } else {
        assert_ne!(home, browser.home);
        assert!(
            !home
                .join(format!(".ff-rdp/launch-record.{}.json", browser.port))
                .exists()
        );
        browser.assert_survives("before-negative-replace");
    }
}

// allow-ungated-live: browser-free exact record archive/refusal control.
#[test]
fn unit_282_record_archive_refuses_changed_lookup_and_preserves_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join("launch-record.1.json");
    let archive = dir.path().join("archive.json");
    fs::write(&active, b"exact fixture record").unwrap();
    let refused = std::panic::catch_unwind(|| archive_record(&active, &archive, b"other record"));
    assert!(refused.is_err());
    assert_eq!(fs::read(&active).unwrap(), b"exact fixture record");
    assert!(!archive.exists());
    archive_record(&active, &archive, b"exact fixture record");
    assert!(!active.exists());
    assert_eq!(fs::read(&archive).unwrap(), b"exact fixture record");
}
#[cfg(unix)]
// allow-ungated-live: actual owned /bin/sleep cleanup control; no Firefox.
#[test]
fn unit_282_cleanup_refuses_changed_birth_and_collects_owned_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new("/bin/sleep").arg("10").spawn().unwrap();
    let token = process_start_token(child.id());
    let wrong = ProcessGuard {
        pid: child.id(),
        token: format!("{token:?}-wrong"),
        home: dir.path().to_path_buf(),
        role: "fixture-child",
    };
    drop(wrong);
    let survived = child.try_wait();
    // This is our actual Child; collect it before assertions on the refusal.
    let kill = child.kill();
    let status = child.wait().expect("actual owned child wait");
    assert!(token.is_some(), "native token required");
    kill.expect("owned child kill");
    assert!(
        survived.expect("actual child status").is_none(),
        "wrong birth guard must not signal child"
    );
    assert!(!status.success());
    let log = fs::read_to_string(dir.path().join(format!("cleanup-{}.jsonl", child.id()))).unwrap();
    assert!(log.contains("\"signal_authorized\":false"));
}

#[cfg(unix)]
// allow-ungated-live: actual owned /bin/sleep cleanup control; no Firefox.
#[test]
fn unit_282_cleanup_matches_birth_and_collects_owned_child() {
    use std::os::unix::{fs::PermissionsExt, process::ExitStatusExt};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ff-rdp/profiles");
    fs::create_dir_all(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let profile = root.join("ff-rdp-profile-fixture");
    fs::create_dir(&profile).unwrap();
    let mut child = Command::new("/bin/sleep").arg("10").spawn().unwrap();
    // A preparation panic cannot abandon this actual Child: the waiter owns it
    // until its bounded sleep ends, even if guard creation is rejected.
    let pid = child.id();
    let waiter = std::thread::spawn(move || child.wait());
    let outcome = std::panic::catch_unwind(|| {
        let token = process_start_token(pid).expect("native owned-child identity");
        fs::write(profile.join(OWNER_PID_MARKER), pid.to_string()).unwrap();
        fs::write(profile.join(".ff-rdp-owner-start"), token).unwrap();
        drop(ProcessGuard::new(pid, dir.path()));
    });
    let status = waiter
        .join()
        .expect("actual waiter joined")
        .expect("actual child wait");
    assert!(
        outcome.is_ok(),
        "matching-birth guard preparation/cleanup failed"
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "permitted guard must stop its own child, not merely wait for natural exit"
    );
    let log = fs::read_to_string(dir.path().join(format!("cleanup-{pid}.jsonl"))).unwrap();
    assert!(log.contains("\"signal_authorized\":true"));
    let rows = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        rows.iter()
            .any(|row| row["signal_result"]["Ok"] == json!(0)),
        "raw Unix signal rc must be retained: {log}"
    );
    assert!(
        rows.iter().any(|row| row["exit_observed"] == json!(true)),
        "bounded exit observation must be retained: {log}"
    );
}
