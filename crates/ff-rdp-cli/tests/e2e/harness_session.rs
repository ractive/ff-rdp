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
use super::common::{bounded_command_output_with_poll, pid_alive};

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
    use std::os::unix::fs::PermissionsExt;

    let home_record = fixture.join("home");
    let invocation_record = fixture.join("cleanup-invocation");
    let executable = fixture.join("fake-ff-rdp");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "launch" ]; then
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
fn isolated_launch_timeout_still_runs_scoped_cleanup_and_passes_product_bound() {
    let fixture = tempfile::tempdir().expect("fixture tempdir");
    let executable = fake_launch_cli(fixture.path(), "exec sleep 10", "exit 0");
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
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("launch-timeout"))
            .expect("recorded launch timeout")
            .trim(),
        "7"
    );
    let launch_pid = std::fs::read_to_string(fixture.path().join("launch-pid"))
        .expect("recorded launch pid")
        .trim()
        .parse::<u32>()
        .expect("numeric launch pid");
    assert!(
        !pid_alive(launch_pid),
        "launch child {launch_pid} was not reaped"
    );
    assert_recorded_scoped_cleanup(fixture.path(), false);
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

    assert!(scoped_daemon_stop_timeout() > Duration::from_millis(35_100));

    assert_eq!(parse_daemon_start_timeout(None), Duration::from_secs(20));
    assert_eq!(
        parse_daemon_start_timeout(Some("45000")),
        Duration::from_secs(45)
    );
    assert_eq!(
        parse_daemon_start_timeout(Some("0")),
        Duration::from_secs(20)
    );
    let trigger_outer = daemon_autostart_trigger_timeout(Duration::from_secs(45));
    assert!(trigger_outer > Duration::from_secs(50));
}
