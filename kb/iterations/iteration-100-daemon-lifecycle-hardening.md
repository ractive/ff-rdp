---
title: "Iteration 100: daemon lifecycle hardening — thread supervision, honest idle timeout, signal cleanup, spawn/kill races, auto-start observability"
type: iteration
date: 2026-07-09
status: planned
branch: iter-100/daemon-lifecycle-hardening
depends_on: []
firefox_refs: []
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
  - kb/iterations/iteration-99-ci-preexisting-reds.md
first_call_sites:
  - primitive: >-
      supervised worker-thread wrapper (catch_unwind → state.shutdown) for
      firefox-reader / event-dispatcher / grip-release-drainer
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: >-
      real SIGTERM/SIGINT (Unix) and console-ctrl (Windows) handler that runs
      registry cleanup before exit
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: >-
      exclusive spawn lock held across the whole check→spawn→register sequence
    site: crates/ff-rdp-cli/src/daemon/client.rs
  - primitive: >-
      daemon_autostart_failed warning in the command envelope when auto-start
      does not register and the CLI falls back to a direct connection
    site: crates/ff-rdp-cli/src/daemon/client.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp tabs                       # auto-starts a daemon
  kill -TERM $(ff-rdp daemon status --jq .pid)
  # expected: registry file removed, next command spawns a clean daemon
  ff-rdp tabs && ff-rdp daemon status
tags: [iteration, daemon, lifecycle, robustness, review-2026-07]
---

# Iteration 100: daemon lifecycle hardening

The 2026-07 deep review ([[deep-review-2026-07-fable5]]) found that the
daemon's failure story has holes previous reviews missed. The worst is a
**zombie mode**: the three worker threads (`firefox-reader`,
`event-dispatcher`, `grip-release-drainer`) are spawned with dropped
`JoinHandle`s, no panic supervision, and none sets `state.shutdown` on exit
(`daemon/server.rs:369-399`) — a panic in any of them leaves a daemon whose
PID, socket, and registry all look healthy while every client hangs. Worse,
the only escape hatch — the idle timeout — never fires for a zombie, because
`last_activity` is bumped on every accept including failed ones
(`server.rs:1017,1026`). Around that core sit four smaller lifecycle races:
SIGTERM/SIGINT skip registry cleanup (`setup_signal_handler` is a no-op whose
doc comment claims the opposite, `server.rs:468-486`), the auto-spawn
check→spawn→register sequence is not serialized (`client.rs:263-320`), the
`handle_client` cleanup block is skippable via an early `?` return
(`server.rs:1174` vs `:1196-1207`), and client identity is keyed on a
recyclable raw fd (`server.rs:1102`). One more kill-safety hole:
`stop_prior_instance` resolves a PID once and kills it later with no
re-verification (`client.rs:999-1009`).

**Absorbed from iter-99 Theme B (auto-start registration).** The
`live_eval_object_leak_soak` CI red was only *deflaked* (commit `23c5994`
widened the status poll to 10 s) — the underlying symptom is that an
auto-started daemon sometimes never registers within the poll window while
the CLI **silently** falls back to a direct connection. That silent
degradation is the same failure surface as the spawn race in Theme D and the
zombie in Theme A: whatever the root cause (TOCTOU double-spawn, a spawn that
dies before the registry write, or a slow registry write), the CLI must not
present "used the daemon" and "quietly went direct" as the same observable
outcome. This iteration owns both the root-cause fix and the
`daemon_autostart_failed` warning the iter-99 plan specified.

## Themes

- **A — Thread supervision.** A worker-thread panic must flip the daemon into
  shutdown, not leave a zombie.
- **B — Honest idle timeout.** Only genuinely successful client interactions
  extend the daemon's life.
- **C — Signal-driven cleanup.** SIGTERM/SIGINT (and Windows console ctrl)
  remove the registry (which contains the auth token) before exit; the lying
  doc comment goes away either way.
- **D — Spawn/kill/identity races.** One daemon per registry, cleanup on every
  client exit path, no fd-reuse confusion, no killing recycled PIDs.
- **E — Auto-start observability (from iter-99 B).** Auto-start failure is
  surfaced, not silent; the soak test goes green because the daemon actually
  registers, not because the poll got longer.

## Tasks

### A. Thread supervision [0/3]
- [ ] Wrap the three worker loops (`server.rs:369-399`) in a supervised
      spawn helper: `catch_unwind` around the loop body; on panic or
      unexpected return, set `state.shutdown` and log once.
- [ ] Make `accept_loop` refuse new clients (clean "daemon shutting down"
      error frame) once `state.shutdown` is set.
- [ ] Land `unit_reader_panic_sets_shutdown` (inject a panicking frame
      handler via a test seam; assert shutdown flag + accept refusal).

### B. Honest idle timeout [0/2]
- [ ] Bump `last_activity` only after a successfully authenticated request
      is handled — not on accept, not on client-thread error exit
      (`server.rs:1017,1026`).
- [ ] Land `unit_idle_timeout_ignores_failed_clients` (repeated
      unauthenticated connects do not extend the deadline; daemon exits on
      schedule) and `unit_idle_timeout_fires` (the branch at
      `server.rs:1007-1013` finally gets a non-live test).

### C. Signal-driven registry cleanup [0/2]
- [ ] Implement `setup_signal_handler` for real: `sigaction` for
      SIGTERM/SIGINT on Unix (via the already-present `libc`),
      `SetConsoleCtrlHandler` on Windows (via the already-present
      `windows-sys`); handler sets `state.shutdown` so `run_daemon`'s normal
      cleanup (`remove_registry`, `server.rs:403-405`) runs. Fix the doc
      comment; also fix the stale "crashing is the right action" comment above
      `lock_or_recover!` (`server.rs:509-511`).
- [ ] Land `e2e_sigterm_removes_registry` (Unix: spawn daemon, SIGTERM,
      assert registry file gone and exit code clean; Windows path covered by
      a unit test on the shared shutdown-flag plumbing).

### D. Spawn/kill/identity races [0/4]
- [ ] Hold one exclusive file lock across the whole
      check-running→spawn→register sequence in `resolve_connection_target`
      (`client.rs:263-320`), so two racing CLI invocations produce exactly
      one registered daemon and zero orphans.
- [ ] Run the `handle_client` cleanup block (stream unsubscribe +
      rpc_writer unregister, `server.rs:1196-1207`) on every exit path —
      scope guard instead of fall-through after `?` (`server.rs:1174`).
- [ ] Replace raw-fd client identity (`server.rs:1102`) with a monotonic
      client id issued at accept time, so fd reuse cannot unregister the
      wrong subscriber or clear a live RPC writer.
- [ ] Re-verify port ownership (or compare process start time) immediately
      before the kill in `stop_prior_instance` (`client.rs:999-1009`).

### E. Auto-start observability (absorbed from iter-99 B) [0/3]
- [ ] Root-cause why the auto-started daemon sometimes fails to register in
      time (`resolve_connection_target`, `client.rs:263-320`): distinguish
      "spawn died before the registry write" from "registry write raced /
      was slow" from "TOCTOU double-spawn orphaned the winner". The Theme D
      spawn lock removes the double-spawn cause; instrument the remaining
      path so the failure mode is identifiable, and record the finding in
      this plan.
- [ ] When auto-start does not yield a usable daemon and the CLI falls back
      to a direct connection, add a `daemon_autostart_failed` warning to the
      command envelope (never a hard error — direct mode still works) with a
      short reason, so tests and users can tell daemon from direct.
- [ ] Confirm `live_eval_object_leak_soak` passes because the daemon
      registers within the *original* window — not because of the widened
      poll; tighten the poll back toward its pre-`23c5994` budget once the
      root cause is fixed, or document why the wider budget is legitimately
      required.

## Acceptance Criteria [0/10]

- [ ] unit_reader_panic_sets_shutdown: an injected worker-loop panic sets
      `state.shutdown` and a subsequent connect receives a shutdown error
      instead of hanging.
- [ ] unit_idle_timeout_ignores_failed_clients: N unauthenticated connects
      after t0 do not move the idle deadline; daemon exits at t0+timeout.
- [ ] unit_idle_timeout_fires: with no clients, the daemon self-terminates
      at the configured idle timeout (mock clock or short timeout).
- [ ] e2e_sigterm_removes_registry: after SIGTERM the registry file is gone
      before process exit (Unix).
- [ ] unit_spawn_lock_serializes_check_spawn_register: two concurrent
      `resolve_connection_target` calls against an empty registry yield one
      daemon registration and no orphaned second process entry.
- [ ] unit_handle_client_cleanup_on_write_error: a client whose socket write
      fails is fully unregistered (no stale rpc_writer, no stale subscriber).
- [ ] unit_client_identity_survives_fd_reuse: cleanup keyed on the monotonic
      id removes only the intended subscriber when an fd number is reused.
- [ ] unit_autostart_failure_surfaces_warning: a forced auto-start failure
      yields a `daemon_autostart_failed` warning in the command envelope
      instead of a silent direct-mode fallback.
- [ ] live_eval_object_leak_soak: green because the auto-started daemon
      registers and `daemon status` reports its pid within the poll window
      (root cause fixed, not just deflaked).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Supervision stays panic-based, not restart-based: a worker panic means the
  daemon's invariants are unknown, so fail the whole daemon and let the next
  CLI invocation spawn a fresh one (that recovery path already works —
  Firefox-death cleanup at `server.rs:550-554` proves it).
- The signal handler only sets the existing atomic shutdown flag — all
  cleanup continues to run on the main thread, keeping the handler
  async-signal-safe.
- No new dependencies: `libc` and `windows-sys` are already direct deps; the
  old "avoid signal-hook" rationale is stale.

## Out of scope

- Daemon *session* semantics (target-switch re-watch, buffer eviction,
  concurrent-client response routing) — [[iteration-101-daemon-session-correctness]].
- Reconnect-to-Firefox on connection loss (fresh-daemon-per-Firefox remains
  the model).
- Windows CI enablement for daemon e2e tests (tracked CI gap; unit-level
  coverage of the shared plumbing only).

## References

- [[deep-review-2026-07-fable5]] — findings A4, A9, D (signal no-op).
- [[iteration-99-ci-preexisting-reds]] — Theme B (daemon auto-start
  registration) moved here; iter-99 keeps only the cookies stack overflow.
- `crates/ff-rdp-cli/src/daemon/server.rs:369-399, 468-486, 1007-1026, 1174-1207`
- `crates/ff-rdp-cli/src/daemon/client.rs:263-320, 999-1009`
- `crates/ff-rdp-cli/tests/eval_object_leak_soak.rs` — the soak test whose red
  Theme B closes for the right reason.
