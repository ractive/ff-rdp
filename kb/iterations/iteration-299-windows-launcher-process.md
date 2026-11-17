---
title: "Iteration 299: Windows launcher process"
type: iteration
date: 2026-11-17
status: done
branch: iter-299/windows-launcher-process
depends_on: []
tags: [iteration, windows, launch, stop]
---

# Iteration 299: Windows launcher process

On Windows the `firefox.exe` ff-rdp spawns is Firefox's launcher process, which starts the real
browser as its child; the browser holds the debug port. ff-rdp recorded the launcher as the
owner, so `stop` and `launch --replace` refused the browser, a headed launch failed outright,
`stop` mistook a terminated browser for a running one, and a pipe-captured `launch` never
returned. Found by the setup-ff-rdp smoke (run 44042813450); diagnosed on windows-latest in runs
44154936841 and 44156332640.

## Tasks

- [x] `-wait-for-browser` on Windows (`launch.rs::browser_command`)
- [x] Re-mark the owner marker (managed profile or `--profile` ownership record) with the browser
      behind the launcher; report it as `results.pid`, plus `results.launcher_pid`
      (`launch/launcher_process.rs`)
- [x] `is_process_alive` on Windows: alive only while the process has not signalled exit
- [x] Clear `HANDLE_FLAG_INHERIT` on ff-rdp's std handles before spawning Firefox
- [x] `windows-launch.yml`: scenarios A, A2, B, C, F, G on windows-latest with asserted outcomes

## Acceptance Criteria [6/6]

Proof: windows-launch run [44163797355](https://github.com/ractive/ff-rdp/actions/runs/44163797355)
(windows-latest, built from the PR head; each line below is an asserted step).

- [x] Windows, headless: `ff-rdp --port N stop` after `launch --headless --temp-profile` exits 0
      with `stopped:true`, `profile_removed:true`.
- [x] Windows: `ff-rdp stop --all` stops a headless launch.
- [x] Windows: `launch --replace` reports `meta.replaced.stopped:true` and a new `results.pid`;
      `stop` then succeeds.
- [x] Windows: a headed `launch` (no `--headless`) succeeds and `stop` stops it.
- [x] Windows: `out=$(ff-rdp launch --headless …)` returns within 30 s.
- [x] Windows: a `--profile` launch is stopped by `stop`, and the user's profile is kept.

## Design notes

- The hand-over is adopted only when the port's listener is not the spawned PID, its parent is
  the spawned PID, the spawned PID is still running, and the listener is a Firefox executable
  (`launcher_process::browser_behind`, a pure function over a `ProcessView` trait).
- The early launcher-PID marker stays for an interrupted launch: `stop --all` and the profile
  prune go by the marker PID, and killing the launcher takes the browser down through its job
  object. `stop --port` goes by the listener, so it cannot reach that window.
- A browser that cannot be identified (after ~2 s of retries) keeps the spawned PID as owner and
  becomes a `results.warnings` entry.
- Disabling the launcher process was rejected: its state lives in
  `HKCU\SOFTWARE\Mozilla\Firefox\Launcher`, keyed by binary path and shared with the user's
  desktop Firefox.
- Linux and macOS are unchanged: `find_browser` answers `NotNeeded` there, `-wait-for-browser` and
  the handle change are `#[cfg(windows)]`.

## Out of scope

- setup-ff-rdp's smoke deliberately hard-fails Windows `stop` for ff-rdp > v0.5.0; it is updated
  in that repo.
- Elevated launches, Windows on ARM, and installs with the launcher disabled — `kb/backlog.md`.
