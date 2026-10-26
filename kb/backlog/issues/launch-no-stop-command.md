---
title: "launch: no command stops the Firefox that ff-rdp launched"
type: feature
status: resolved
priority: medium
discovered: 2026-10-26
reporter: saturnus
ff_rdp_version: ff-rdp 0.4.2 (fe5eb7256985 2026-10-18)
tags: [launch, lifecycle]
resolved_by: 344
---

`ff-rdp launch --headless` starts a Firefox that keeps running after the work is done. Nothing in
ff-rdp shuts it down again: `--help` lists `launch` and `doctor` under Lifecycle, but no `stop`,
`quit` or `close`. The only stop path is `launch --replace`, which stops the old instance and starts
a new one. To end the session an agent has to find the process itself (`pgrep -fl ff-rdp-profile`)
and `kill` the PID. The PID is in launch's output, but a later session has to grep for it. The
`profiles` help says so outright ("A launched Firefox is never stopped by ff-rdp on its own"); its
temporary profile stays until `profiles prune` or the next `launch` removes it.

## Repro

```sh
ff-rdp launch --headless
ff-rdp navigate https://example.com
# done; now stop it:
ff-rdp --help | grep -iE 'stop|quit|close'   # nothing
pgrep -fl ff-rdp-profile                      # the headless Firefox is still running
```

## Wanted

`ff-rdp stop`, the same graceful path `--replace` uses (SIGTERM, then SIGKILL after 2 s). It should
remove that instance's own temporary profile and print the JSON envelope `{"results": {"stopped": true, "pid": N}}`,
or `stopped: false` when nothing is running on the port.

## Design (triage 2026-10-26)

`ff-rdp stop` (Lifecycle group, next to `launch`):

- `ff-rdp stop` — the Firefox on the current port (`--port` / `FF_RDP_PORT`, default 6000), only if
  ff-rdp launched it. SIGTERM → 2 s → SIGKILL (shared with `launch --replace`), then removes that
  instance's managed profile dir. Envelope `{"results": {"stopped": true, "pid": N, "profile_removed": true}}`;
  `stopped: false` when nothing listens on the port.
- `ff-rdp stop --all` — every live ff-rdp-launched Firefox, found from the owner markers in the
  managed profile roots (no port needed). Results list one entry per instance.
- `--dry-run` — report what would be stopped, signal nothing.
- No automatic stop on exit or idle: per-command connections mean Firefox must outlive every command.

### Safety: `stop` must not be able to kill an arbitrary process

ff-rdp runs with the caller's rights, so the threat is not privilege escalation but ff-rdp being
steered (by an agent, a stale file, a recycled PID, a planted marker) into signalling a process it
did not start. Rules, all fail-closed:

1. **No PID input.** `stop` takes a port or `--all`, never `--pid`. The PID always comes from the
   OS (port listener) and must be confirmed by an ff-rdp marker; a user-supplied number never
   reaches `kill`.
2. **Ownership requires the start token.** Kill only on `OwnerLiveness::Live` *with* a recorded
   `.ff-rdp-owner-start` token that matches the live PID's current token. Today a missing token
   degrades to `Live` (`profile_dir.rs` `owner_liveness`, "Pre-iter-171 profile") — acceptable for
   prune's keep/delete decision, not for a kill. `Unverified`, `Unreadable`, `Unmarked` → refuse.
3. **Trusted roots only.** Markers are read only from roots that pass `root_is_trustworthy`
   (owned by the current user, not group/world-writable) — already the case for
   `pid_is_ff_rdp_spawned`; `--all` must use the same gate.
4. **It must be Firefox.** The process's executable name must be a Firefox binary
   (`firefox`, `firefox-bin`, `firefox.exe`, …) before signalling.
5. **Process-group kill only for a group leader.** `kill_process_group` sends to `-pid` assuming
   Firefox called `setsid()`. Verify `getpgid(pid) == pid` before any group signal; if not, signal
   the PID alone. Never signal a pre-captured PGID that differs from the PID (it could be the
   caller's shell job group). Audit the `--replace` escalation path for the same.
6. **Re-verify before every signal** (port owner and marker/token), as `--replace` already does.
7. **Refusal is an error, not a skip.** A port held by a non-ff-rdp process returns an error
   naming the PID and process; under `--all`, unowned candidates never appear at all.

### Integration

- `launch` hints suggest `ff-rdp stop`; `doctor` reports whether the listener is ff-rdp-launched
  and hints `stop` when it is; the port-in-use error from `launch` names `stop` alongside
  `--replace`; `profiles` help ("never stopped by ff-rdp on its own") and `prune` output point at
  `stop`; skill doc regenerated.

### Proofs

Real Firefox: `stop` on a launched instance (process gone, profile gone); `stop` with nothing
running (`stopped: false`); `stop --all` with two launched instances; `stop` refusing a Firefox
started by hand on the same port with a raw profile (still running afterwards); `stop --dry-run`.
Unit tests for every refusal branch (missing token, token mismatch, untrusted root, non-Firefox
executable, pgid ≠ pid).
