//! iter-113 Theme A: bounded, self-describing Firefox-launch timeout.
//!
//! The pre-iter-113 live launchers waited on the Firefox remote-debugging port
//! and, when it never opened, silently skipped. Combined with an *ungated*
//! `#[test]`, an absent or wedged Firefox burned the whole CI job budget before
//! the job-level timeout fired (the iter-112 failure mode). This suite pins the
//! replacement: [`common::wait_for_debugger_port`] gives up within a bounded,
//! env-overridable budget and panics with a message naming the launcher binary
//! and the port it waited on.
//!
//! Unlike the rest of `tests/live/`, this suite needs **no Firefox** — it points
//! the wait at a port nothing is listening on — so it must run by default (that
//! is the entire value: proving the bound fires in ordinary CI). It is therefore
//! intentionally not `#[ignore]`-gated; see the `// allow-ungated-live:` note.

use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

use crate::common::{LAUNCH_TIMEOUT_ENV, ff_rdp_bin, launch_wait_timeout, wait_for_debugger_port};

/// Bind an ephemeral port, then drop the listener so the port is (almost
/// certainly) closed — nothing will accept a connection on it. Returns the port.
fn dead_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind :0");
    let port = l.local_addr().expect("local_addr").port();
    drop(l);
    port
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
    // Force a sub-second bound so the test itself is fast and deterministic.
    // (Serial within this binary; no other test reads this env var.)
    // SAFETY: set_var is unsafe as of Rust 2024; this test binary is
    // single-suite for this env key and does not spawn threads that read it.
    unsafe {
        std::env::set_var(LAUNCH_TIMEOUT_ENV, "1");
    }
    // The override is observed by launch_wait_timeout().
    assert_eq!(
        launch_wait_timeout(),
        Duration::from_secs(1),
        "the env override must be honored so the wait is bounded, not indefinite",
    );

    let port = dead_port();
    let bin = ff_rdp_bin();

    // Silence the default panic hook for the *expected* panic below so it does
    // not pollute test output; restore it immediately after.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let start = Instant::now();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        wait_for_debugger_port(&bin, port);
    }));
    let elapsed = start.elapsed();
    std::panic::set_hook(prev_hook);

    // Restore the env so we don't leak a 1 s bound into any later test.
    // SAFETY: same single-suite justification as above.
    unsafe {
        std::env::remove_var(LAUNCH_TIMEOUT_ENV);
    }

    // 1. It must have failed (panicked), not succeeded on a dead port.
    let payload = result.expect_err("wait on a dead port must panic, not return");

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
