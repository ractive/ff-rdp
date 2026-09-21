//! Positive prevention-contract proof. Run the identical test with
//! FF_RDP_262_PROOF_BIN pointing to the frozen pre/post product respectively.
//! Both legs finish navigation/readiness/eval observations before asserting
//! that the watched descriptor received no legacy getTarget. This does not
//! claim to force Firefox's suppression schedule.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct ProofRun {
    binary: PathBuf,
    root: PathBuf,
    port: u16,
    pid: Option<u32>,
    firefox: Option<crate::common::FirefoxGuard>,
    sequence: usize,
}

struct OwnedPort {
    port: u16,
    reservation: Option<TcpListener>,
}

fn acquire_owned_port(
    root: &std::path::Path,
    supplied: Option<OsString>,
) -> std::io::Result<OwnedPort> {
    use std::io::Write;

    let (port, reservation, source) = if let Some(raw) = supplied {
        let port = raw.to_string_lossy().parse::<u16>().map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid runner-owned port: {error}"),
            )
        })?;
        (port, None, "proof-runner")
    } else {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        (port, Some(listener), "ordinary-live-sweep")
    };
    let mut receipt = std::fs::File::create(root.join("owned-port.json"))?;
    writeln!(
        receipt,
        "{}",
        json!({
            "time": chrono::Utc::now(),
            "port": port,
            "owner_pid": std::process::id(),
            "source": source,
            "reserved_while_receipted": reservation.is_some(),
        })
    )?;
    receipt.sync_all()?;
    Ok(OwnedPort { port, reservation })
}

// Files, intent and the owned port are durable before any spawn. Output goes
// straight to files, so a full pipe cannot prevent timeout or lose diagnostics.
struct OwnedCli(std::process::Child);
impl Drop for OwnedCli {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn bounded_command(
    mut command: Command,
    root: &std::path::Path,
    name: &str,
    timeout: Duration,
) -> std::io::Result<Output> {
    use std::io::Write;
    use std::process::Stdio;
    let stdout_path = root.join(format!("{name}.stdout"));
    let stderr_path = root.join(format!("{name}.stderr"));
    let argv: Vec<_> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let env: Vec<_> = command
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let mut receipt = std::fs::File::create(root.join(format!("{name}.receipt.jsonl")))?;
    writeln!(
        receipt,
        "{}",
        json!({"phase":"intent","time":chrono::Utc::now(),"binary":command.get_program().to_string_lossy(),"args":argv,"env":env})
    )?;
    receipt.sync_all()?;
    command
        .stdout(Stdio::from(std::fs::File::create(&stdout_path)?))
        .stderr(Stdio::from(std::fs::File::create(&stderr_path)?));
    let mut owned = OwnedCli(command.spawn()?);
    let child = &mut owned.0;
    // Keep the Child even if a receipt write fails: it must be reaped first.
    let pid_receipt = writeln!(
        receipt,
        "{}",
        json!({"phase":"spawn","time":chrono::Utc::now(),"pid":child.id()})
    )
    .and_then(|()| receipt.sync_all());
    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if pid_receipt.is_err() || Instant::now() >= deadline {
            child.kill()?;
            break (child.wait()?, true);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    writeln!(
        receipt,
        "{}",
        json!({"phase":"exit","time":chrono::Utc::now(),"pid":child.id(),"exit":status.code(),"timed_out":timed_out})
    )?;
    receipt.sync_all()?;
    pid_receipt?;
    if timed_out {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("owned CLI exceeded {timeout:?}"),
        ));
    }
    Ok(Output {
        status,
        stdout: std::fs::read(stdout_path)?,
        stderr: std::fs::read(stderr_path)?,
    })
}
impl ProofRun {
    fn command(&mut self, args: &[&str]) -> Output {
        self.sequence += 1;
        let autostart = args == ["eval", "1"];
        let timeout = if autostart {
            crate::common::daemon_autostart_trigger_timeout(
                crate::common::parse_daemon_start_timeout(
                    std::env::var("FF_RDP_DAEMON_START_TIMEOUT_MS")
                        .ok()
                        .as_deref(),
                ),
            )
        } else if args.first() == Some(&"launch") {
            crate::common::isolated_launch_command_timeout(
                Duration::from_secs(30),
                Duration::from_secs(5),
            )
        } else {
            Duration::from_secs(25)
        };
        let attributed = crate::common::ff_rdp_launch_command();
        let mut command = Command::new(&self.binary);
        command
            .envs(
                attributed
                    .get_envs()
                    .filter_map(|(key, value)| value.map(|value| (key, value))),
            )
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &self.port.to_string(),
                "--timeout",
                if autostart { "5000" } else { "15000" },
            ])
            .args(args)
            .env("FF_RDP_HOME", self.root.join("home"))
            .env(
                "RUST_LOG",
                "ff_rdp_core::transport=trace,ff_rdp_cli::daemon=debug",
            );
        let out = bounded_command(
            command,
            &self.root,
            &format!("{:03}", self.sequence),
            timeout,
        )
        .expect("bounded proof command");
        if args.first() == Some(&"launch") {
            self.firefox = crate::common::guard_launched_firefox(&out);
            self.pid = self.firefox.as_ref().map(crate::common::FirefoxGuard::pid);
        }
        out
    }

    fn cleanup(&mut self) -> std::io::Result<()> {
        let mut stop = Command::new(&self.binary);
        stop.args([
            "--port",
            &self.port.to_string(),
            "--timeout",
            "15000",
            "daemon",
            "stop",
        ])
        .env("FF_RDP_HOME", self.root.join("home"));
        let stopped = bounded_command(
            stop,
            &self.root,
            "cleanup-stop",
            crate::common::scoped_daemon_stop_timeout(),
        );
        // Only an already observed dead browser may disarm the guard.
        if self.pid.is_some_and(|pid| !crate::common::pid_alive(pid)) {
            if let Some(guard) = self.firefox.take() {
                guard.disarm();
            }
        } else {
            drop(self.firefox.take());
        }
        let mut prune = Command::new(&self.binary);
        prune
            .args(["profiles", "prune", "--older-than", "0s"])
            .env("FF_RDP_HOME", self.root.join("home"));
        let pruned = bounded_command(prune, &self.root, "cleanup-prune", Duration::from_secs(25));
        let alive = self.pid.is_some_and(crate::common::pid_alive);
        let listening = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], self.port)),
            Duration::from_millis(200),
        )
        .is_ok();
        let profiles = self.root.join("home/ff-rdp/profiles");
        let profiles_remaining = if profiles.exists() {
            std::fs::read_dir(profiles)?.count()
        } else {
            0
        };
        let daemon: Value = std::fs::read(self.root.join("daemon-owned.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        let daemon_alive = daemon["pid"]
            .as_u64()
            .is_some_and(|pid| crate::common::pid_alive(u32::try_from(pid).unwrap_or(0)));
        let proxy_listening = daemon["proxy_port"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .is_some_and(|port| {
                std::net::TcpStream::connect_timeout(
                    &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                    Duration::from_millis(200),
                )
                .is_ok()
            });
        let ok = !daemon_alive
            && !proxy_listening
            && stopped.as_ref().is_ok_and(|o| o.status.success())
            && pruned.as_ref().is_ok_and(|o| o.status.success())
            && !alive
            && !listening
            && profiles_remaining == 0;
        std::fs::write(self.root.join("cleanup.json"),json!({"time":chrono::Utc::now(),"ok":ok,
            "pid":self.pid,"pid_alive":alive,"firefox_port_listening":listening,"profiles_remaining":profiles_remaining,
            "daemon_pid":daemon["pid"],"daemon_alive":daemon_alive,"proxy_port":daemon["proxy_port"],"proxy_listening":proxy_listening,
            "stop_exit":stopped.as_ref().ok().and_then(|o|o.status.code()),"prune_exit":pruned.as_ref().ok().and_then(|o|o.status.code())}).to_string())?;
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::other("proof cleanup verification failed"))
        }
    }
}
impl Drop for ProofRun {
    fn drop(&mut self) {
        if !self.root.join("cleanup.json").exists() {
            let _ = self.cleanup();
        }
    }
}

#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_262_watched_target_prevention_contract() {
    assert!(std::env::var_os("FF_RDP_LIVE_TESTS").is_some());
    assert!(std::env::var_os("FF_RDP_LIVE_NETWORK_TESTS").is_some());
    let root = std::env::var_os("FF_RDP_262_PROOF_DIR").map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "ff-rdp-262-proof-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_millis()
            ))
        },
        PathBuf::from,
    );
    // Never overwrite evidence, and never silently launch a second attempt.
    std::fs::create_dir(&root).expect("new proof attempt directory");
    std::fs::create_dir(root.join("home")).unwrap();
    let mut owned_port = acquire_owned_port(&root, std::env::var_os("FF_RDP_262_PROOF_PORT"))
        .expect("durably receipt an owned port before spawn");
    let port = owned_port.port;
    let binary = std::env::var_os("FF_RDP_262_PROOF_BIN")
        .map_or_else(crate::common::ff_rdp_bin, PathBuf::from);
    let mut run = ProofRun {
        binary,
        root,
        port,
        pid: None,
        firefox: None,
        sequence: 0,
    };
    {
        use std::io::Write;
        let mut receipt = std::fs::File::create(run.root.join("owned-run.json")).unwrap();
        writeln!(receipt,"{}",json!({"time":chrono::Utc::now(),"port":port,"home":run.root.join("home"),"binary":run.binary,"owner_pid":std::process::id()})).unwrap();
        receipt.sync_all().unwrap();
    }
    // The ordinary sweep path holds the bound socket through both durable
    // ownership receipts. Release it only for the one product launch that must
    // bind this exact port. The private proof runner owns its supplied port.
    drop(owned_port.reservation.take());
    // command binds guard_launched_firefox before writing receipts or asserting.
    let launch = run.command(&["launch", "--headless", "--debug-port", &port.to_string()]);
    let launched: Value = serde_json::from_slice(&launch.stdout).expect("launch JSON");
    assert!(
        launch.status.success() && run.pid.is_some(),
        "one launch failed; no retry"
    );
    eprintln!(
        "PROOF_LAUNCH root={} port={port} pid={:?} profile={}",
        run.root.display(),
        run.pid,
        launched["results"]["profile_path"]
    );
    let profile = launched["results"]["profile_path"]
        .as_str()
        .expect("profile path");
    std::fs::copy(
        std::path::Path::new(profile).join("user.js"),
        run.root.join("profile-user.js"),
    )
    .expect("retain actual launch preferences");
    std::fs::write(run.root.join("launched.json"), launched.to_string()).unwrap();
    let version = run.command(&["--version"]);
    assert!(version.status.success(), "product version receipt required");
    std::fs::write(run.root.join("version-output"), version.stdout).unwrap();
    let start = run.command(&["eval", "1"]);
    assert!(start.status.success(), "supported eval autostart failed");
    let startup_status = run.command(&["daemon", "status"]);
    let startup: Value =
        serde_json::from_slice(&startup_status.stdout).expect("startup status JSON");
    assert!(
        startup_status.status.success(),
        "startup daemon status failed"
    );
    assert!(
        startup["results"]["live_target_count"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "successful startup eval must retain a live watched target: {startup}"
    );
    std::fs::copy(
        run.root.join(format!("home/.ff-rdp/daemon.{port}.json")),
        run.root.join("daemon-owned.json"),
    )
    .expect("durable daemon ownership");
    // Capture a complete setup prefix after eval has returned. Navigation
    // prevention is counted only after this boundary; setup is retained apart.
    let setup_log = std::fs::read_to_string(run.root.join("home/.ff-rdp/daemon.log"))
        .expect("setup wire trace");
    std::fs::write(run.root.join("setup-daemon.log"), &setup_log).unwrap();
    std::fs::write(
        run.root.join("navigation-boundary.json"),
        json!({"time":chrono::Utc::now(),"setup_bytes":setup_log.len(),"setup_command":"eval 1"})
            .to_string(),
    )
    .unwrap();
    let nav = run.command(&["navigate", "https://www.theguardian.com"]);
    let readiness_start = Instant::now();
    let mut ready = false;
    while readiness_start.elapsed() < Duration::from_secs(15) {
        let status = run.command(&["daemon", "status"]);
        let value: Value = serde_json::from_slice(&status.stdout).unwrap_or(Value::Null);
        if value["results"]["live_target_count"].as_u64().unwrap_or(0) > 0 {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let eval = run.command(&[
        "eval",
        "JSON.stringify({href:location.href,ready:document.readyState})",
    ]);
    // Observations are complete even on the pre-fix product before asserting
    // the prevention invariant, which is expected to fail there.
    let log = std::fs::read_to_string(run.root.join("home/.ff-rdp/daemon.log"))
        .expect("retained daemon wire trace");
    let packets: Vec<(bool, Value)> = log
        .lines()
        .filter_map(|line| {
            let (_, body) = line.split_once(" body=")?;
            Some((
                line.contains("direction=\"send\""),
                serde_json::from_str(body).ok()?,
            ))
        })
        .collect();
    let descriptor = packets
        .iter()
        .find(|(send, p)| *send && p["type"] == "getWatcher")
        .and_then(|(_, p)| p["to"].as_str())
        .expect("startup descriptor binding");
    let watcher = packets
        .iter()
        .find(|(send, p)| !send && p["from"] == descriptor && p["actor"].is_string())
        .and_then(|(_, p)| p["actor"].as_str())
        .expect("startup watcher binding");
    assert!(
        log.starts_with(&setup_log),
        "daemon log must retain the exact setup prefix"
    );
    let navigation_log = &log[setup_log.len()..];
    std::fs::write(run.root.join("navigation-daemon.log"), navigation_log).unwrap();
    let navigation_packets: Vec<(bool, Value)> = navigation_log
        .lines()
        .filter_map(|line| {
            let (_, body) = line.split_once(" body=")?;
            Some((
                line.contains("direction=\"send\""),
                serde_json::from_str(body).ok()?,
            ))
        })
        .collect();
    let setup_legacy = packets
        .iter()
        .take(packets.len() - navigation_packets.len())
        .filter(|(send, p)| *send && p["type"] == "getTarget" && p["to"] == descriptor)
        .count();
    let legacy = navigation_packets
        .iter()
        .filter(|(send, p)| *send && p["type"] == "getTarget" && p["to"] == descriptor)
        .count();
    let mut live: Option<&Value> = None;
    let mut selected = None;
    let mut replacements = 0;
    for (send, p) in &packets {
        if !send && p["from"] == watcher {
            let t = &p["target"];
            if p["type"] == "target-available-form"
                && t["isTopLevelTarget"] == true
                && t["isPopup"] == false
            {
                live = Some(t);
                replacements += 1;
            } else if p["type"] == "target-destroyed-form"
                && live.is_some_and(|l| l["actor"] == t["actor"])
            {
                live = None;
            }
        }
        if *send && p["type"] == "evaluateJSAsync" {
            selected = Some((p, live));
        }
    }
    let (last_eval, selected_form) = selected.expect("product evaluation reached Firefox");
    std::fs::write(run.root.join("identity.json"),json!({"descriptor":descriptor,"watcher":watcher,
        "legacy_get_target":legacy,"setup_legacy_get_target":setup_legacy,"selected_eval_actor":last_eval["to"],"live_form":selected_form,
        "top_level_availabilities":replacements,"readiness":ready,"navigation_exit":nav.status.code(),
        "eval_exit":eval.status.code()}).to_string()).unwrap();
    // Verification failures remain fatal on the normal path; Drop is fallback.
    run.cleanup().expect("owned cleanup verification");
    assert_eq!(
        legacy, 0,
        "watched descriptor must never issue legacy getTarget; all observations retained"
    );
    assert!(
        nav.status.success() && ready && eval.status.success(),
        "navigation/readiness/evaluation must succeed independently"
    );
    assert!(
        replacements >= 2,
        "initial and replacement watcher forms required"
    );
    let selected_form = selected_form.expect("live watcher target at final evaluation");
    assert_eq!(last_eval["to"], selected_form["consoleActor"]);
    assert!(selected_form["browsingContextID"].is_u64() && selected_form["innerWindowId"].is_u64());
    assert!(String::from_utf8_lossy(&eval.stdout).contains("theguardian.com"));
}

// allow-ungated-live: Firefox-free proof that an ordinary full-sweep invocation
// owns and durably receipts its port without the private proof-runner variable.
#[test]
fn ordinary_sweep_inputs_reserve_and_receipt_port() {
    let root = tempfile::tempdir().expect("temporary receipt directory");
    let owned = acquire_owned_port(root.path(), None).expect("reserve ordinary sweep port");
    let receipt: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("owned-port.json")).expect("owned-port receipt"),
    )
    .expect("valid owned-port JSON");
    assert_eq!(receipt["port"], owned.port);
    assert_eq!(receipt["owner_pid"], std::process::id());
    assert_eq!(receipt["source"], "ordinary-live-sweep");
    assert_eq!(receipt["reserved_while_receipted"], true);
    assert!(
        TcpListener::bind(("127.0.0.1", owned.port)).is_err(),
        "receipt must be durable while the port is still reserved"
    );
}
