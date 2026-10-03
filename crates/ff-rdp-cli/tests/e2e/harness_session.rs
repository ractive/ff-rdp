//! Firefox-free checks for isolated live-session parsing and configuration.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::common::{
    IsolatedLiveFirefox, ProfilePreference, TestHome, bounded_command_output,
    isolated_launch_command_timeout, launch_receipt_from_output, parse_product_launch_timeout,
    retained_failed_launch_home, validate_session_binary, write_requested_profile_prefs,
};
#[cfg(unix)]
use super::common::{
    OWNER_PID_MARKER, OWNER_START_MARKER, bounded_command_output_with_poll, failed_launch_error,
    kill_pid, launch_request_context, pid_alive, process_start_token,
};

#[test]
fn isolated_session_rejects_an_empty_ff_rdp_binary_path() {
    let error = validate_session_binary(Path::new(""))
        .expect_err("an exact ff-rdp binary path is required before a launch attempt");
    assert!(
        error.contains("non-empty exact ff-rdp binary path"),
        "{error}"
    );
}

#[test]
fn isolated_session_rejects_missing_pid_zero_port_and_mismatched_profile_before_version_lookup() {
    let profile = Path::new("/tmp/isolated-profile");
    let zero_pid = br#"{"results":{"pid":0,"port":60123,"profile":"/tmp/isolated-profile"},"meta":{"firefox":"/missing"}}"#;
    assert!(
        launch_receipt_from_output(zero_pid, 60123, profile)
            .unwrap_err()
            .contains("/results/pid")
    );
    let zero_port = br#"{"results":{"pid":44,"port":0,"profile":"/tmp/isolated-profile"},"meta":{"firefox":"/missing"}}"#;
    assert!(
        launch_receipt_from_output(zero_port, 60123, profile)
            .unwrap_err()
            .contains("/results/port")
    );
    let wrong_profile = br#"{"results":{"pid":44,"port":60123,"profile":"/tmp/other"},"meta":{"firefox":"/missing"}}"#;
    assert!(
        launch_receipt_from_output(wrong_profile, 60123, profile)
            .unwrap_err()
            .contains("requested port/profile")
    );
}

#[test]
fn isolated_session_serializes_typed_requested_preferences() {
    let dir = tempfile::tempdir().expect("tempdir");
    let prefs = vec![
        ("example.bool".to_owned(), ProfilePreference::Bool(false)),
        (
            "example.string".to_owned(),
            ProfilePreference::String("a\\\"b".to_owned()),
        ),
        ("example.number".to_owned(), ProfilePreference::Number(7)),
    ];
    write_requested_profile_prefs(dir.path(), &prefs).expect("write preferences");
    let user_js = std::fs::read_to_string(dir.path().join("user.js")).expect("read user.js");
    assert!(
        user_js.contains("user_pref(\"example.bool\", false);"),
        "{user_js}"
    );
    assert!(
        user_js.contains("user_pref(\"example.string\", \"a\\\\\\\"b\");"),
        "{user_js}"
    );
    assert!(
        user_js.contains("user_pref(\"example.number\", 7);"),
        "{user_js}"
    );
}

#[test]
fn isolated_session_resolves_a_bare_binary_filename_once() {
    let filename = format!("ff-rdp-harness-binary-{}", std::process::id());
    std::fs::write(&filename, b"test fixture").expect("write bare-name fixture");
    let expected = std::fs::canonicalize(&filename).expect("canonical fixture path");
    let resolved = validate_session_binary(Path::new(&filename)).expect("resolve bare filename");
    std::fs::remove_file(&filename).expect("remove bare-name fixture");
    assert_eq!(resolved, expected);
    assert!(resolved.is_absolute());
}

#[cfg(unix)]
fn fake_launch_cli(fixture: &Path, launch_result: &str) -> std::path::PathBuf {
    fake_launch_cli_before_receipts(fixture, "", launch_result)
}

#[cfg(unix)]
fn fake_launch_cli_before_receipts(
    fixture: &Path,
    before_receipts: &str,
    launch_result: &str,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let home_record = fixture.join("home");
    let executable = fixture.join("fake-ff-rdp");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "launch" ]; then
  {before_receipts}
  shift
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--debug-port" ]; then
      port="$2"
      shift 2
    elif [ "$1" = "--launch-timeout" ]; then
      launch_timeout="$2"
      shift 2
    else
      shift
    fi
  done
  printf '%s\n' "$FF_RDP_HOME" > '{home_record}'
  printf '%s\n' "$port" > '{port_record}'
  printf '%s\n' "$launch_timeout" > '{launch_timeout_record}'
  printf '%s\n' "$$" > '{launch_pid_record}'
  {launch_result}
fi
exit 92
"#,
        home_record = home_record.display(),
        port_record = fixture.join("port").display(),
        launch_timeout_record = fixture.join("launch-timeout").display(),
        launch_pid_record = fixture.join("launch-pid").display(),
    );
    std::fs::write(&executable, script).expect("write cleanup fixture executable");
    let mut permissions = std::fs::metadata(&executable)
        .expect("fixture metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions).expect("make fixture executable");
    executable
}

/// The private home the fake launch recorded, asserted removed: no browser
/// listens on the requested port, so the failed-launch path cleans it up.
#[cfg(unix)]
fn assert_recorded_home_removed(fixture: &Path) -> std::path::PathBuf {
    let home = std::path::PathBuf::from(
        std::fs::read_to_string(fixture.join("home"))
            .expect("recorded launch home")
            .trim(),
    );
    assert!(!home.exists(), "isolated home: {}", home.display());
    home
}

#[cfg(unix)]
#[test]
fn launch_request_context_handles_non_utf8_path_without_panicking() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    // Diagnostic serialization accepts arbitrary Unix paths independently of
    // whether this host filesystem permits creating their component bytes.
    let home = std::path::PathBuf::from(OsString::from_vec(b"/owned/non-utf8-\xff".to_vec()));
    let request = launch_request_context(&home, 60123);
    assert!(request["home"].is_null());
    assert_eq!(request["home_display_is_lossy"], true);
    assert_eq!(
        request["home_display"].as_str(),
        Some(home.to_string_lossy().as_ref())
    );
    assert_eq!(request["port"], 60123);
}

#[cfg(unix)]
#[test]
fn failed_launch_removes_home_only_when_the_requested_port_is_free() {
    for occupied in [false, true] {
        let fixture = tempfile::tempdir().expect("owned fixture");
        let home = tempfile::tempdir_in(fixture.path()).expect("owned private home");
        let requested_home = home.path().to_owned();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe port");
        let port = listener.local_addr().expect("probe addr").port();
        if !occupied {
            drop(listener);
        }
        let error = failed_launch_error("regression launch failure", home.into(), port);
        let request: serde_json::Value = serde_json::from_str(
            error
                .lines()
                .next()
                .expect("request context")
                .strip_prefix("isolated launch request: ")
                .expect("parent launch request"),
        )
        .expect("non-panicking structured diagnostic");
        assert_eq!(request["home"].as_str(), requested_home.to_str());
        assert_eq!(request["port"], port);
        assert!(error.contains("regression launch failure"), "{error}");
        if occupied {
            assert!(error.contains("still listening"), "{error}");
            assert!(requested_home.exists(), "a live port preserves the home");
            std::fs::remove_dir(&requested_home).expect("remove exact owned preserved home");
        } else {
            assert!(error.contains("is free"), "{error}");
            assert!(!requested_home.exists(), "a free port removes the home");
        }
    }
}

#[cfg(unix)]
#[test]
fn failed_launch_always_cleans_exact_private_home() {
    for (label, launch_result) in [
        ("nonzero", "echo launch-failed >&2; exit 7"),
        ("malformed", "printf '{malformed json'; exit 0"),
    ] {
        let fixture = tempfile::tempdir().expect("fixture tempdir");
        let executable = fake_launch_cli(fixture.path(), launch_result);
        let error = IsolatedLiveFirefox::launch(&executable)
            .err()
            .expect("fake launch must fail before session ownership");
        assert!(error.contains("is free"), "{label}: {error}");
        assert_recorded_home_removed(fixture.path());
    }
}

#[cfg(unix)]
#[test]
fn bounded_child_output_kills_and_reaps_on_timeout_without_pipe_deadlock() {
    let started = Instant::now();
    let error = bounded_command_output(
        Command::new("/bin/sh").args([
            "-c",
            "i=0; while [ $i -lt 200000 ]; do echo x; i=$((i+1)); done; exec sleep 10",
        ]),
        Duration::from_millis(100),
        "timeout fixture",
    )
    .expect_err("fixture must time out");
    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[cfg(unix)]
#[test]
fn bounded_child_output_kills_and_reaps_when_polling_fails() {
    let mut child_pid = 0;
    let error = bounded_command_output_with_poll(
        Command::new("/bin/sh").args(["-c", "exec sleep 10"]),
        Duration::from_secs(1),
        "poll-error fixture",
        |child| {
            child_pid = child.id();
            Err(std::io::Error::other("injected poll failure"))
        },
    )
    .expect_err("injected polling failure must be returned");
    assert!(error.contains("injected poll failure"), "{error}");
    assert!(error.contains("reap=Ok"), "{error}");
    assert_ne!(child_pid, 0);
    assert!(!pid_alive(child_pid), "child {child_pid} was not reaped");
}

#[cfg(unix)]
#[test]
fn isolated_launch_forwards_product_bound_to_child() {
    let fixture = tempfile::tempdir().expect("fixture tempdir");
    let executable = fake_launch_cli(fixture.path(), "exit 42");
    let product_timeout = Duration::from_secs(7);
    // This prompt child proves argument receipt, not a deliberate timeout.
    // Use the ordinary harness allowance so scheduling before its first write
    // does not compete with the separate one-second timeout regression.
    let error = IsolatedLiveFirefox::launch_with_preferences_and_timeouts(
        &executable,
        &[],
        product_timeout,
        isolated_launch_command_timeout(product_timeout, Duration::from_secs(5)),
    )
    .err()
    .expect("the child deliberately exits unsuccessfully after recording arguments");
    assert!(error.contains("isolated launch exited"), "{error}");
    assert!(!error.contains("isolated launch timed out"), "{error}");
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("launch-timeout"))
            .expect("child-recorded product timeout")
            .trim(),
        "7"
    );
    assert_recorded_home_removed(fixture.path());
}

#[cfg(unix)]
#[test]
fn isolated_launch_timeout_without_child_receipts_reaps_and_removes_home() {
    let fixture = tempfile::tempdir().expect("fixture tempdir");
    // The same finite sleeper runs before parsing/writing receipts. exec keeps
    // the parent's direct child PID; no descendant or indefinite stop is added.
    let executable = fake_launch_cli_before_receipts(fixture.path(), "exec sleep 10", "exit 99");
    let started = Instant::now();
    let error = IsolatedLiveFirefox::launch_with_preferences_and_timeouts(
        &executable,
        &[],
        Duration::from_secs(7),
        Duration::from_secs(1),
    )
    .err()
    .expect("the injected outer deadline must stop the fake launch");
    assert!(error.contains("isolated launch timed out"), "{error}");
    assert!(error.contains("is free"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
    // PID and scope come from the parent that spawned the command, never a
    // receipt whose creation the tested deadline is allowed to prevent.
    let launch_pid = error
        .split_once("child_pid=")
        .expect("parent-owned child PID")
        .1
        .split(';')
        .next()
        .expect("PID field")
        .parse::<u32>()
        .expect("numeric parent-owned PID");
    assert!(error.contains("kill=None; reap=Ok("), "{error}");
    assert!(
        !pid_alive(launch_pid),
        "launch child {launch_pid} was not reaped"
    );
    let request: serde_json::Value = serde_json::from_str(
        error
            .lines()
            .next()
            .expect("request context")
            .strip_prefix("isolated launch request: ")
            .expect("parent launch request"),
    )
    .expect("structured parent launch request");
    let home = request["home"].as_str().expect("requested home");
    assert!(!Path::new(home).exists(), "private home must be removed");
    for receipt in ["home", "port", "launch-timeout", "launch-pid"] {
        assert!(
            !fixture.path().join(receipt).exists(),
            "no launch receipt {receipt}"
        );
    }
}

#[test]
fn isolated_harness_timeouts_cover_product_budgets_without_global_env_mutation() {
    assert_eq!(parse_product_launch_timeout(None), Duration::from_secs(30));
    assert_eq!(
        parse_product_launch_timeout(Some("45")),
        Duration::from_secs(45)
    );
    assert_eq!(
        parse_product_launch_timeout(Some("bad")),
        Duration::from_secs(30)
    );
    let launch_outer =
        isolated_launch_command_timeout(Duration::from_secs(45), Duration::from_secs(5));
    assert!(launch_outer > Duration::from_secs(50));
}
#[cfg(unix)]
#[test]
fn unit_282_launch_ledger_keeps_failed_and_successful_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let ledger = temp.path().join("attempts.jsonl");
    for (attempt, script) in [
        (0, "printf 'failed stdout'; printf 'bad\\377' >&2; exit 9"),
        (1, "printf 'success'; exit 0"),
    ] {
        let output = crate::common::recorded_launch_output(
            std::process::Command::new("/bin/sh").args(["-c", script]),
            &ledger,
            attempt,
            7628,
        )
        .unwrap();
        assert_eq!(output.status.success(), attempt == 1);
    }
    let text = std::fs::read_to_string(ledger).unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["phase"], "start");
    assert_eq!(rows[1]["phase"], "output");
    assert_eq!(rows[1]["identity"], rows[0]["identity"]);
    assert_eq!(rows[1]["success"], false);
    assert_eq!(rows[1]["stdout"], serde_json::json!(b"failed stdout"));
    assert_eq!(
        rows[1]["stderr"],
        serde_json::json!([98, 97, 100, 255]),
        "stdout and stderr row: {:?}",
        rows[1]
    );
    assert_eq!(rows[3]["success"], true);
    assert_eq!(rows[3]["identity"]["attempt"], 1);
}

#[cfg(unix)]
#[test]
fn unit_282_unwritable_launch_ledger_prevents_spawn() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("should-not-exist");
    let error = crate::common::recorded_launch_output(
        std::process::Command::new("/usr/bin/touch").arg(&marker),
        temp.path(),
        0,
        7628,
    )
    .unwrap_err();
    assert!(error.contains("cannot open launch-attempt ledger"));
    assert!(!marker.exists());
}

#[test]
fn unit_282_failed_launch_rejects_live_dead_and_unmarked_profiles() {
    let occurrence = tempfile::tempdir().unwrap();
    let ledger = occurrence.path().join("launches.log");
    for marker in [
        Some(std::process::id().to_string()),
        Some("4294967295".to_owned()),
        None,
    ] {
        let home = crate::common::retained_failed_launch_home(Some(&ledger)).unwrap();
        assert_eq!(home.parent(), Some(occurrence.path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&home).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let root = home.join("ff-rdp/profiles");
        std::fs::create_dir_all(&root).unwrap();
        crate::common::assert_no_managed_profiles(&root);
        let profile = root.join("ff-rdp-profile-survivor");
        std::fs::create_dir(&profile).unwrap();
        if let Some(marker) = marker {
            std::fs::write(profile.join(crate::common::OWNER_PID_MARKER), marker).unwrap();
        }
        std::fs::write(profile.join("evidence"), b"retain through unwind").unwrap();
        let failure = std::panic::catch_unwind(|| crate::common::assert_no_managed_profiles(&root));
        assert!(
            failure.is_err(),
            "every unexpected managed profile must fail, including a live owner"
        );
        drop(home);
        assert_eq!(
            std::fs::read(profile.join("evidence")).unwrap(),
            b"retain through unwind"
        );
    }
}

#[cfg(unix)]
#[test]
fn unit_282_direct_failed_launch_retains_raw_outcome_and_actual_home() {
    let occurrence = tempfile::tempdir().unwrap();
    let ledger = occurrence.path().join("launches.attempts.jsonl");
    let home = crate::common::retained_failed_launch_home(Some(&ledger)).unwrap();
    let output = crate::common::recorded_launch_output(
        Command::new("/bin/sh")
            .args([
                "-c",
                "printf 'did not open debug port'; printf 'diagnostic' >&2; exit 1",
            ])
            .env("FF_RDP_HOME", &home),
        &ledger,
        0,
        7628,
    )
    .unwrap();
    assert!(!output.status.success());
    let text = std::fs::read_to_string(&ledger).unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[0]["home"], serde_json::json!(home.as_os_str()));
    assert_eq!(rows[1]["stdout"], serde_json::json!(output.stdout));
    assert_eq!(
        rows[1]["stderr"],
        serde_json::json!(output.stderr),
        "stdout/stderr bytes must both survive: {rows:?}"
    );
    assert_eq!(rows[1]["status"], output.status.to_string());
    assert_eq!(rows[1]["success"], false);
    let _ = std::panic::catch_unwind(|| panic!("subsequent test assertion"));
    assert!(home.is_dir());
    assert!(ledger.is_file());
}

#[test]
fn test_home_is_removed_on_drop_even_while_unwinding() {
    let home = TestHome::new("ff-rdp-test-home-unwind-").unwrap();
    let path = home.to_path_buf();
    let profile = path.join("ff-rdp/profiles/ff-rdp-profile-x");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("prefs.js"), b"x").unwrap();
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _home = home;
        panic!("simulated assertion failure");
    }));
    assert!(unwound.is_err());
    assert!(!path.exists(), "{} survived the unwind", path.display());
}

#[test]
fn failed_launch_home_without_a_ledger_is_removed_on_drop() {
    let home = retained_failed_launch_home(None).unwrap();
    let path = home.to_path_buf();
    assert!(path.is_dir());
    drop(home);
    assert!(!path.exists(), "{} leaked under $TMPDIR", path.display());
}

#[test]
fn test_home_close_reports_success_for_an_already_removed_tree() {
    let home = TestHome::new("ff-rdp-test-home-gone-").unwrap();
    std::fs::remove_dir_all(&*home).unwrap();
    assert_eq!(home.close(), Ok(()));
}

/// Spawn a stand-in "Firefox" and plant ff-rdp owner markers for it inside
/// `home`'s managed profile root; `start` overrides the recorded start token.
/// The child is reaped on a thread so, once killed, it does not linger as a
/// zombie that `kill(pid, 0)` still reports alive.
#[cfg(unix)]
fn plant_owned_process(
    home: &Path,
    start: Option<&str>,
) -> (
    u32,
    std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>,
) {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = child.id();
    let token = process_start_token(pid).expect("native start token");
    let profile = home.join("ff-rdp/profiles/ff-rdp-profile-owned");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join(OWNER_PID_MARKER), pid.to_string()).unwrap();
    std::fs::write(profile.join(OWNER_START_MARKER), start.unwrap_or(&token)).unwrap();
    (pid, std::thread::spawn(move || child.wait()))
}

#[cfg(unix)]
#[test]
fn test_home_drop_kills_its_own_browser_before_removing_the_tree() {
    use std::os::unix::process::ExitStatusExt;
    let home = TestHome::new("ff-rdp-test-home-kill-").unwrap();
    let path = home.to_path_buf();
    let (_pid, waiter) = plant_owned_process(&home, None);
    drop(home);
    let status = waiter.join().unwrap().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL), "{status:?}");
    assert!(!path.exists(), "{} survived", path.display());
}

#[cfg(unix)]
#[test]
fn test_home_drop_never_signals_a_pid_whose_start_token_differs() {
    let home = TestHome::new("ff-rdp-test-home-recycled-").unwrap();
    let path = home.to_path_buf();
    let (pid, waiter) = plant_owned_process(&home, Some("not-this-incarnation"));
    drop(home);
    assert!(pid_alive(pid), "a recycled PID must not be signalled");
    assert!(!path.exists(), "{} survived", path.display());
    kill_pid(pid);
    waiter.join().unwrap().unwrap();
}
