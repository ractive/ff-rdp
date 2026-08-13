---
branch: iter-153/launch-replace-double-envelope
date: 2026-08-12
depends_on:
  - kb/iterations/iteration-151-residual-live-firefox-leak.md
dogfood_path: |
  ff-rdp launch --headless --debug-port 6000
  ff-rdp launch --headless --debug-port 6000 --replace | python3 -c 'import json,sys; json.load(sys.stdin)'
  # → must parse as ONE JSON document. Today it raises "Extra data" because two
  #   top-level envelopes are written back to back.
  ff-rdp launch --headless --debug-port 6000 --replace --jq '.results.pid'
  # → must print the pid of the Firefox that was just LAUNCHED, not the one stopped.
first_call_sites: []
status: done
title: "Iteration 153: launch --replace emits two JSON envelopes and hides the launched PID"
type: iteration
tags:
  - iteration
---

# Iteration 153: `launch --replace` emits two JSON envelopes and hides the launched PID

Found while fixing the `--replace` orphan class in
[[iteration-151-residual-live-firefox-leak]]'s PR (#188). Confirmed on the wire, not inferred.

## The defect

`ff-rdp launch --replace`, when it has a prior instance to stop, writes **two top-level JSON
objects** to stdout, back to back:

```json
{
  "results": { "stopped": true, "pid": 86210 },
  "total": 1
}
{
  "results": { "pid": 86352, "host": "127.0.0.1", "port": 63076, "headless": true, ... },
  "total": 1,
  "meta": { "firefox": "/Applications/Firefox.app/Contents/MacOS/firefox" }
}
```

The first envelope is `stop_prior_instance`'s; the second is the launch's.

This violates the project's JSON-only output contract. Concretely:

- `serde_json::from_slice` / `json.load` over the command's stdout **fails** ("trailing
  characters" / "Extra data"). Every programmatic consumer of `launch --replace` is broken.
- `--jq` operates on a single envelope, so its behaviour here is undefined at best — the
  dogfood path above pins what it must do.
- The **launched** PID is invisible to anyone who parses only the first envelope, while the
  first envelope's `pid` field means something entirely different (the process that was
  *stopped*). A caller that reads `.results.pid` naively gets a dead PID and believes it is
  the new instance.

## How it was found

`live_123_daemon_autostart_and_registry.rs` leaked exactly one Firefox per run. A guard bound
from `serde_json::from_slice(&stdout)["results"]["pid"]` silently produced `None` — the parse
failed on the second envelope — so nothing owned the replacement. `live_86_perf_field_fixes.rs`
did *not* leak, purely because its topology has no daemon record to stop and therefore emits a
single envelope. Same code, opposite outcome, decided by whether a prior daemon record existed.

Both call sites now stream-parse and take the last pid-bearing envelope
(`serde_json::Deserializer::into_iter().last()`) as a **workaround**. That workaround should be
deleted by this iteration once the output is single-envelope.

## Themes

### Theme A — one command, one envelope

Decide and implement the correct single-envelope shape. Options to weigh (pick one, record the
call in [[decision-log]]):

- Fold the stop outcome into the launch envelope's `meta` (e.g.
  `meta.replaced: {"stopped": true, "pid": <prior>}`) — keeps `results.pid` unambiguously the
  launched instance, which is what a caller of `launch` asks for.
- Emit only the launch envelope and demote the stop line to stderr/`--verbose`.

`results.pid` must mean *the process this command started*, in every mode.

### Theme B — audit for the same shape elsewhere

`stop_prior_instance` is not necessarily the only helper that prints its own envelope from
inside another command's run. Sweep every command that can internally invoke another
command's print path and assert one envelope per invocation.

### Theme C — pin it with a test that parses

A test that greps stdout for a substring would not have caught this. The regression test must
**parse** the whole stdout as a single JSON document and fail on trailing data.

## Acceptance Criteria [4/4]

- [x] live_153_replace_emits_single_envelope: stdout of `launch --replace` against a prior
      instance *with* a daemon record parses as exactly one JSON document (no trailing data)
      [verified: 2026-08-13, `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_153
      -- --include-ignored` → 3 passed / 0 failed in 5.32s; stdout parsed as exactly one JSON
      document, replacement pid=40242]
- [x] live_153_replace_reports_launched_pid: `results.pid` of that envelope is the PID of the
      newly launched Firefox — alive immediately after the command — and not the stopped one
      [verified: 2026-08-13, same run → `results.pid=40301` alive and distinct from the stopped
      prior instance pid=40039]
- [x] live_153_replace_reports_stopped_instance: the stopped instance's PID is still
      discoverable in the chosen shape (nothing is silently dropped in the fix) — the stop
      outcome is folded into `meta.replaced` rather than dropped
      [verified: 2026-08-13, same run → `meta.replaced={stopped: true, pid: 40038}`]
- [x] `unit_153_no_nested_envelope_prints` (+ `stop_daemon_and_build_result`): an audit test
      asserts no command path prints a second top-level envelope from inside another command's
      run — it scans the crate source for any
      `run_daemon_stop(cli, ...)` call site outside `dispatch.rs`'s daemon-stop arm.
      Verified pre-fix state structurally: on `main`, `stop_prior_instance` called
      `run_daemon_stop` at `daemon/client.rs:1160`, which prints its own envelope; that call site
      is replaced by the non-printing `stop_daemon_and_build_result`. `cargo test -p ff-rdp-cli
      --bin ff-rdp unit_153` → 1 passed / 0 failed.

## Notes

- Delete the stream-parse workarounds in `live_86_perf_field_fixes.rs` and
  `live_123_daemon_autostart_and_registry.rs` as part of this iteration, and let their guards
  bind straight from `results.pid` again.
- This is the same honesty family as [[iteration-149-a11y-restore-honesty]]: the command did
  something (started a process) and did not tell the caller in a usable way. Here the
  consequence is a real leak, since a caller cannot clean up a PID it was never given.
- Verify on the wire before fixing — across 135–151 the stated root cause diverged from
  reality at least eight times. The evidence above was captured from an actual run; reproduce
  it before changing anything.
</content>
