//! iter-113 Theme A: bounded, self-describing Firefox-launch timeout.
//!
//! The pre-iter-113 live launchers waited on the Firefox remote-debugging port
//! and, when it never opened, silently skipped. Combined with an *ungated*
//! `#[test]`, an absent or wedged Firefox burned the whole CI job budget before
//! the job-level timeout fired (the iter-112 failure mode). This suite pins the
//! replacement: [`common::wait_for_debugger_port_within`] gives up within a
//! bounded budget and panics with a message naming the launcher binary and the
//! port it waited on.
//!
//! Unlike the rest of `tests/live/`, this suite needs **no Firefox** — it points
//! the wait at a port nothing is listening on — so it must run by default (that
//! is the entire value: proving the bound fires in ordinary CI). It is therefore
//! intentionally not `#[ignore]`-gated; see the `// allow-ungated-live:` note.
//!
//! These tests do not mutate the process-wide `LAUNCH_TIMEOUT_ENV` var: `cargo
//! test-live` (unlike CI's `--test-threads=1` live job) runs test binaries with
//! multiple threads by default, and these tests are intentionally ungated, so
//! they can run concurrently with `#[ignore]`-gated live suites that spawn real
//! Firefox and read [`common::launch_wait_timeout`] on another thread.
//! `std::env::set_var` on that key would risk truncating an in-flight real
//! launch's wait. [`common::wait_for_debugger_port_within`] takes the bound as
//! a parameter instead, keeping these tests hermetic.

use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

use crate::common::{ff_rdp_bin, wait_for_debugger_port_within};

/// Keep the port bound but never listen. Releasing an ephemeral listener lets
/// another socket claim that number while the timeout probe is still running.
fn dead_port() -> socket2::Socket {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None)
        .expect("create reservation");
    socket
        .bind(
            &"127.0.0.1:0"
                .parse::<std::net::SocketAddr>()
                .unwrap()
                .into(),
        )
        .expect("bind reservation");
    socket
}

/// AC: `launch_times_out_fast` — pointing the live launcher's port-wait at an
/// unreachable Firefox fails *within the bounded wait* (not indefinitely) with a
/// panic message naming the launcher binary and the port.
///
// allow-ungated-live: no Firefox needed — this points wait_for_debugger_port at
// a dead port to prove the bound fires; it must run by default in plain CI to be
// worth anything, so #[ignore] would defeat its purpose (iter-113 Theme A).
#[test]
fn launch_times_out_fast() {
    // Sub-second bound passed explicitly (not via env — see module docs) so the
    // test itself is fast, deterministic, and safe to run alongside concurrent
    // live suites that read the real env-backed timeout on other threads.
    let bound = Duration::from_secs(1);

    let reservation = dead_port();
    let address = reservation.local_addr().unwrap().as_socket().unwrap();
    let port = address.port();
    assert!(
        std::net::TcpListener::bind(address).is_err(),
        "reservation must prevent reuse"
    );
    assert!(
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "reservation must not accept connections"
    );
    let bin = ff_rdp_bin();

    // Catch only this thread's panic; do not replace the process-wide hook.
    let start = Instant::now();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        wait_for_debugger_port_within(&bin, port, bound);
    }));
    let elapsed = start.elapsed();

    // 1. It must have failed (panicked), not succeeded on a dead port.
    let payload = result.expect_err("wait on a dead port must panic, not return");
    assert!(
        elapsed >= bound,
        "timeout helper returned before its bound: {elapsed:?}"
    );

    // 2. It must have failed *within the bound* — not hung indefinitely. Allow
    //    generous slack (5 s) for a loaded CI runner while still proving the wait
    //    is bounded rather than the pre-iter-113 open-ended behavior.
    assert!(
        elapsed < Duration::from_secs(5),
        "bounded wait must give up quickly (1 s budget); took {elapsed:?}",
    );

    // 3. The panic message must name the launcher binary and the port so CI logs
    //    point straight at the cause. Deref the boxed payload to the inner
    //    `dyn Any` (a `&Box<dyn Any>` would downcast as the box, not its content).
    let msg = panic_message(payload.as_ref());
    assert!(
        msg.contains(&port.to_string()),
        "panic message must name the port {port}; got: {msg}",
    );
    assert!(
        msg.contains(&bin.display().to_string()),
        "panic message must name the launcher binary {}; got: {msg}",
        bin.display(),
    );
}

// allow-ungated-live: exercises the launch helper on an owned TCP listener, no Firefox.
#[test]
fn launch_accepts_an_open_port() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let started = Instant::now();
    wait_for_debugger_port_within(
        &ff_rdp_bin(),
        listener.local_addr().unwrap().port(),
        Duration::from_secs(1),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// AC: the [`common::LAUNCH_TIMEOUT_ENV`] override parsing rules are honored —
/// numeric ⇒ that many seconds, missing/non-numeric ⇒ the 30 s default.
///
/// Exercises [`common::parse_launch_timeout`] (the pure half of
/// [`common::launch_wait_timeout`]) directly with in-memory `Option<&str>`
/// inputs rather than mutating the process-wide env var, so this cannot race
/// concurrently-running live suites that read the real var on another thread.
///
// allow-ungated-live: no Firefox needed — exercises pure parsing logic; must
// run by default to guard the parsing rules (iter-113 Theme A).
#[test]
fn launch_timeout_env_override_is_honored() {
    assert_eq!(
        crate::common::parse_launch_timeout(Some("1")),
        Duration::from_secs(1),
        "a numeric override must be honored so the wait is bounded, not indefinite",
    );
    assert_eq!(
        crate::common::parse_launch_timeout(None),
        Duration::from_secs(30),
        "an unset override must fall back to the 30s default",
    );
    assert_eq!(
        crate::common::parse_launch_timeout(Some("not-a-number")),
        Duration::from_secs(30),
        "a malformed override must fall back to the 30s default, not panic",
    );
}

/// Extract the human-readable payload of a caught panic (`&str` or `String`).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::new()
    }
}
