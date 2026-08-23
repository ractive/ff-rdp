---
title: "Iteration 191: `launch --replace` signals a recycled PID because a stale launch record is treated as ownership proof"
type: iteration
date: 2026-08-23
status: planned
branch: iter-191/stale-launch-record-recycled-pid
depends_on:
  - iteration-178-live-sweep-carryover-watch-conditions
  - iteration-186-launch-records-leak-one-file-per-port
first_call_sites: []
dogfood_path: |
  # Reproduce the 2026-08-23 observation deterministically, without waiting for
  # a random port to collide with one of the ~4 600 leaked records on the
  # machine. The sacrificial PID below MUST be a process you spawned for this
  # purpose — ff-rdp will send it SIGTERM and SIGKILL.

  # 1. Spawn a harmless victim and note its pid.
  sleep 900 &
  VICTIM=$!
  PORT=51999
  echo "victim=$VICTIM port=$PORT"

  # 2. Plant a launch record naming that pid on a port ff-rdp did not launch on
  #    — exactly the shape of ~/.ff-rdp/launch-record.<port>.json after a leak.
  cat > ~/.ff-rdp/launch-record.$PORT.json <<JSON
  {"pid": $VICTIM, "port": $PORT, "headless": true,
   "launched_at": "2026-08-16T20:32:13.855708Z",
   "profile_dir": "/tmp/does-not-exist"}
  JSON

  # 3. Occupy the port with something ff-rdp does not own, so the "port still in
  #    use" path is reached rather than a clean launch.
  #    (Any listener will do; a raw Firefox is what the live test uses.)
  #    Then provoke the replace path:
  cargo run -p ff-rdp-cli -- launch --replace --headless --debug-port $PORT

  # → OBSERVED TODAY: ff-rdp signals $VICTIM (`kill -0 $VICTIM` may now fail)
  #   and reports
  #   {"error":"port <P> is still in use after stopping the prior instance (pid <VICTIM>) ..."}
  # → EXPECTED AFTER THIS ITERATION: no signal is sent to $VICTIM, and the
  #   envelope is the same refusal the port-owner branch already emits —
  #   "... which ff-rdp did not launch ... Refusing to stop a process ff-rdp
  #   does not own".

  # 4. Clean up.
  rm -f ~/.ff-rdp/launch-record.$PORT.json; kill $VICTIM 2>/dev/null

  # 5. The live test that caught this must pass in a full sweep:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
tags: [iteration, safety, launch, daemon, kill-scoping]
---

# Iteration 191: a leaked launch record is not ownership proof

## The observation

[[iteration-178-live-sweep-carryover-watch-conditions]]'s sweep on 2026-08-23
(`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 → LIVE_SWEEP_SUMMARY executed=284 skipped=0
preexisting=0 vanished=0 launch_timeout=0 total=284`, 283 passed / 1 failed) had exactly one
failure, and it was not one of that plan's seven watch conditions:

```text
---- live_110_kill_scoping::live_110_replace_never_kills_foreign_firefox stdout ----
thread '...' panicked at crates/ff-rdp-cli/tests/live/live_110_kill_scoping.rs:76:5:
refusal message must explain ff-rdp will not stop an unowned process; got: {"error":"port 51371
is still in use after stopping the prior instance (pid 65225). Run `ff-rdp doctor` or
`lsof -i :51371` to investigate.","error_type":"User"}
```

The test spawns a raw Firefox on a random port, runs `launch --replace` at that port, and asserts
(a) the foreign browser survives and (b) ff-rdp says it refuses to stop what it does not own.
**(a) held.** (b) did not, and the reason is the defect.

## The mechanism, established from artefacts still on disk

| Fact | Evidence |
|---|---|
| A launch record existed for the random port the test picked | `~/.ff-rdp/launch-record.51371.json`, mtime 2026-08-16 22:32 — seven days before the sweep |
| It named pid 65225 | `{"pid": 65225, "port": 51371, ..., "launched_at": "2026-08-16T20:32:13Z"}` |
| pid 65225 is not, and was never, that Firefox | `ps -p 65225` → `Pencil.app/.../mcp-server-darwin-arm64 --app desktop`, `STARTED Thu Aug 20 23:28:01 2026` — an unrelated process of the operator's, started four days *after* the record was written |
| Nothing in the code re-checked that | `stop_prior_instance_with` branch 1 (`crates/ff-rdp-cli/src/daemon/client.rs:1186-1213`, the escalation call at `:1193`, the error at `:1202`) matches on `rec.port == port && is_alive(rec.pid)` alone |
| It then signalled that pid | `stop_pid_with_full_escalation(rec.pid, Some(rec.port), &deps.hooks, None)` — note the `None`: the `reverify` ownership check is passed only by branch 3 |
| The signals are group-wide | `kill_group_term`/`kill_group_kill` are `process::kill_process_group`/`_force`, i.e. `kill(-pid, SIGTERM)` then `kill(-pid, SIGKILL)` (`daemon/process.rs:234`, `:270`) |

So `launch --replace` sent SIGTERM and then SIGKILL aimed at a process group derived from a
recycled PID belonging to an unrelated application, without ever consulting
`pid_is_ff_rdp_spawned` — the "fails closed: no marker ⇒ no kill" guard that the branch *below*
it (the port-owner fallback, `client.rs:1256-1263`) was added by iter-110 Theme A0 to enforce
after the 2026-07-09 incident. The guard exists; this path simply never reaches it.

**Why nothing died this time.** `kill(-65225, …)` addresses the process *group* 65225. The victim's
ppid is 65140 and its pgid is not 65225, so the call returned ESRCH. The step-4 tree kill was then
skipped by the `pgid_safe_to_kill` guard (`i64::from(group_id) == i64::from(pid)` was false).
Both are coincidences of that particular process's ancestry, not protections: a recycled PID that
*is* a group leader — every process `launch` itself spawns with `process_group(0)`, every shell
job, most daemons — takes the signal and takes its whole group with it.

**Windows is strictly worse.** `kill_process_group` on Windows has no group semantics at all and
falls back to `kill_process(pid)` (`daemon/process.rs:250-251`), a direct kill of the recycled
PID. The ESRCH luck that saved this machine does not exist there.

## What this is not

- **Not the record leak.** That `~/.ff-rdp` holds 4 619 files, one leaked `launch-record.<port>.json`
  per port ever used, is owned by [[iteration-186-launch-records-leak-one-file-per-port]] and is
  deliberately not re-diagnosed here. 186 shrinks the population of stale records; it does not make
  trusting one safe. A single record left by a crash between `launch` and cleanup reproduces this
  with an empty directory.
- **Not [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] again.** 171 (done) fixed the same
  *class* — PID reuse defeating `kill(pid, 0)` — for the `.ff-rdp-owner-pid` marker inside a
  profile directory, where the consequence is a misread liveness and a refused `profiles prune`.
  This is the `launch-record.<port>.json` artefact on a path whose consequence is an outbound
  kill signal. 171's fix did not reach it.
- **Not a load or contention artefact.** Nothing here is timing-dependent; the record, the pid and
  the branch are all deterministic given a collision. The sweep merely supplied the collision by
  choosing a random port.
- **Not a `live_110` test defect.** The test asserted the right thing and its message assertion is
  what caught this. Do not relax it.

## Scope

- [ ] Branch 1 of `stop_prior_instance_with` must establish that `rec.pid` still identifies the
      process the record was written for *before* signalling it — not merely that some process
      holds that pid. The record already carries `launched_at` and `profile_dir`; either is a
      usable cross-check (process start time ≥ `launched_at`, or the owner-PID marker under
      `profile_dir` naming `rec.pid`), and `pid_is_ff_rdp_spawned` already implements the second.
- [ ] When that check fails, take the same refusal the port-owner branch takes — the message must
      say ff-rdp did not launch the process and will not stop it — and leave the record alone for
      186 to garbage-collect, per the iter-158 Theme B rule that a failed stop must not destroy its
      own ownership proof.
- [ ] Decide, and write down, what branch 2 (the proxy-daemon registry path) does about the same
      question. It is not implicated by this observation and may already be safe via the registry's
      own liveness handshake; say which, rather than leaving it unexamined.

## Acceptance Criteria [0/5]

- [ ] A unit test over `stop_prior_instance_with`'s injected `StopDeps` covers "record matches the
      port, its pid is alive, the pid is not ours" and asserts **no kill hook is invoked** — the
      existing hook-logging deps at `client.rs:1367` make this observable
- [ ] The same test asserts the returned `AppError::User` message contains "did not launch" or
      "does not own", i.e. the string `live_110` requires
- [ ] The "still in use after stopping the prior instance" message is emitted only on a path where
      ff-rdp actually did stop something it owned
- [ ] `live_110_kill_scoping::live_110_replace_never_kills_foreign_firefox` passes in a full
      live-sweep run with a stale record planted for the target port (paste the
      `LIVE_SWEEP_SUMMARY` line and the gates)
- [ ] The dogfood path above runs to its "EXPECTED AFTER" outcome: the sacrificial pid is still
      alive after `launch --replace`, and the envelope is the refusal

## References

- [[iteration-178-live-sweep-carryover-watch-conditions]] — the sweep that surfaced this
- [[iteration-186-launch-records-leak-one-file-per-port]] — owns the stale-record population
- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — the same PID-reuse class, fixed for the
  profile marker only
