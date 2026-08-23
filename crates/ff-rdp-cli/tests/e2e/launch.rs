//! Tests for the `launch` command.
//!
//! We cannot actually launch Firefox in CI, so these tests focus on:
//! - CLI argument parsing (--help, flag combinations)
//! - `build_command` argument construction (white-box unit tests via `pub(crate)`)
//! - Graceful failure when given a non-existent binary path
//!
//! A live-Firefox integration test is left for local developer use and is
//! gated behind the `live_firefox` env-var pattern to avoid CI noise.

use super::support;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

// ---------------------------------------------------------------------------
// CLI argument-parsing smoke tests (no Firefox needed)
// ---------------------------------------------------------------------------

#[test]
fn launch_help_exits_zero() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");

    assert!(
        output.status.success(),
        "expected zero exit for --help, stderr: {}",
        support::output_note(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("headless") || stdout.contains("Launch"),
        "help output should mention launch flags: {stdout}"
    );
}

/// `launch --port <busy>` must fail with a structured error that names
/// `doctor`, instead of silently spawning a Firefox that no-ops because the
/// port is taken.
#[test]
fn launch_detects_port_collision() {
    // Bind to a port and hold it open for the duration of the test.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();

    let output = std::process::Command::new(ff_rdp_bin())
        .args([
            "launch",
            "--debug-port",
            &port.to_string(),
            "--temp-profile",
        ])
        .output()
        .expect("failed to spawn ff-rdp");

    drop(listener);

    assert!(
        !output.status.success(),
        "expected non-zero exit when port is in use; stderr: {}",
        support::output_note(&output)
    );

    // The port-collision error is emitted as the JSON error envelope on stdout
    // (iter-98 Theme D removed the duplicate human `error:` stderr line).
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("already in use"),
        "output must mention 'already in use'; stderr={stderr:?} stdout={stdout:?}"
    );
    assert!(
        combined.contains("ff-rdp doctor") || combined.contains("`ff-rdp doctor`"),
        "output must reference `ff-rdp doctor`; stderr={stderr:?} stdout={stdout:?}"
    );
}

/// `ff-rdp --help` (top-level) must mention `ff-rdp doctor` somewhere in the
/// command reference so AI agents can discover it without grep-spelunking.
#[test]
fn help_mentions_doctor() {
    let output = std::process::Command::new(ff_rdp_bin())
        .arg("--help")
        .output()
        .expect("spawn");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("doctor"),
        "top-level --help must mention `doctor`; got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// iter-133 Theme A — `launch --window-size`
// ---------------------------------------------------------------------------

/// AC: `e2e_help_viewport_pointers` — `launch --help` documents
/// `--window-size` and the ~500px live-viewport floor, without needing
/// Firefox installed.
#[test]
fn launch_help_mentions_window_size_and_floor() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--help"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("window-size"),
        "launch --help must document --window-size: {stdout}"
    );
    assert!(
        stdout.contains("500px") || stdout.contains("floor"),
        "launch --help must document the live-viewport floor: {stdout}"
    );
}

/// A malformed `--window-size` value must be rejected with a user error
/// naming the expected `WxH` form — before any port-collision check or
/// Firefox spawn, so this test needs neither a free port nor Firefox
/// installed (see the parse-before-spawn ordering in `commands::launch::run`).
#[test]
fn launch_window_size_invalid_rejected() {
    let output = std::process::Command::new(ff_rdp_bin())
        .args(["launch", "--window-size", "0x0", "--temp-profile"])
        .output()
        .expect("failed to spawn ff-rdp");
    assert!(
        !output.status.success(),
        "expected non-zero exit for an invalid --window-size"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("WxH") || combined.contains("greater than 0"),
        "error must name the expected WxH form; stderr={stderr:?} stdout={stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// iter-191: a stale launch record is not ownership proof
// ---------------------------------------------------------------------------

/// Spawn a harmless, long-lived child to stand in for the process a recycled
/// PID belongs to. Returns the child handle so the test can both check that it
/// survived and kill it afterwards.
///
/// Deliberately a *real* process rather than a made-up PID: the record is only
/// consulted when its PID is alive (`daemon_record::read_in` drops dead ones),
/// so a fabricated number would skip the branch under test entirely.
fn spawn_sacrificial_child() -> std::process::Child {
    #[cfg(unix)]
    let mut cmd = {
        let mut c = std::process::Command::new("sleep");
        c.arg("30");
        c
    };
    // `ping` rather than `timeout`: `timeout /t` needs a real console and
    // exits immediately with "Input redirection is not supported" when stdin
    // is redirected, which is exactly how CI runs the test binary. That made
    // the child die before ff-rdp ever ran, and the survival assertion below
    // then reported a regression that had not happened (windows-latest, PR
    // #224).
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("ping");
        c.args(["-n", "31", "127.0.0.1"]);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn sacrificial child")
}

/// AC (iter-191): `launch --replace` must not signal a PID that a stale launch
/// record names but no longer identifies.
///
/// This is the 2026-08-23 live-sweep failure made deterministic, and the
/// dogfood path from `kb/iterations/iteration-191-*.md` in test form: plant a
/// `launch-record.<port>.json` naming a live process ff-rdp never launched
/// (its recorded start token disagrees with that PID's real one, exactly as a
/// recycled PID's would), occupy the port so the `--replace` stop path is
/// actually reached, and require the refusal — not a kill.
///
/// `FF_RDP_HOME` scopes the planted record to a temp dir so the test never
/// touches the developer's real `~/.ff-rdp`.
#[test]
fn launch_replace_refuses_stale_record_naming_a_foreign_pid() {
    let home = tempfile::tempdir().expect("tempdir");

    // Hold the port for the whole test: `launch` only takes the `--replace`
    // stop path when the port is already in use. The listener never accepts,
    // which is fine — `is_port_in_use` probes with a connect, and the kernel
    // backlog completes it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();

    let mut victim = spawn_sacrificial_child();
    let victim_pid = victim.id();

    // Precondition, asserted separately from the guarantee: the record is only
    // consulted while its PID is alive, so a child that died on its own would
    // make the branch under test unreachable *and* make the survival assertion
    // below report a regression that never happened.
    assert!(
        matches!(victim.try_wait(), Ok(None)),
        "precondition: the sacrificial child (pid {victim_pid}) must outlive the command \
         under test; it exited before ff-rdp ran"
    );

    let dir = home.path().join(".ff-rdp");
    std::fs::create_dir_all(&dir).expect("create .ff-rdp");
    std::fs::write(
        dir.join(format!("launch-record.{port}.json")),
        format!(
            r#"{{"pid": {victim_pid}, "port": {port}, "headless": true,
                 "launched_at": "2026-08-16T20:32:13.855708Z",
                 "profile_dir": "/tmp/ff-rdp-does-not-exist",
                 "start_token": "0.000000"}}"#
        ),
    )
    .expect("plant stale launch record");

    let output = std::process::Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home.path())
        .args([
            "launch",
            "--replace",
            "--headless",
            "--debug-port",
            &port.to_string(),
        ])
        .output()
        .expect("run ff-rdp launch --replace");

    // THE assertion: the process the stale record pointed at is untouched.
    assert!(
        matches!(victim.try_wait(), Ok(None)),
        "REGRESSION: launch --replace signalled pid {victim_pid}, which it only knew about \
         through a stale launch record"
    );

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "launch --replace must fail rather than pretend it freed the port; got: {combined}"
    );
    assert!(
        combined.contains("did not launch") || combined.contains("does not own"),
        "refusal must explain ff-rdp will not stop an unowned process; got: {combined}"
    );
    assert!(
        !combined.contains("still in use after stopping the prior instance"),
        "nothing was stopped, so the post-stop message must not be used; got: {combined}"
    );

    // The record is the ownership trail — a refused stop must not delete it
    // (iter-158 Theme B); iter-186's GC is what reclaims it.
    assert!(
        dir.join(format!("launch-record.{port}.json")).exists(),
        "a refused stop must leave the launch record in place"
    );

    let _ = victim.kill();
    let _ = victim.wait();
    drop(listener);
}
