---
title: "Iteration 222: live_123's decoy-port eval fails under sweep contention and the assertion says nothing about why"
type: iteration
date: 2026-08-30
status: planned
branch: iter-222/live-123-daemon-autostart-under-load
depends_on: []
first_call_sites:
  - primitive: (none — test-only change; no new pub item)
    site: crates/ff-rdp-cli/tests/live/live_123_daemon_autostart_and_registry.rs
dogfood_path: |
  firefox -no-remote --start-debugger-server 6000 --headless   # raw browser, NOT ff-rdp launch
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # expected: 0 failures. The 2026-08-30 sweep on iter-220 reported
  #   live_123 …targets_debug_port_not_cli_port FAILED  ("eval on decoy port should succeed")
  # while the same test passed in isolation and in the immediately preceding sweep.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_daemon_stop_prior_instance_targets_debug_port_not_cli_port -- --include-ignored
  # expected: ok — it always is alone. That gap is the thing to close.
tags: [iteration, live-tests, daemon, carry-over, flake]
---

# Iteration 222: `live_123`'s decoy-port eval fails under sweep contention

## Why

Carry-over from [[iteration-220-with-page-after-navigating-click]]'s closing sweep.

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 → LIVE_SWEEP_SUMMARY
  executed=313 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=313
  301 passed / 3 failed (CLI tier)

live_123_daemon_autostart_and_registry::live_daemon_stop_prior_instance_targets_debug_port_not_cli_port
  panicked at live_123_daemon_autostart_and_registry.rs:320:
  eval on decoy port should succeed
```

Evidence on how load-dependent it is, all on the same commit:

| run | result |
|---|---|
| full sweep, 2026-08-30 21:5x | **FAILED** |
| full sweep, 2026-08-30 21:2x (three commits earlier, same test code) | ok |
| `cargo test … live_daemon_stop_prior_instance_targets_debug_port_not_cli_port` alone | ok, 8.1 s |

This is exactly the shape `kb/discipline-rationale.md` and the `iteration-close` skill warn
about: iter-153 shipped a broken feature certified by a truthful isolated run. A test that only
fails inside a sweep is not "environmental" — it is a test (or a product) that does not survive
contention, and the sweep is the only place anyone ever sees it.

## What the failure does and does not tell us

The test launches **two** `LiveFirefox` instances back to back and then runs `eval 1` against
each, autostarting a proxy daemon per port inside an isolated `FF_RDP_HOME`. Only the first
`eval` failed. `assert!(ok_decoy, "eval on decoy port should succeed")` asserts a **bool** — so
the record contains no exit code, no stderr, no envelope, and no way to tell apart:

- the daemon autostart lost a race under load (the likeliest reading — two Firefoxes plus ~300
  other live tests were competing for CPU),
- `eval` hit the direct-connection fallback and that failed,
- Firefox on the decoy port was not up yet despite `LiveFirefox` returning.

Nothing in the sweep log distinguishes them. That is the first thing to fix, because without it
the next occurrence is just as uninformative.

## Themes

- **A — Make the assertion say what happened.** `run_json` already has the `Output`; the panic
  should carry exit status, stdout and stderr, the way `live_210`'s `run_json` does. A red that
  names its cause is worth more than a red that is merely reproducible.
- **B — Then find the race.** With a real message, decide whether the fix belongs in the test
  (wait for the daemon the way `wait_daemon_running` already does, *before* the first `eval`
  rather than after) or in `eval`'s daemon-autostart path (a retry, or a longer
  `--daemon-timeout` under load). Do not guess before Theme A has produced one honest failure.
- **C — Do not "fix" it by loosening the assertion.** If the daemon autostart genuinely cannot
  survive a loaded machine, that is a product finding about `--daemon-timeout` defaults, and it
  belongs in the Outcome — not papered over with a retry loop in the test.

## Tasks

### A. Honest failure text [0/1]
- [ ] `live_123`'s eval assertions report exit code, stdout and stderr on failure

### B. Diagnose and fix [0/2]
- [ ] Reproduce under artificial load (run the suite with `--test-threads` raised, or alongside a
      CPU hog) and capture one real failure message
- [ ] Fix in the layer the message points at, and say which in the Outcome

### C. Sibling tests [0/1]
- [ ] Check the other `live_123_*` tests (and `live_164`'s autostart tests) for the same
      bool-only assertion shape

## Acceptance Criteria [0/3]

- [ ] `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep` reports
      0 failures across **two consecutive** sweeps, both `LIVE_SWEEP_SUMMARY` lines pasted
- [ ] A deliberately-broken daemon autostart makes `live_123` print the CLI's actual stderr,
      not just `eval on decoy port should succeed`
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean

## Out of scope

- The two `live_166` reds from the same sweep — those are
  [[iteration-214-live-166-cache-304]] (filed as 221 by this sweep, reconciled into 214).

## References

- [[iteration-220-with-page-after-navigating-click]] — the sweep that surfaced this
- `crates/ff-rdp-cli/tests/live/live_123_daemon_autostart_and_registry.rs:320`
- `kb/discipline-rationale.md` — why an isolated pass is not evidence
