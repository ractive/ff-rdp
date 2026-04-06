---
title: "Iteration 13: Connection Daemon (SSH ControlMaster Pattern)"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - daemon
  - connection
  - performance
  - cross-platform
status: completed
branch: iter-13/connection-daemon
---

# Iteration 13: Connection Daemon

Background daemon that holds a persistent Firefox RDP connection, subscribes to all watcher resource types, and buffers events. Eliminates per-invocation TCP overhead and enables cross-command workflows like `navigate` then `network` as two separate calls.

## Background

Each CLI invocation currently opens a fresh TCP connection to Firefox (~50-100ms overhead: TCP connect + RDP greeting + listTabs + getTarget). For AI agent workflows that run 5-10 commands in sequence, this adds up. More critically, watcher subscriptions are connection-scoped — `navigate` and `network` are separate connections, so you can't capture a navigation's network traffic without `--with-network`.

See [[research/connection-persistence]] for the full analysis of approaches. See [[research/gradle-daemon-architecture]] for Gradle's daemon pattern (same concept: auto-start, TCP loopback, idle timeout, `--no-daemon`).

## Design

The daemon has two roles:

1. **Transparent proxy**: Forwards RDP frames between CLI and Firefox (serialized, one client at a time)
2. **Event buffer**: Subscribes to watcher resource types at startup, buffers events, serves them to CLI on request

### The "daemon" Virtual Actor

The daemon exposes itself as a virtual actor named `"daemon"` on the CLI↔daemon channel. Same wire format as RDP (length-prefixed JSON), same `transport.send()`/`transport.recv()`. No new framing, no new parsing, no second channel.

The daemon distinguishes messages by the `"to"` field:
- `"to": "daemon"` → handle locally (drain buffered events, status query)
- anything else → forward to Firefox as raw RDP frame

Firefox actors are always named like `server1.conn0.child2/consoleActor3` — `"daemon"` will never collide.

### CLI↔Daemon Messages

**Drain buffered events:**
```json
← {"to": "daemon", "type": "drain", "resourceType": "network-event"}
→ {"from": "daemon", "events": [...]}
```

Returns all buffered events of that type and clears the buffer. The `events` array contains the original RDP event payloads as received from Firefox.

**Status query:**
```json
← {"to": "daemon", "type": "status"}
→ {"from": "daemon", "uptime_secs": 142, "buffer_sizes": {"network-event": 23, "console-message": 5}}
```

### CLI Branching

Only commands that use watcher subscriptions need daemon-aware code (`network`, `navigate --with-network`). All other commands pass through transparently.

```rust
// network command
if daemon_mode {
    transport.send(json!({"to": "daemon", "type": "drain", "resourceType": "network-event"}));
    let response = transport.recv()?;
    let events = parse_network_events(&response["events"]);
} else {
    // Current path
    watch_resources(&["network-event"]);
    let events = collect_network_events(transport);
    unwatch_resources(&["network-event"]);
}
```

### Daemon Lifecycle

1. **Startup**: Connect to Firefox, read and cache greeting, `getWatcher` → `watchResources(["network-event", "console-message", "error-message"])`
2. **Background**: Continuously read from Firefox. Watcher events go into the buffer. Other messages (command responses) are forwarded to the connected CLI client if any.
3. **Accept CLI client**: Send cached greeting, then bidirectional forwarding + handle `"to": "daemon"` messages
4. **Idle timeout**: `unwatchResources`, disconnect from Firefox, remove registry file, exit
5. **Crash/signal**: Same cleanup as idle timeout

### Message Routing in the Daemon

```
From Firefox:
  ├─ Is a watcher event (resources-available-array / resources-updated-array)?
  │   ├─ Yes → buffer it (do NOT forward to CLI)
  │   └─ No  → forward to CLI (if connected), else discard

From CLI:
  ├─ "to" == "daemon"?
  │   ├─ Yes → handle locally (drain / status), respond to CLI
  │   └─ No  → forward to Firefox as raw RDP frame
```

The daemon inspects two things: the `type` field on Firefox messages (to identify watcher events) and the `to` field on CLI messages (to identify daemon commands). Everything else is opaque byte forwarding.

### Event Buffer

- `HashMap<String, Vec<Value>>` — keyed by resource type
- Max entries per type: 10,000 (oldest evicted when full)
- `drain` returns all buffered events and clears the buffer for that type

## Architecture

```
┌─────────┐  TCP loopback   ┌──────────────┐  TCP (persistent)  ┌─────────┐
│ ff-rdp   │◄───────────────►│   daemon     │◄──────────────────►│ Firefox  │
│ (CLI)    │  RDP frames +   │              │  RDP frames        │          │
│          │  "daemon" actor │  ┌────────┐  │                     │          │
└─────────┘                 │  │ event  │  │                     └─────────┘
                             │  │ buffer │  │
┌─────────┐  (queued)       │  └────────┘  │
│ ff-rdp   │- - - - - - - -►│              │
│ (CLI)    │  waits for      │  registry:   │
└─────────┘  current client  │  daemon.json │
                             └──────────────┘
```

### Registry File

Location: `~/.ff-rdp/daemon.json`

```json
{
  "pid": 12345,
  "proxy_port": 16000,
  "firefox_host": "localhost",
  "firefox_port": 6000,
  "started_at": "2026-04-06T12:00:00Z"
}
```

## CLI Flags

```
--no-daemon          Don't use or start a daemon (direct Firefox connection)
--daemon-timeout N   Daemon idle timeout in seconds (default: 300)
```

These are global flags (apply to all commands), added alongside `--host`, `--port`, `--timeout`.

## Cross-Platform Considerations

| Concern | Approach |
|---------|----------|
| IPC transport | TCP loopback (`127.0.0.1`) — works everywhere |
| Registry location | `dirs::home_dir()/.ff-rdp/` on all platforms |
| Process spawning | `std::process::Command` with stdout/stderr redirected to log file |
| PID liveness check | `kill -0` (Unix), `OpenProcess` (Windows) |
| Daemon detach | `setsid` / `pre_exec` on Unix; `CREATE_NO_WINDOW` on Windows |
| File locking | `fs2::FileExt::try_lock_exclusive()` on registry file |

### PID Liveness (Cross-Platform)

```rust
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    { unsafe { libc::kill(pid as i32, 0) == 0 } }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::*;
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() { return false; }
        unsafe { CloseHandle(handle); }
        true
    }
}
```

### Daemon Process Detach

Unix: `std::process::Command` + `pre_exec(setsid)` + redirect stdout/stderr to `~/.ff-rdp/daemon.log`

Windows: `std::process::Command` with `CREATE_NO_WINDOW` creation flag

## Tasks

### Part A: Daemon Process

- [x] Implement daemon as hidden subcommand `ff-rdp _daemon` (single binary distribution)
- [x] Daemon main loop:
  - [ ] Connect to Firefox, cache greeting
  - [ ] `getWatcher` → `watchResources(["network-event", "console-message", "error-message"])`
  - [ ] Listen on TCP loopback (random port)
  - [ ] Write registry file (`~/.ff-rdp/daemon.json`) with PID + port
  - [ ] Accept one client at a time (queue with timeout)
  - [ ] Route messages: `"to": "daemon"` → handle locally, else forward to Firefox
  - [ ] Route Firefox messages: watcher events → buffer, else forward to CLI
  - [ ] Handle `drain` requests: return buffered events, clear buffer
  - [ ] Handle `status` requests: return uptime + buffer sizes
  - [ ] Idle timeout: `unwatchResources`, disconnect, remove registry, exit
  - [ ] Signal handler (SIGTERM/SIGINT): same cleanup
- [x] Event buffer: `HashMap<String, Vec<Value>>`, capped at 10k per type
- [x] Greeting cache + replay to CLI clients
- [x] Log file: `~/.ff-rdp/daemon.log`

### Part B: CLI Integration

- [x] Add `--no-daemon` and `--daemon-timeout` global flags to args.rs
- [x] `find_running_daemon()`: read registry file, check PID liveness, try TCP connect
- [x] `start_daemon()`: spawn `ff-rdp _daemon` process, wait for registry file to appear
- [x] `resolve_connection_target()`: daemon port if available, else auto-start, else direct
- [x] Handle stale registry (PID dead): clean up and start fresh daemon
- [x] Handle daemon connection failure: fall back to direct Firefox connection with warning

### Part C: Command Adaptation

- [x] `network`: in daemon mode, send `{"to":"daemon","type":"drain","resourceType":"network-event"}` instead of `watchResources` + drain + `unwatchResources`
- [x] `navigate --with-network`: in daemon mode, navigate then drain buffered network events (no subscribe/unsubscribe)
- [x] `inspect`: error with actionable message when used with `--no-daemon` (grip actors don't survive across connections)
- [x] `inspect`: detect stale/invalid grip actor and suggest re-running `eval`
- [x] All other commands: no changes (transparent pass-through)

### Part D: Cross-Platform

- [x] PID liveness check: Unix (`kill -0`) + Windows (`OpenProcess`)
- [x] Daemon detach: Unix (`setsid` + `pre_exec`) + Windows (`CREATE_NO_WINDOW`)
- [x] Registry directory creation: `dirs::home_dir()/.ff-rdp/`
- [x] File locking on registry file (`fs2` or `fd-lock` crate)

### Part E: Testing

- [x] Unit tests for event buffer (insert, drain, eviction at cap)
- [x] Unit tests for greeting cache + replay
- [x] Unit tests for message routing (`"to": "daemon"` → local, else forward)
- [x] Unit tests for registry file read/write/cleanup
- [x] Unit tests for PID liveness check
- [x] Integration test: start daemon, run multiple CLI commands, verify results
- [x] Integration test: `navigate` then `network` as separate calls — network sees navigation's events
- [x] Integration test: daemon idle timeout (exits after period)
- [x] Integration test: stale registry cleanup
- [x] Integration test: `--no-daemon` bypasses daemon (existing behavior preserved)
- [x] Integration test: daemon handles Firefox disconnect gracefully
- [x] Cross-platform CI verification (Linux, macOS, Windows)

### Part F: Documentation

- [x] Update README with daemon section:
  - How it works (auto-start, idle timeout, transparent)
  - The `"daemon"` virtual actor protocol
  - `--no-daemon` and `--daemon-timeout` flags
  - Cross-command workflows (`navigate` then `network`)
  - Troubleshooting (stale registry, daemon log location)
- [x] Document registry file location and format
- [x] Add decision log entry: DEC-016 (daemon with virtual actor protocol)
- [x] Document `--with-network` deprecation path (still works, no longer necessary with daemon)

## Acceptance Criteria

- First `ff-rdp` invocation auto-starts daemon in background
- Subsequent invocations reuse daemon (visible as faster execution)
- `ff-rdp navigate https://example.com && ff-rdp network` captures the navigation's network events
- `ff-rdp navigate https://example.com && ff-rdp console` shows console messages from the navigation
- `ff-rdp --no-daemon eval "1+1"` connects directly to Firefox (existing behavior)
- `ff-rdp --no-daemon inspect <actor_id>` errors with actionable message
- Daemon exits after 5 minutes of inactivity (configurable via `--daemon-timeout`)
- Stale registry files are cleaned up automatically
- Works on Linux, macOS, and Windows
- All existing commands work identically through daemon
- All existing tests pass with `--no-daemon`
- `--with-network` still works (but no longer necessary in daemon mode)

## Design Notes

- **Why a virtual actor, not a custom protocol?** Same wire format (length-prefixed JSON), same `transport.send()`/`transport.recv()`, no new parsing code. The daemon is "just another actor" from the CLI's perspective. The `"to"` field routing is trivial.
- **Why serialized, not multiplexed?** Firefox RDP has no request correlation IDs. Multiplexing would require building a new protocol. Serialization is sufficient — CLI commands are fast (<100ms each).
- **Why hidden subcommand?** `ff-rdp _daemon` keeps single-binary distribution. The user never invokes it directly — the CLI auto-spawns it.
- **Concurrency**: If CLI-B connects while CLI-A is active, daemon queues CLI-B (block until CLI-A disconnects, with 5s timeout).
- **Firefox disconnect**: Daemon detects EOF, removes registry, exits. Next CLI invocation auto-starts a fresh daemon.
- **`--with-network` deprecation path**: With the daemon, `--with-network` is no longer necessary — `navigate` then `network` works. Keep the flag working but document that daemon mode is preferred.

## Open Questions

- Should there be a `ff-rdp daemon stop` command for manual cleanup?
- Max buffer size: 10k events per type? Configurable?
- What's the right queue timeout for concurrent clients? 5s? 10s?
- Should the daemon also buffer `console-message` events or only `network-event`? (Buffering console messages enables `navigate` then `console` workflow, which is useful.)

## Risk: Threading Model

The daemon must simultaneously:
1. Read from Firefox → route to buffer or CLI
2. Read from CLI → route to Firefox or handle `"to": "daemon"`
3. Monitor idle timeout
4. Accept new client connections

Recommended: `mio` or `polling` crate for non-blocking I/O on a single thread (event loop). Simpler than tokio, avoids async complexity, handles all four concerns in one select loop.
