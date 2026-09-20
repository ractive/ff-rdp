//! Firefox-free checks for isolated live-session parsing and configuration.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::common::{
    IsolatedLiveFirefox, ProfilePreference, bounded_command_output, launch_receipt_from_output,
    validate_session_binary, write_requested_profile_prefs,
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
    use std::os::unix::fs::PermissionsExt;

    let home_record = fixture.join("home");
    let invocation_record = fixture.join("cleanup-invocation");
    let executable = fixture.join("fake-ff-rdp");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "launch" ]; then
  shift
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--debug-port" ]; then port="$2"; shift 2; else shift; fi
  done
  printf '%s\n' "$FF_RDP_HOME" > '{home_record}'
  printf '%s\n' "$port" > '{port_record}'
  {launch_result}
fi
test -d "$FF_RDP_HOME" || exit 91
printf '%s\n' "$FF_RDP_HOME|$*" > '{invocation_record}'
{cleanup_result}
"#,
        home_record = home_record.display(),
        port_record = fixture.join("port").display(),
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
