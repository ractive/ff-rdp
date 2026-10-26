//! `ff-rdp stop` without a Firefox: the nothing-running path, `--all` on an
//! empty root, and the refusals — a port held by this test process (not
//! Firefox, no marker) must produce an error naming the PID, never a signal.

use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use super::common::{OWNER_PID_MARKER, OWNER_START_MARKER, process_start_token};
use super::support;

fn stop_command(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .env("FF_RDP_HOME", home)
        .env_remove("FF_RDP_PORT")
        .args(args)
        .output()
        .expect("spawn ff-rdp")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn json(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON ({e}); {}", support::output_note(out)))
}

#[test]
fn stop_with_nothing_listening_reports_stopped_false() {
    let home = tempfile::tempdir().expect("tempdir");
    let port = free_port().to_string();
    let out = stop_command(home.path(), &["stop", "--port", &port]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    let v = json(&out);
    assert_eq!(v["results"]["stopped"], false);
    assert!(v["results"]["pid"].is_null());
    assert_eq!(v["results"]["profile_removed"], false);
    assert!(
        v["results"]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("nothing is listening")),
        "{v}"
    );
}

#[test]
fn stop_dry_run_with_nothing_listening_signals_nothing() {
    let home = tempfile::tempdir().expect("tempdir");
    let port = free_port().to_string();
    let out = stop_command(home.path(), &["stop", "--dry-run", "--port", &port]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    let v = json(&out);
    assert_eq!(v["results"]["stopped"], false);
    assert_eq!(v["results"]["dry_run"], true);
}

#[test]
fn stop_all_on_an_empty_root_lists_nothing() {
    let home = tempfile::tempdir().expect("tempdir");
    for args in [&["stop", "--all"][..], &["stop", "--all", "--dry-run"][..]] {
        let out = stop_command(home.path(), args);
        assert!(out.status.success(), "{}", support::output_note(&out));
        let v = json(&out);
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
    }
}

/// Rule 1/7: a port held by a process ff-rdp did not launch is an error that
/// names the PID, and the holder keeps running.
#[test]
fn stop_refuses_a_listener_ff_rdp_did_not_launch() {
    let home = tempfile::tempdir().expect("tempdir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port().to_string();
    for args in [
        &["stop", "--port", &port][..],
        &["stop", "--dry-run", "--port", &port][..],
    ] {
        let out = stop_command(home.path(), args);
        assert!(!out.status.success(), "{}", support::output_note(&out));
        let note = support::output_note(&out);
        assert!(
            note.contains(&format!("PID {}", std::process::id())),
            "the refusal must name the holder's PID; {note}"
        );
        assert!(note.contains("did not launch"), "{note}");
    }
    // Still ours, still listening.
    assert!(TcpListener::bind(format!("127.0.0.1:{port}")).is_err());
    drop(listener);
}

/// Rule 4 end to end: a perfect marker pair naming this test process, in a
/// private managed profile under `FF_RDP_HOME`, still cannot get it stopped —
/// it is not a Firefox executable. `--all` does not even list it.
#[test]
fn stop_refuses_a_marked_process_that_is_not_firefox() {
    let Some(token) = process_start_token(std::process::id()) else {
        return; // no start token on this platform: nothing can be authorised anyway
    };
    let home = tempfile::tempdir().expect("tempdir");
    let root = home.path().join("ff-rdp").join("profiles");
    let profile = root.join("ff-rdp-profile-AAAAAAAAAAAAAAAA");
    std::fs::create_dir_all(&profile).expect("profile");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [home.path().join("ff-rdp"), root.clone()] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .expect("chmod 0700");
        }
    }
    std::fs::write(
        profile.join(OWNER_PID_MARKER),
        format!("{}\n", std::process::id()),
    )
    .expect("pid marker");
    std::fs::write(profile.join(OWNER_START_MARKER), format!("{token}\n")).expect("token");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port().to_string();
    let out = stop_command(home.path(), &["stop", "--port", &port]);
    assert!(!out.status.success(), "{}", support::output_note(&out));
    let note = support::output_note(&out);
    assert!(note.contains("is not Firefox"), "{note}");

    let out = stop_command(home.path(), &["stop", "--all"]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    assert_eq!(json(&out)["total"], 0);
    assert!(
        profile.is_dir(),
        "a refused stop must not touch the profile"
    );
    drop(listener);
}

#[test]
fn stop_refuses_a_remote_host() {
    let home = tempfile::tempdir().expect("tempdir");
    let out = stop_command(home.path(), &["--host", "192.0.2.1", "stop"]);
    assert!(!out.status.success());
    assert!(support::output_note(&out).contains("not a loopback"));
}

#[test]
fn stop_help_states_the_safety_rules() {
    let out = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .args(["stop", "--help"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for needle in ["--all", "--dry-run", "start time", "There is no --pid flag"] {
        assert!(help.contains(needle), "stop --help must mention {needle:?}");
    }
}
