//! Firefox-free checks for isolated live-session parsing and configuration.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::common::{
    IsolatedLiveFirefox, ProfilePreference, bounded_command_output,
    daemon_autostart_trigger_timeout, isolated_launch_command_timeout, launch_receipt_from_output,
    parse_daemon_start_timeout, parse_product_launch_timeout, scoped_daemon_stop_timeout,
    validate_session_binary, write_requested_profile_prefs,
};
#[cfg(unix)]
use super::common::{
    bounded_command_output_with_poll, failed_launch_error, launch_request_context, pid_alive,
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
fn fake_launch_cli(
    fixture: &Path,
    launch_result: &str,
    cleanup_result: &str,
) -> std::path::PathBuf {
    fake_launch_cli_before_receipts(fixture, "", launch_result, cleanup_result)
}

#[cfg(unix)]
fn fake_launch_cli_before_receipts(
    fixture: &Path,
    before_receipts: &str,
    launch_result: &str,
    cleanup_result: &str,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let home_record = fixture.join("home");
    let invocation_record = fixture.join("cleanup-invocation");
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
test -d "$FF_RDP_HOME" || exit 91
printf '%s\n' "$FF_RDP_HOME|$*" > '{invocation_record}'
{cleanup_result}
"#,
        home_record = home_record.display(),
        port_record = fixture.join("port").display(),
        launch_timeout_record = fixture.join("launch-timeout").display(),
        launch_pid_record = fixture.join("launch-pid").display(),
        invocation_record = invocation_record.display(),
    );
    std::fs::write(&executable, script).expect("write cleanup fixture executable");
    let mut permissions = std::fs::metadata(&executable)
        .expect("fixture metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions).expect("make fixture executable");
    executable
}

#[cfg(unix)]
fn assert_recorded_scoped_cleanup(fixture: &Path, home_must_exist: bool) -> std::path::PathBuf {
    let home = std::path::PathBuf::from(
        std::fs::read_to_string(fixture.join("home"))
            .expect("recorded launch home")
            .trim(),
    );
    let port = std::fs::read_to_string(fixture.join("port")).expect("recorded launch port");
    let invocation =
        std::fs::read_to_string(fixture.join("cleanup-invocation")).expect("cleanup invocation");
    assert_eq!(
        invocation.trim(),
        format!(
            "{}|--host 127.0.0.1 --port {} daemon stop",
            home.display(),
            port.trim()
        )
    );
    assert_eq!(
        home.exists(),
        home_must_exist,
        "isolated home: {}",
        home.display()
    );
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
fn failed_launch_request_context_keeps_exact_cleanup_and_home_disposition() {
    use std::os::unix::ffi::OsStrExt;

    for cleanup_status in [0, 9] {
        let fixture = tempfile::tempdir().expect("owned fixture");
        let home = tempfile::tempdir_in(fixture.path()).expect("owned private home");
        let requested_home = home.path().to_owned();
        assert!(requested_home.to_str().is_some());
        let executable = fake_launch_cli(
            fixture.path(),
            "exit 99",
            &format!(
                "printf '%s\\n' \"$$\" > '{}'; exit {cleanup_status}",
                fixture.path().join("cleanup-pid").display()
            ),
        );
        let error = failed_launch_error("regression launch failure", &executable, home, 60123);
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
        assert_eq!(request["home_display_is_lossy"], false, "{error}");
        assert_eq!(
            request["home_display"].as_str(),
            Some(requested_home.to_string_lossy().as_ref())
        );
        assert_eq!(request["port"], 60123);
        let mut exact_invocation = requested_home.as_os_str().as_bytes().to_vec();
        exact_invocation.extend_from_slice(b"|--host 127.0.0.1 --port 60123 daemon stop\n");
        assert_eq!(
            std::fs::read(fixture.path().join("cleanup-invocation"))
                .expect("actual cleanup invocation bytes"),
            exact_invocation
        );
        let cleanup_pid = std::fs::read_to_string(fixture.path().join("cleanup-pid"))
            .expect("actual cleanup child PID")
            .trim()
            .parse::<u32>()
            .expect("numeric cleanup child PID");
        assert!(!pid_alive(cleanup_pid), "cleanup child was not reaped");
        assert!(error.contains("regression launch failure"), "{error}");
        if cleanup_status == 0 {
            assert!(error.contains("scoped cleanup succeeded"), "{error}");
            assert!(error.contains("status=Some(0)"), "{error}");
            assert!(!requested_home.exists(), "successful cleanup removes home");
        } else {
            assert!(error.contains("scoped cleanup failed"), "{error}");
            assert!(error.contains("status=Some(9)"), "{error}");
            assert!(requested_home.exists(), "failed cleanup preserves home");
            std::fs::remove_dir(&requested_home).expect("remove exact owned preserved home");
        }
        println!(
            "owned_cleanup status={cleanup_status} pid={cleanup_pid} exact_bytes=true home_removed=true result={error}"
        );
    }
}

#[cfg(unix)]
#[test]
fn failed_launch_always_cleans_exact_private_home_and_requested_port() {
    for (label, launch_result) in [
        ("nonzero", "echo launch-failed >&2; exit 7"),
        ("malformed", "printf '{malformed json'; exit 0"),
    ] {
        let fixture = tempfile::tempdir().expect("fixture tempdir");
        let executable = fake_launch_cli(fixture.path(), launch_result, "exit 0");
        let error = IsolatedLiveFirefox::launch(&executable)
            .err()
            .expect("fake launch must fail before session ownership");
        assert!(
            error.contains("scoped cleanup succeeded"),
            "{label}: {error}"
        );
        assert_recorded_scoped_cleanup(fixture.path(), false);
    }
}

#[cfg(unix)]
#[test]
fn failed_launch_preserves_private_home_when_scoped_cleanup_fails() {
    let fixture = tempfile::tempdir().expect("fixture tempdir");
    let executable = fake_launch_cli(fixture.path(), "printf '{malformed json'; exit 0", "exit 8");
    let error = IsolatedLiveFirefox::launch(&executable)
        .err()
        .expect("malformed receipt and failing cleanup must fail launch");
    assert!(error.contains("scoped cleanup failed"), "{error}");
    assert!(error.contains("preserving"), "{error}");
    let preserved_home = assert_recorded_scoped_cleanup(fixture.path(), true);
    std::fs::remove_dir_all(preserved_home).expect("remove deliberately preserved test home");
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
    let executable = fake_launch_cli(fixture.path(), "exit 42", "exit 0");
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
    assert_recorded_scoped_cleanup(fixture.path(), false);
}

#[cfg(unix)]
#[test]
fn isolated_launch_timeout_without_child_receipts_reaps_and_runs_scoped_cleanup() {
    let fixture = tempfile::tempdir().expect("fixture tempdir");
    // The same finite sleeper runs before parsing/writing receipts. exec keeps
    // the parent's direct child PID; no descendant or indefinite stop is added.
    let executable =
        fake_launch_cli_before_receipts(fixture.path(), "exec sleep 10", "exit 99", "exit 0");
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
    assert!(error.contains("scoped cleanup succeeded"), "{error}");
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
    let port = request["port"].as_u64().expect("requested port");
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("cleanup-invocation"))
            .expect("child-recorded cleanup request")
            .trim(),
        format!("{home}|--host 127.0.0.1 --port {port} daemon stop")
    );
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

    let daemon_rpc_connect = Duration::from_secs(10);
    let daemon_rpc_greeting = Duration::from_secs(10);
    let daemon_rpc_response_deadline = Duration::from_secs(10);
    let daemon_rpc_final_read = Duration::from_secs(10);
    let graceful_shutdown = Duration::from_secs(2);
    let proxy_escalation = Duration::from_millis(2_300);
    let firefox_escalation = Duration::from_millis(10_800);
    let stop_product_path = daemon_rpc_connect
        + daemon_rpc_greeting
        + daemon_rpc_response_deadline
        + daemon_rpc_final_read
        + graceful_shutdown
        + proxy_escalation
        + firefox_escalation;
    assert!(scoped_daemon_stop_timeout() > stop_product_path);

    assert_eq!(parse_daemon_start_timeout(None), Duration::from_secs(20));
    assert_eq!(
        parse_daemon_start_timeout(Some("45000")),
        Duration::from_secs(45)
    );
    assert_eq!(
        parse_daemon_start_timeout(Some("0")),
        Duration::from_secs(20)
    );
    let registry_wait = Duration::from_secs(45);
    let socket_phase = Duration::from_secs(5);
    let post_registration_phases = 11;
    let finite_trigger_path = registry_wait + socket_phase * post_registration_phases;
    let trigger_outer = daemon_autostart_trigger_timeout(registry_wait);
    assert!(trigger_outer > finite_trigger_path);
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
