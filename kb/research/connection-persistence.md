---
title: Connection Persistence — Sharing a Firefox RDP Connection Across CLI Invocations
type: research
date: 2026-04-06
tags: [research, architecture, connection, daemon, multiplexing]
status: active
---

# Connection Persistence

## Problem

Firefox RDP actor IDs and watcher subscriptions are scoped to a single TCP connection (`conn0`, `conn1`, etc.). Each `ff-rdp` CLI invocation opens a new connection, gets fresh actors, and loses all state when it exits. This means:

- Watcher events (network, console, etc.) are only captured during a single invocation's lifetime
- You can't navigate on one invocation and see the network traffic on the next
- `navigate --with-network` works but is a special case — every command combination would need its own `--with-X` flag

## Constraint

A TCP connection is kernel state tied to a process's file descriptor table. When the process exits, the fd closes, the connection drops, and Firefox cleans up all associated actors. There is no way to "park" a connection without keeping a process alive.

## Approaches Explored

### 1. SSH ControlMaster Pattern (recommended)

SSH solves the identical problem: expensive connection setup (TCP + TLS + auth), desire to reuse across invocations.

**How SSH does it:**
- First `ssh` invocation creates a Unix domain socket at a `ControlPath`
- Forks a background "master" process that holds the TCP connection
- Master listens on the Unix socket, multiplexes SSH channels over the single TCP connection
- Subsequent `ssh` invocations detect the socket, connect to the master instead of opening a new TCP connection
- `ControlPersist=600` auto-exits the master after 10 minutes idle
- `ssh -O exit` explicitly tears down the master

**Applied to ff-rdp:**
- First `ff-rdp` invocation: no master running → connects to Firefox, forks a master process, subscribes to all watchers, runs the command
- Master listens on IPC channel, continuously drains watcher events into a buffer
- Subsequent invocations: master detected → sends command via IPC, master forwards to Firefox on the shared TCP connection, routes response back
- Master auto-exits after idle timeout
- User never explicitly manages daemons — it "just works"

**Key difference from SSH:** SSH has built-in multiplexing (SSH channels). Firefox RDP is a single request/response stream with interleaved events. The master would need to:
1. Serialize commands (one CLI client at a time)
2. Separate watcher events from command responses (by `from` actor field)
3. Buffer watcher events for later retrieval

### 2. REPL / Shell Mode

`ff-rdp shell` — interactive mode with one persistent connection.

- Simple, no IPC
- Doesn't integrate with Claude Code's Bash tool (each tool call is a separate process)
- Good for interactive human use, not for programmatic use

### 3. Read-Only Daemon + Separate Command Connections

Daemon only captures watcher events. Commands still open their own connections for actions.

- **Doesn't work.** Watcher events are connection-scoped. The daemon's watchers won't see events caused by actions on a different connection.

### 4. fd Passing (SCM_RIGHTS)

Pass the TCP socket file descriptor to a new process via Unix domain sockets.

- **Doesn't work.** Even with the fd, the actor IDs and watcher subscriptions are server-side state tied to the original connection context. The new process can read/write the socket but can't use the old actors.

## Cross-Platform IPC

The master process needs an IPC channel. Options:

| Mechanism | Linux/macOS | Windows | Notes |
|-----------|------------|---------|-------|
| TCP loopback (`127.0.0.1:port`) | Yes | Yes | Simplest cross-platform. Any local process can connect. |
| Named pipes | Yes | Yes | `interprocess` crate. Platform-native. |
| Unix domain sockets | Yes | Win10+ | SSH's approach. Not universal on Windows. |

**TCP loopback is the pragmatic choice** for cross-platform support. Master listens on `127.0.0.1:<random>`, writes `{"pid": N, "port": P}` to a lockfile. CLI commands read the lockfile to find the master.

## Architecture Sketch

```
┌─────────┐  TCP loopback   ┌──────────────┐  TCP (persistent)  ┌─────────┐
│ ff-rdp   │◄───────────────►│   master     │◄──────────────────►│ Firefox  │
│ (CLI)    │  send command   │              │  all watchers       │          │
│          │  recv response  │  event buffer│  subscribed         │          │
└─────────┘                 │              │                     │          │
                             │  lockfile:   │                     └─────────┘
┌─────────┐  TCP loopback   │  /tmp/ff-rdp │
│ ff-rdp   │◄───────────────►│  -master.json│
│ (CLI)    │                 └──────────────┘
└─────────┘
```

## Implementation Considerations

- **Async required for master:** Must simultaneously handle IPC clients, Firefox TCP events, and idle timeout. Likely needs `tokio` (only in the master, not in regular CLI path).
- **Command serialization:** Only one CLI command in flight at a time on the Firefox TCP connection. Master queues or rejects concurrent requests.
- **Event routing:** Master distinguishes watcher events (buffered) from command responses (routed to the requesting CLI client) using the `from` field.
- **Lockfile cleanup:** Master removes lockfile on exit. CLI commands handle stale lockfiles (pid no longer running).
- **Transparent to user:** No explicit `daemon start/stop`. First invocation auto-starts master if not running. Master auto-exits after idle.

## Open Questions

- Should the master buffer all events in memory, write to JSONL, or both?
- How large can the event buffer grow on a busy page?
- Should there be a `ff-rdp master stop` escape hatch?
- How does this interact with multiple Firefox instances on different ports?
- What's the right idle timeout default? (SSH defaults to none — explicit ControlPersist needed)
