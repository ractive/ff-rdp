---
title: "Iteration 301: Windows port-owner lookup on non-English Windows"
type: iteration
date: 2026-11-18
status: done
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

- [x] `parse_netstat_pid`: a TCP row is a listener when its foreign address has port 0
      (`0.0.0.0:0`, `[::]:0`); the PID is the last column; the state column is never read;
      a loopback row wins over a wildcard row for the same port
- [x] `parse_tasklist_name`: only a quoted CSV row counts, so no localized message prefix
      (`INFO:` / `INFORMATION:`) is matched; a comma in the image name survives
- [x] Parsers compile under `cfg(any(windows, test))`; unit tests (English, German OEM rows,
      IPv6, TIME_WAIT with the port as remote, prefix-sharing ports, loopback preference, UDP,
      tasklist messages) run on every CI OS
- [x] `tests/live/support/replace_fixture.rs` listener check: same rule, lossy decode (it
      panicked on non-UTF-8 OEM output)

## Acceptance Criteria [4/4]

- [x] On German Windows, `launch --headless --temp-profile --port P` prints no "could not
      identify" warning, `results.pid` is the port's netstat owner and `results.launcher_pid`
      is set
- [x] `--port P stop` then returns `stopped:true, profile_removed:true`, the browser and
      launcher processes are gone, and other Firefox processes are untouched
- [x] `--port P doctor` names the listener PID
- [x] English Windows (`windows-launch` workflow) stays green

## Design notes

- The lookup keeps `netstat -ano -p tcp` (the IPv4 table). `netstat -ano` would add IPv6
  listeners and the UDP table, but `is_port_in_use` probes `127.0.0.1` only, and callers such as
  `port_free_or_held_by` combine the two — a `[::]:P` listener of another process would then
  read as "held" while IPv4 clients see the port free. Firefox's debugger server binds
  `127.0.0.1`. The parser still accepts `[…]:P` rows.
- `GetExtendedTcpTable` (no subprocess, no parsing) was not chosen: it needs the
  `Win32_NetworkManagement_IpHelper` feature and `unsafe` buffer handling for a fix the
  port-0 rule already makes locale-independent.
