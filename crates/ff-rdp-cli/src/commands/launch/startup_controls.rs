//! Actual-child controls. A private subprocess isolates all launch state writes.
use super::*;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Owned {
    pid: u32,
    profile: PathBuf,
    channel: BufReader<UnixStream>,
    started: Instant,
    late_released: bool,
    saturation: Option<usize>,
}
thread_local! {
    static CASE: RefCell<&'static str> = const { RefCell::new("") };
    static OWNED: RefCell<Option<Owned>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Log(Arc<Mutex<Vec<u8>>>);
impl Write for Log {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn spawn_control(cmd: &mut Command) -> std::io::Result<Child> {
    let args: Vec<_> = cmd.get_args().collect();
    let arg = |key: &str| args.windows(2).find(|pair| pair[0] == key).unwrap()[1];
    let profile = PathBuf::from(arg("--profile"));
    let port = arg("--start-debugger-server");
    let (parent, endpoint) = UnixStream::pair()?;
    parent.set_read_timeout(Some(Duration::from_secs(1)))?;
    parent.set_write_timeout(Some(Duration::from_secs(1)))?;
    let input: OwnedFd = endpoint.try_clone()?.into();
    let output: OwnedFd = endpoint.into();
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "commands::launch::startup_controls::controlled_child",
            "--ignored",
            "--nocapture",
        ])
        .env("FF_RDP_284_CHILD_CASE", CASE.with(|case| *case.borrow()))
        .env("FF_RDP_284_CHILD_PORT", port)
        .stdin(Stdio::from(input))
        .stdout(Stdio::from(output))
        .stderr(Stdio::piped())
        .spawn()?;
    let mut channel = BufReader::new(parent);
    let ready = (|| {
        for _ in 0..8 {
            let mut line = String::new();
            if channel.read_line(&mut line)? == 0 {
                break;
            }
            if let Some(rest) = line.split("284-READY:").nth(1) {
                return Ok(rest.trim().parse::<usize>().ok());
            }
        }
        Err(std::io::Error::other(
            "controlled child did not publish readiness",
        ))
    })();
    match ready {
        Ok(saturation) => {
            OWNED.with(|owned| {
                *owned.borrow_mut() = Some(Owned {
                    pid: child.id(),
                    profile,
                    channel,
                    started: Instant::now(),
                    late_released: false,
                    saturation,
                });
            });
            Ok(child)
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

fn observed_status(child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
    let status = child.try_wait()?;
    OWNED.with(|owned| {
        let mut slot = owned.borrow_mut();
        let owned = slot.as_mut().unwrap();
        if status.is_none()
            && CASE.with(|case| *case.borrow() == "late")
            && !owned.late_released
            && owned.started.elapsed() >= Duration::from_millis(500)
        {
            // Publish release AFTER the actual status observation. The old
            // launcher returns None to its sole500ms check, then loses status
            // visibility in its real port loop. No fake exit result is injected.
            owned.channel.get_mut().write_all(b"x")?;
            owned.late_released = true;
        }
        Ok(status)
    })
}

#[test]
#[ignore = "self-spawned controlled child; parent selects one exact case"]
#[allow(unsafe_code)]
fn controlled_child() {
    let case = std::env::var("FF_RDP_284_CHILD_CASE").expect("owned child case");
    if case == "pipe" {
        // SAFETY: this controlled child's stderr is its own actual pipe write
        // endpoint. Only this thread writes it; restore flags before write_all.
        let flags = unsafe { libc::fcntl(2, libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(2, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let mut filled = 0;
        let mut saturated = false;
        let started = Instant::now();
        let data = [b'X'; 8192];
        while filled < 4 * 1024 * 1024 && started.elapsed() < Duration::from_secs(1) {
            match std::io::stderr().write(&data) {
                Ok(n) => {
                    assert!(n > 0);
                    filled += n;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    saturated = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("stderr saturation setup: {error}"),
            }
        }
        assert!(
            saturated,
            "must measure actual pipe saturation, not assume capacity"
        );
        assert_eq!(unsafe { libc::fcntl(2, libc::F_SETFL, flags) }, 0);
        println!("284-READY:{filled}");
        std::io::stdout().flush().unwrap();
        std::io::stderr()
            .write_all(&vec![b'Y'; 1024 * 1024])
            .unwrap();
        let port: u16 = std::env::var("FF_RDP_284_CHILD_PORT")
            .unwrap()
            .parse()
            .unwrap();
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        println!("284-BOUND:{}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        let mut release = [0];
        std::io::stdin().read_exact(&mut release).unwrap();
        drop(listener);
    } else {
        println!("284-READY:none");
        std::io::stdout().flush().unwrap();
        let mut release = [0];
        std::io::stdin().read_exact(&mut release).unwrap();
        if case == "late" {
            // stderr-ok: controlled child marker proves late startup stderr was captured.
            eprintln!("284-late-exit-marker");
            std::process::exit(37);
        }
    }
}

#[allow(unsafe_code)]
fn run_case(case: &'static str) {
    // Only the executor below calls the product path. These assertions precede
    // housekeeping, profile creation and launch-record writes.
    let private_home =
        PathBuf::from(std::env::var_os("FF_RDP_HOME").expect("isolated executor home"));
    assert_eq!(
        std::env::var_os("FF_RDP_284_ISOLATED_HOME"),
        Some(private_home.clone().into_os_string())
    );
    assert!(private_home.is_absolute() && private_home.is_dir());
    CASE.with(|slot| *slot.borrow_mut() = case);
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let hooks = LaunchHooks {
        is_port_in_use: |_| false,
        locate_firefox: || Ok(PathBuf::from("/owned-controlled-child")),
        spawn: spawn_control,
        try_wait: observed_status,
        ..LaunchHooks::none_running()
    };
    let cli = <Cli as clap::Parser>::try_parse_from(["ff-rdp", "launch"]).unwrap();
    let opts = LaunchOpts {
        headless: true,
        profile: None,
        temp_profile: true,
        debug_port: Some(port),
        auto_consent: false,
        replace: false,
        window_size: None,
        launch_timeout: Some(3),
    };
    let log = Log(Arc::default());
    let captured = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || captured.clone())
        .finish();
    let started = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tracing::subscriber::with_default(subscriber, || run_with_hooks(&cli, &opts, &hooks))
    }));
    let elapsed = started.elapsed();
    let mut owned = OWNED
        .with(|slot| slot.borrow_mut().take())
        .expect("actual child was acquired");
    let pid = i32::try_from(owned.pid).unwrap();
    let mut status = 0;
    // SAFETY: this is the exact direct child acquired by spawn_control. This
    // observation is nonblocking and cannot wait on or signal another child.
    let observed = unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) };
    let error = std::io::Error::last_os_error().raw_os_error();
    let transferred_alive = observed == 0;
    // The successful controlled child is still waiting on our release channel.
    // read() filters dead PIDs, so retain its actual record before release/wait.
    // Defer a read-error assertion until after the owned child is collected.
    let launch_record_before_release = crate::daemon_record::read(port);
    let mut cleanup_wait = observed;
    if observed == 0 {
        if result.as_ref().is_ok_and(Result::is_ok) {
            owned.channel.get_mut().write_all(b"x").unwrap();
        } else {
            // SAFETY: owned direct child is still alive and has not been reaped.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        // SAFETY: exact unreaped direct child; the test's outer watchdog bounds
        // a broken controlled child, which is never accepted as successful proof.
        cleanup_wait = unsafe { libc::waitpid(pid, &raw mut status, 0) };
    }
    let mut remaining = String::new();
    let tail_read = owned.channel.read_to_string(&mut remaining);
    let launch_record =
        launch_record_before_release.expect("read isolated launch record before release");
    let launch_record_matches_child = launch_record
        .as_ref()
        .is_none_or(|record| record.pid == owned.pid && record.profile_dir == owned.profile);
    // The exact controlled child has already been collected above. This state
    // root belongs only to this executor, including the success-path record.
    crate::daemon_record::remove(port).expect("remove isolated launch record after wait");
    let launch_record_removed = crate::daemon_record::read(port).unwrap().is_none();
    let profile_removed_by_product = !owned.profile.exists();
    if owned.profile.exists() {
        std::fs::remove_dir_all(&owned.profile).unwrap();
    }
    let trace = String::from_utf8_lossy(&log.0.lock().unwrap()).into_owned();
    let result_text = match &result {
        Ok(Ok(())) => "success".to_owned(),
        Ok(Err(error)) => error.to_error_json().to_string(),
        Err(_) => "unwind".to_owned(),
    };
    let receipt = json!({"case":case,"pid":owned.pid,"port":port,"elapsed_ms":elapsed.as_millis(),
        "initial_native_wait":observed,"wait_errno":error,"cleanup_native_wait":cleanup_wait,"native_status":status,
        "transferred_alive":transferred_alive,"saturation_bytes":owned.saturation,"late_released":owned.late_released,
        "profile_removed_by_product":profile_removed_by_product,"profile_removed_after_actual_wait":!owned.profile.exists(),
        "channel_eof":tail_read.is_ok(),"remaining_child_output":remaining,"result":result_text,"trace":trace,
        "private_home":private_home,"launch_record_observed":launch_record.is_some(),
        "launch_record_read_while_child_waited_for_release":transferred_alive,
        "launch_record_before_release":launch_record.as_ref().map(|record|
            json!({"pid":record.pid,"profile_dir":record.profile_dir,"port":record.port})),
        "launch_record_matches_child":launch_record_matches_child,"launch_record_removed_after_wait":launch_record_removed,
        "reader_thread":"none; synchronous product drain returned","worker_return_inferred":false});
    std::fs::write(
        private_home.join(format!("284-launch-{case}.json")),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    assert!(
        launch_record_matches_child && launch_record_removed,
        "only this controlled child's private launch record may be cleaned: {receipt}"
    );
    assert!(
        observed == -1 && error == Some(libc::ECHILD) || cleanup_wait == pid,
        "actual child wait required: {receipt}"
    );
    assert!(
        tail_read.is_ok(),
        "owned control channel must close after child return"
    );
    let result = result.expect("launcher must not unwind");
    if case == "late" {
        let error = result
            .expect_err("late child must fail startup")
            .to_error_json();
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("exited during startup with exit status: 37"),
            "late natural exit must retain its actual status: {error}"
        );
        assert!(trace.contains("natural_exit") && trace.contains("before_cleanup_signals"));
        assert!(profile_removed_by_product && owned.late_released);
    } else if case == "pipe" {
        assert!(
            result.is_ok(),
            "pipe output must drain so listener can open: {result:?}"
        );
        assert!(
            launch_record
                .as_ref()
                .is_some_and(|record| record.pid == owned.pid
                    && record.profile_dir == owned.profile
                    && record.port == port),
            "successful product launch must write its owned PID/profile/port record before release"
        );
        assert!(owned.saturation.is_some_and(|n| n > 0) && remaining.contains("284-BOUND:"));
        assert!(
            transferred_alive,
            "successful launch must transfer a live child"
        );
        assert!(trace.contains("discarded_bytes=") && trace.contains("<redacted len="));
        assert!(
            !trace.contains("YYYYYYYY"),
            "default tracing must redact actual stderr"
        );
    } else {
        let error = result.expect_err("live child never binds").to_error_json();
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("did not open debug port")
        );
        assert!(
            trace.contains("alive_when_deadline_checked")
                && trace.contains("alive before cleanup signals")
        );
        assert!(profile_removed_by_product && !transferred_alive);
        assert!(elapsed >= Duration::from_millis(3500) && elapsed < Duration::from_secs(6));
    }
}

/// All ordinary tests enter here without touching product state in this process.
/// Environment overrides belong to one Command, never the parallel test process.
fn run_isolated(case: &'static str) {
    let parent_override = std::env::var_os("FF_RDP_HOME");
    // Keep ownership explicit: no panic may implicitly remove state while a
    // child-wait receipt is still missing. The verified path below removes it.
    let private_path = tempfile::Builder::new()
        .prefix("ff-rdp-284-launch-control-")
        .tempdir()
        .unwrap()
        .keep();
    let stdout_path = private_path.join("executor.stdout");
    let stderr_path = private_path.join("executor.stderr");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "commands::launch::startup_controls::isolated_executor",
            "--ignored",
            "--nocapture",
        ])
        .env("FF_RDP_HOME", &private_path)
        .env("FF_RDP_284_ISOLATED_HOME", &private_path)
        .env("FF_RDP_284_ISOLATED_CASE", case)
        .env_remove("FF_RDP_TRACE_RAW")
        .env_remove("FF_RDP_284_CHILD_CASE")
        .env_remove("FF_RDP_284_CHILD_PORT")
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::fs::File::create(&stdout_path).unwrap()))
        .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
        .spawn()
        .expect("spawn isolated launch-control executor");
    let pid = child.id();
    let status = child.wait().expect("actual isolated executor wait");
    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    let control = std::fs::read(private_path.join(format!("284-launch-{case}.json")))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let child_collected = control.as_ref().is_some_and(|receipt| {
        receipt["initial_native_wait"].as_i64() == Some(-1)
            && receipt["wait_errno"].as_i64() == Some(i64::from(libc::ECHILD))
            || receipt["cleanup_native_wait"].as_u64() == receipt["pid"].as_u64()
                && receipt["pid"].as_u64().is_some()
    });
    // A missing/failed child-wait receipt is not authority to delete a profile
    // an uncollected child might still use. Retain that private tree on failure.
    let private_home_removed = if child_collected {
        std::fs::remove_dir_all(&private_path)
            .expect("remove executor's private state after actual waits");
        !private_path.exists()
    } else {
        false
    };
    let receipt = json!({"case":case,"executor_pid":pid,"executor_actual_wait":true,
        "executor_exit":status.code(),"parent_home_override_present":parent_override.is_some(),
        "private_home":private_path,"private_home_removed":private_home_removed,
        "controlled_child_collected":child_collected,"control":control,
        "stdout":stdout,"stderr":stderr});
    if let Some(root) = parent_override.as_ref() {
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(format!("284-launch-isolation-{case}.json")),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    } else {
        // Captured by normal libtest, or the bounded absent-parent-home proof.
        // stderr-ok: test-only isolation receipt records child waits and private-state cleanup.
        eprintln!("284 launch isolation receipt: {receipt}");
    }
    assert_eq!(
        std::env::var_os("FF_RDP_HOME"),
        parent_override,
        "parent environment must not change"
    );
    assert!(status.success(), "isolated control failed: {receipt}");
    assert!(
        child_collected && private_home_removed,
        "private state cleanup needs actual waits: {receipt}"
    );
    let control = receipt["control"].as_object().unwrap();
    assert_eq!(
        control.get("launch_record_removed_after_wait"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        control.get("profile_removed_after_actual_wait"),
        Some(&serde_json::Value::Bool(true))
    );
}

#[test]
#[ignore = "isolated control executor; parent supplies a private state home"]
fn isolated_executor() {
    let case = std::env::var("FF_RDP_284_ISOLATED_CASE").expect("parent selects one case");
    match case.as_str() {
        "late" => run_case("late"),
        "pipe" => run_case("pipe"),
        "live" => run_case("live"),
        _ => panic!("unknown controlled launch case"),
    }
}

#[test]
fn late_exit_retains_actual_status() {
    run_isolated("late");
}
#[test]
fn saturated_stderr_can_reach_listener() {
    run_isolated("pipe");
}
#[test]
fn live_timeout_retains_pre_cleanup_state() {
    run_isolated("live");
}
