---
title: "Iteration 90: daemon lifecycle — launch and daemon share state, single source of truth"
type: iteration
date: 2026-05-29
status: planned
branch: iter-90/daemon-lifecycle-state-sharing
depends_on:
  - iteration-87-gate-hardening-required-checks-and-dogfood-linter
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-58.md
  - kb/dogfooding/field-report-perf-2026-05-27.md
  - kb/iterations/iteration-86-perf-field-report-fixes.md
  - kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md
first_call_sites:
  - primitive: DaemonRecord (PID, port, headless, launched_at) read/written by both launch and daemon
    site: crates/ff-rdp-cli/src/daemon_record.rs
  - primitive: launch writes DaemonRecord on successful Firefox spawn
    site: crates/ff-rdp-cli/src/commands/launch.rs
  - primitive: "daemon stop reads DaemonRecord, SIGTERMs PID, waits for port to free"
    site: crates/ff-rdp-cli/src/commands/daemon.rs
  - primitive: launch --replace reads DaemonRecord, stops prior, then proceeds
    site: crates/ff-rdp-cli/src/commands/launch.rs
dogfood_script: iteration-90-daemon-lifecycle-state-sharing.dogfood.sh
tags:
  - iteration
  - daemon-lifecycle
  - bugfix
  - carry-over
---

# Iteration 90 — daemon and launch share one record

Session-58 confirms the iter-86 daemon-stop fix doesn't actually work:

```
$ ff-rdp launch --headless --port 6000   # OK
$ ff-rdp daemon stop
{"results": {"reason": "not running", "stopped": false}, "total": 1}
$ lsof -i :6000
firefox 15228 ... TCP localhost:6000 (LISTEN)
$ ff-rdp launch --headless --port 6000
error: port 6000 is already in use by firefox (PID 15228).
$ ff-rdp launch --replace --headless --port 6000
error: port 6000 is still in use after stopping the prior instance.
```

The root cause is exactly what session-58 named: `daemon stop` and
`launch` don't share state. `daemon stop` only tracks instances started
via `daemon start`; `launch` registers nothing it can later stop;
`launch --replace` calls the same blind stop path. End-user impact:
identical to the original field-report bug — `kill -9` still required.

iter-86 tried to fix the visible symptom (port leak after stop) without
fixing the underlying split-state. iter-90 fixes the root cause:
**every Firefox spawned by ff-rdp goes through the same record**.

## Hard rule

Do not tick an AC checkbox until `iteration-90-….dogfood.sh` exits 0
on a live FF 151 and writes `/tmp/ff-rdp-iter-90-dogfood-ok`.

## Pre-fix repro

Per the [[iteration-87-gate-hardening-required-checks-and-dogfood-linter#pre-fix-repro-convention|iter-87 convention]],
`pre_fix_repro_daemon_state_sharing_red_then_green` launches Firefox via
`ff-rdp launch`, calls `ff-rdp daemon stop`, and asserts:

- on `origin/main`: response JSON contains `"reason": "not running"`
  AND port 6000 stays held (current bug).
- on branch HEAD: port 6000 is free within 3 s AND a follow-up `launch`
  succeeds.

## Tasks

### Theme A — `launch` and `daemon` share one record [0/5] [pre_fix_repro_test: pre_fix_repro_daemon_state_sharing_red_then_green]

- [x] Introduce `DaemonRecord` in
      `crates/ff-rdp-cli/src/daemon_record.rs` with fields: `pid: u32`,
      `port: u16`, `headless: bool`, `launched_at: chrono::DateTime<Utc>`,
      `profile_dir: PathBuf`. Single on-disk location:
      `~/.cache/ff-rdp/daemon.json` (Linux/macOS) or
      `%LOCALAPPDATA%\ff-rdp\daemon.json` (Windows) via the `directories`
      crate. Atomic write (write-to-temp + rename). Read-with-staleness
      check: if the recorded PID is not running, the record is treated
      as absent.
- [x] `launch` writes the record on successful Firefox spawn (after the
      RDP socket is reachable). Cleans the record on its own shutdown
      (Ctrl-C handler) when run in foreground mode; leaves it in
      background/daemon mode.
- [x] `daemon stop` reads the record. If present and the PID is alive,
      SIGTERM, wait up to 2 s for graceful exit, SIGKILL if needed,
      poll the port until free (max 3 s), remove the record. If absent
      or PID dead, response stays the existing `"reason": "not running"`
      shape. JSON response includes `"pid"` and `"port"` so the user can
      verify which instance was stopped.
- [x] `launch --replace` (and `--force` alias) reads the record; if
      present, runs the stop path internally before proceeding to
      spawn. Test `unit_daemon_record_round_trip` covers the JSON
      serialization in both directions (PID, port, headless,
      timestamp).
- [x] dogfood_script Theme A block exits 0: `launch → daemon stop →
      launch` works without manual `kill`, and `launch --replace`
      against a live prior instance succeeds.

## Acceptance Criteria [0/5]

- [x] pre_fix_repro_daemon_state_sharing_red_then_green: `launch` then
      `daemon stop` leaves the port held on `origin/main`, frees it on
      branch HEAD. Verified by `xtask check-pre-fix-repro`.
- [x] unit_daemon_record_round_trip: serialize → deserialize round-trip
      preserves PID, port, headless, timestamp; staleness check returns
      `None` when the recorded PID is dead.
- [x] live_daemon_stop_after_launch_frees_port: `ff-rdp launch
      --headless --port 6000`, then `ff-rdp daemon stop`, then poll
      `localhost:6000` for refusal within 3 s; asserts port is free.
- [x] live_launch_replace_handles_prior_instance: with a live Firefox
      already running via `launch` on port 6000, `ff-rdp launch
      --replace --headless --port 6000` succeeds and the new PID
      differs from the prior PID.
- [x] dogfood_script_full_run_iter_90: sibling `.dogfood.sh` exits 0
      and writes `/tmp/ff-rdp-iter-90-dogfood-ok`.

## Out of scope

- A real supervisor for daemon mode (systemd-style restart-on-crash,
  log rotation, health checks). The current bug is state-sharing, not
  supervision.
- Daemon-mode event streaming (the iter-37/38 follow-up work).
- Multi-instance support (more than one ff-rdp-managed Firefox at a
  time). One record, one port, one PID.
- Cross-user / system-wide daemon registry. Per-user cache dir is
  enough for the current bug.

## References

- [[dogfooding-session-58]] — the verification that iter-86's fix didn't land
- [[field-report-perf-2026-05-27]] — original user-facing report
- [[iteration-86-perf-field-report-fixes]] — the previous attempt
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]] — the gate
