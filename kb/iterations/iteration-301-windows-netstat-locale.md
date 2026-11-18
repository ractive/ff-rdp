---
title: "Iteration 301: Windows port-owner lookup on non-English Windows"
type: iteration
date: 2026-11-18
status: in-progress
branch: iter-301/windows-netstat-locale
depends_on: []
tags: [iteration, windows, launch, stop]
---

# Iteration 301: Windows port-owner lookup on non-English Windows

`port_owner::parse_netstat_pid` accepted a `netstat` row only when it contained the English
word `LISTENING`. German Windows prints `ABHÖREN`, which the OEM code page and
`from_utf8_lossy` turn into `ABH�REN`, so `find_listener` returned `Ok(None)` for every port.
On a German Windows 11 machine (Firefox 160) `launch` then warned `could not identify the
browser behind Firefox's launcher process … no listener found on port`, reported the launcher
as `results.pid` with no `launcher_pid`, and `stop` refused with `port … is in use, but its
owner could not be identified`. Every `find_listener` caller (`stop`, `launch` collision check
and `--replace`, the launcher hand-over, `doctor`, the tab-target and connection-meta owner
lines) was affected.

## Tasks

- [ ] `parse_netstat_pid`: a TCP row is a listener when its foreign address has port 0
      (`0.0.0.0:0`, `[::]:0`); the PID is the last column; the state column is never read
- [ ] Run `netstat -ano` (IPv4 and IPv6 rows) instead of `-p tcp` (IPv4 only)
- [ ] `parse_tasklist_name`: only a quoted CSV row counts, so no localized message prefix
      (`INFO:` / `INFORMATION:`) is matched
- [ ] Parsers compile under `cfg(any(windows, test))`; unit tests (English, German OEM rows,
      IPv6, TIME_WAIT with the port as remote, prefix-sharing ports, UDP, tasklist messages)
      run on every CI OS
- [ ] `tests/live/support/replace_fixture.rs` listener check: same rule, lossy decode

## Acceptance Criteria

- [ ] On German Windows, `launch --headless --temp-profile --port P` prints no "could not
      identify" warning, `results.pid` is the port's netstat owner and `results.launcher_pid`
      is set
- [ ] `--port P stop` then returns `stopped:true, profile_removed:true`, the browser and
      launcher processes are gone, and other Firefox processes are untouched
- [ ] `--port P doctor` names the listener PID
- [ ] English Windows (`windows-launch` workflow) stays green
