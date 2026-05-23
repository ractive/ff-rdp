---
title: ff-rdp Daemon Architecture Review
type: architecture-review
subject: daemon
date: 2026-05-23
tags: [ff-rdp, daemon, architecture, review]
---

# ff-rdp Daemon Architecture Review

## 1. What the daemon actually does today

**Process model**: `ff-rdp _daemon` is re-invoked from the same binary via
`process::spawn_daemon` (daemon/process.rs:155-210). On Unix it calls `setsid()` to detach
from the terminal; on Windows it sets `CREATE_NO_WINDOW`. The daemon is a fully separate OS
process, not a thread of the CLI. It publishes its port and PID to `~/.ff-rdp/daemon.json`
(atomic write-then-rename, 0o600 permissions — registry.rs:88-136).

**Discovery**: CLI calls `find_running_daemon` (client.rs:46-70) which reads `daemon.json`,
verifies the PID is alive via `kill(pid, 0)`, and checks `firefox_host`/`firefox_port` match.
There is no version field in `DaemonInfo`. A protocol mismatch (e.g. after a binary upgrade)
is not detected until a message parse fails.

**IPC**: TCP loopback on a random OS-assigned port. Every CLI invocation opens a new
TCP connection to the daemon's proxy port. The first frame must carry `{"auth": "<token>"}`;
the daemon closes the connection immediately on mismatch. No Unix-socket, no shared
memory, no pipe.

**State the daemon keeps**:
- One persistent RDP connection to Firefox (`FramedReader`/`FramedWriter`, split after
  `RdpTransport::connect_raw` — server.rs:202-203).
- A `WatcherActor` subscription to `["network-event", "console-message", "error-message"]`
  established at startup (server.rs:176-178).
- An `EventBuffer`: per-type `VecDeque<Value>` capped at 10,000 events per type, plus
  a `Vec<NavBoundary>` capped at 1,000 entries (buffer.rs:5,51).
- A `RefStore`: `HashMap<String, String>` mapping `e<N>` handles to JS resolver
  expressions; cleared on navigation (server.rs:29-74).
- A `nav_generation: AtomicU64` bumped on each navigation event (server.rs:136,313).
- `rpc_writer`: at most one active RPC client at a time — `Mutex<Option<(RawHandle,
  FramedWriter)>>` (server.rs:100). Only one CLI process can issue Firefox RDP requests
  through the daemon simultaneously.
- `stream_subs: Mutex<Vec<StreamSubscriber>>`: zero or more streaming-only subscribers
  (for `--follow` mode).

**State not kept**:
- The watcher actor ID is fixed at startup; if Firefox restarts the watcher is stale and
  the daemon has no recovery path.
- No actor ID → Front cache; every CLI connection must resolve `root → descriptor →
  target → console/inspector` from scratch (the daemon doesn't cache these).
- No `target-available-form` / `target-destroyed-form` handling; if the tab is replaced
  (e.g. by a cross-process navigation), the daemon's implicit tab reference goes stale
  silently.

---

## 2. Why a daemon at all?

### Connection reuse
Each direct CLI invocation pays: TCP connect, Firefox RDP greeting packet, `listTabs`,
`getTarget`. Empirically ~50–150 ms. The daemon amortises this to zero on subsequent
calls. **Delivering today**: yes, when the daemon is running.

### Persistent watcher subscription — cross-invocation event capture
The key raison d'être: `network-event` and `console-message` events are pushed by Firefox
over the watcher channel continuously. A fresh-per-command CLI can only subscribe, wait, and
unwatch. Cross-invocation capture requires the daemon to buffer events between CLI calls.
**Delivering today**: partially. The daemon _does_ buffer events from its watcher subscription
(server.rs:177, buffer.rs). However `navigate --with-network` does not use the daemon's
persistent subscription; instead it issues its own `stream`/`stop-stream` dance and then
calls `store_network_events` to hand the collected events back (navigate.rs:577, 669-676).
The daemon's own watcher buffer stays empty during this flow because the `stream` call was
clearing it (client.rs:231: "Clears any buffered events for that type so only new events are
received"). The subsequent `network` drain then finds the buffer populated by `store_network_events`
— but only if that path executed without error.

### Avoiding actor cold-start
The daemon resolves `root → descriptor → target` once at startup (server.rs:158-176).
**Partially delivering**: the daemon caches nothing beyond `tab_actor` and
`watcher_actor` (server.rs:158-176). The `console_actor` and `inspector_actor` IDs are
not cached, so every eval round-trip is preceded by another `getTarget` from the CLI
side. `consoleActor` staleness on navigation (a known footgun — lessons-learned.md:
`consoleActor-staleness`) means the daemon must re-`getTarget` after each navigation,
but the CLI is responsible for this, not the daemon.

---

## 3. CLI ↔ daemon protocol

All messages are JSON frames using the same length-prefixed framing as the Firefox RDP
protocol (sharing `RdpTransport`). The "virtual actor" is the string `"daemon"`.

**Client → daemon messages** (client.rs):

| `type` | Purpose | Key fields |
|---|---|---|
| `drain` | Read buffered events | `resourceType`, `sinceNavIndex` |
| `stream` | Start forwarding events live | `resourceType` |
| `stop-stream` | Stop forwarding | `resourceType` |
| `store-events` | Push collected events into buffer | `resourceType`, `events`, `nav_url?` |
| `alloc-refs` | Allocate ref ID range | `count` |
| `register-refs` | Store `e<N>` → resolver mappings | `nav_generation`, `refs` |
| `resolve-ref` | Look up a resolver | `ref_id` |
| `status` | Live stats | — |
| `shutdown` | Graceful shutdown | — |

**No versioning scheme**. There is no `version` or `protocol_version` field in
`DaemonInfo` (registry.rs:29-44) or in the greeting the daemon sends after auth. A
binary upgrade that changes message shapes silently breaks the connection or produces
garbled results until the daemon is manually restarted.

**Schema mismatch handling**: `drain_daemon_events_since` (client.rs:189-226) polls up
to 64 frames before bailing with `"did not receive daemon drain response within 64 frames"`.
The 64-frame cap is arbitrary and will misfire under heavy event bursts. No timeout other
than the socket read timeout protects this loop.

---

## 4. Watcher buffer architecture

`EventBuffer` (buffer.rs) is a `HashMap<String, VecDeque<Value>>`. Each resource type
gets an independent ring buffer (max 10,000 items). Navigation boundaries are tracked
in a parallel `Vec<NavBoundary>` (max 1,000 entries).

**Per-type** (not per-target). There is one global bucket for `"network-event"` regardless
of how many tabs or browsing contexts are active. A multi-tab daemon would mix events from
all tabs into the same bucket. The current codebase only tracks the first tab, so this is
not yet a bug, but it is a structural limitation.

**Indexed by**: insertion order and navigation boundary. `drain_since` (buffer.rs:120-166)
uses the boundary's `start_index` to slice the `VecDeque`. Eviction interacts with this:
when events are evicted from the front of the deque, `network_evicted` is incremented and
the slice calculation compensates (buffer.rs:158-163). This arithmetic is non-trivial and
has unit tests, but was not verified against the session-53 "buffer populated but `network`
returns performance-api" bug.

**The iter-61l bug (AC-C): `network` default ignores daemon buffer**. Root cause traced
through the code:

1. `navigate --with-network` in daemon mode calls `start_daemon_stream` (navigate.rs:577),
   which sets the daemon into streaming mode for `network-event` AND clears the buffer
   (client.rs:231: comment says "Clears any buffered events for that type").
2. After network drain, it calls `store_network_events` (navigate.rs:670) to push
   collected events back into the daemon buffer with a nav boundary.
3. `network` (default, no `--since`) calls `drain_network_from_daemon_since` (network.rs:52)
   with `since_nav` derived from the `--since` CLI flag. The default `--since` value is
   `"-1"` (most recent navigation). The `drain_since` call in the daemon (server.rs) will
   thus look for a navigation boundary from `tabNavigated` events observed by the daemon's
   own `firefox_reader_loop`.

**The gap**: `navigate --with-network` navigates through the daemon's RDP connection,
so `tabNavigated` IS observed by `firefox_reader_loop` and a `NavBoundary` IS recorded
(server.rs:321-331). Then `store_network_events` inserts events with a `nav_url` that
causes the daemon's `handle_store_events` to call `record_nav_boundary` again — resulting
in **two boundaries for the same navigation**. The subsequent `drain_since(-1)` resolves
to the _second_ boundary (the one recorded by `store_network_events`), whose `start_index`
equals the total inserted at that moment. Since `store_network_events` appends events
_after_ recording the boundary, `start_index` points past all the events, and the drain
returns nothing. This is the architectural bug: `store_network_events` calls
`record_nav_boundary` before inserting events, so the boundary's `start_index` is
`total_inserted` at boundary time, not after the events are appended.

---

## 5. `--with-network` engagement path

The daemon subscribes at startup to `["network-event", ...]` (server.rs:177) but does
**not** call `watchTargets("frame")` first. Per the WatcherActor wiki
(rdp/actors/watcher.md): _"A WatcherActor will not see anything until you `watchTargets("frame")` AND `watchResources([...])`"_. The current code only calls `watchResources` (server.rs:177). `WatcherActor::watch_targets` exists in the core library (watcher.rs:60-73) but is never called by the daemon.

Whether this actually causes event loss in practice depends on Firefox version and whether
targets are already established. Firefox may deliver events for already-attached targets
without an explicit `watchTargets` call in some versions, which would explain why events
arrive during `navigate --with-network` (where a stream subscription is set up) but not
at daemon startup (where only `watchResources` is called). If `watchTargets` were missing,
the daemon would receive no events at all — but `daemon status` reports `buffer_sizes: {}`
rather than non-zero counts during idle periods, consistent with the daemon's own buffer
never being populated.

When `navigate --with-network` runs in daemon mode, it bypasses the daemon's buffer
entirely: it sends `stream` (which clears the buffer), then streams events directly to
itself via the forwarding path, then calls `store_network_events` to push them back. This
is a workaround for the daemon buffer being empty (due to missing `watchTargets`). The
non-daemon `navigate --with-network` path (navigate.rs:751-795) correctly calls
`TabActor::get_watcher` → `WatcherActor::watch_resources` → drain → `unwatch_resources`,
all in one connection.

---

## 6. Lifecycle bugs

**Firefox dies**: `firefox_reader_loop` catches the read error (server.rs:350-354), logs
`"Firefox connection lost"`, sets `shutdown = true`. The accept loop then exits and
`remove_registry()` is called (server.rs:234-235). CLI invocations after this will
find the PID dead, remove the stale registry, and fall through to a direct connection.
**This path works correctly.**

**Firefox restarts**: The daemon's watcher actor and tab actor IDs become invalid. The
daemon does not detect this — it has no `target-destroyed-form` handler that would notice
the old top-level target vanish and a new one appear. The `watcher_actor` string stored in
`SharedState.watcher_actor` points to a dead actor on the new Firefox instance's
connection. CLI commands that try to navigate or eval will get `"No such actor"` errors.
**No recovery.**

**Stale daemon after binary upgrade**: As noted above, no version field, no detection.

**`about:neterror` / target switches**: Navigation to `about:neterror` causes a process
switch. The old `WindowGlobalTargetActor` ID is gone; the new target is on the error
page. The daemon records `tabNavigated` (is_navigation_event → true, server.rs:407-411),
clears refs, and records a boundary. But because the CLI's cached `target_actor` and
`console_actor` are stale (held in `TabTarget`, crates/ff-rdp-cli/src/tab_target.rs),
subsequent `eval` fails with `"No such actor"` or a CSP error from about:neterror itself.

**Heavy-SPA navigate daemon timeout (session-53 N2 — "Comparis" bug)**:

The error `"daemon did not respond within the timeout after auth"` originates in
`daemon_rpc` (client.rs:541-543). The `daemon_rpc` path is used by `status` and
`shutdown` — not by the normal `navigate` path. The error message says "after auth",
suggesting the heavy SPA causes the auth handshake phase to time out. The daemon sends a
greeting frame after successful auth (client.rs:513-514). If the daemon is busy forwarding
a large burst of watcher events (from the SPA's dozens of network requests), its
`accept_loop` may be blocked on mutex acquisition while `firefox_reader_loop` holds the
`stream_subs` lock, preventing the greeting from being sent. The auth-greeting delay
appears as a "timeout after auth" to the CLI. No recovery: the tab state is left stale
(no commit recorded, no nav boundary), and subsequent `network` calls return the
pre-navigate buffer contents.

---

## 7. Concurrency model

The daemon uses OS threads (not tokio). The design (server.rs:4-6, 225-232):

- **Main thread**: `accept_loop` — polls the non-blocking `TcpListener`, spawns a handler
  thread per client.
- **Firefox reader thread** (`firefox-reader`): reads from Firefox indefinitely, dispatches
  to `stream_subs` or `rpc_writer` or buffer.
- **Client handler threads**: one per connected CLI client.

Shared state is protected by `Mutex` (four separate mutexes: `buffer`, `rpc_writer`,
`stream_subs`, `last_activity`). There is no `RwLock`, no priority ordering.

**RDP per-actor FIFO**: The daemon does not enforce this. `rpc_writer` is a single writer
that forwards CLI frames to Firefox verbatim. If two CLI clients were both RPC clients
simultaneously, their frames would interleave on the wire. The `rpc_writer` mutex ensures
at most one frame is in-flight at a time to Firefox, but only one CLI client can be the
RPC client at all (the last to connect wins — server.rs:100 comment: "Replaced atomically
when a new client connects").

**Backpressure**: none. `EventBuffer::insert` evicts the oldest event on overflow (buffer.rs:
68-77). A watcher event burst from a heavy SPA drops old events silently. There is no
flow-control signal sent upstream to Firefox to slow event delivery. The WatcherActor's
100 ms throttle (watcher.md: `RESOURCES_THROTTLING_DELAY`, line 65) provides some
natural batching, but under load this is insufficient.

---

## 8. Comparison to Firefox's DevToolsClient

`DevToolsClient` (rdp/client/devtools-client.md) capabilities vs. our daemon:

| Capability | DevToolsClient | ff-rdp daemon |
|---|---|---|
| Per-actor request queue with serialisation | ✓ (pending/active request maps, devtools-client.js:238-293) | ✗ — single `rpc_writer` mutex, no queuing |
| Automatic Front invalidation on `targetDestroyed` | ✓ via `purgeRequests(prefix)` | ✗ — no target-destroyed handler |
| Bulk-packet handling | ✓ — transport-level | ✗ — each frame dispatched individually |
| Reconnect/retry on connection loss | ✗ (DevToolsClient also doesn't reconnect) | ✗ — exits on loss |
| Emit-fanout (multiple subscribers to one resource) | ✓ — Front base class event emitter | partial — `stream_subs` list, but only for watcher events, not arbitrary actors |
| In-flight request cancellation | ✓ — `purgeRequests` rejects pending Promises | ✗ — no cancellation |
| Per-actor Front cache | ✓ — `Pool` / actor registry | ✗ — no front cache |
| Versioned greeting / traits negotiation | ✓ — `mainRoot.connect({frontendVersion})` | ✗ — version not sent or checked |

The most painful gap is **per-actor request queuing**. RDP requires FIFO per actor. When
the CLI sends a sequence of messages through the daemon (e.g. `eval` then `getResponseHeaders`
then `getRequestHeaders`), there is no enforcement that Firefox sees them in order or that
replies are correlated to requests. This works today only because each CLI invocation issues
one logical sequence and then disconnects — the daemon does not multiplex concurrent actors.
It would break immediately if two CLI clients tried to talk to the same actor concurrently.

---

## 9. Failure modes by session

| Session | Bug | Architectural root cause |
|---|---|---|
| 51 #5 | `--with-network` → `network` falls back to perf-api | `watchTargets` missing at daemon startup → daemon buffer empty; `store_network_events` double-boundary bug means drain returns nothing |
| 52 N1 | `--detail --headers` flips source to perf-api | Source-selection logic in network.rs checks `watcher_was_empty` after filtering; `--headers` triggers a code path that fetches headers via `NetworkEventActor`, which re-subscribes and drains a fresh watcher, returning empty; selection then picks perf-api |
| 53 AC-C | `network` default still perf-api | Same as session 51 — double-boundary timing bug unresolved |
| 53 N2 | Heavy-SPA navigate → "daemon did not respond within the timeout after auth" | Daemon blocked on `stream_subs` mutex during event burst; auth-greeting frame delayed past `cli.timeout` |
| 53 AC-F | `navigate bad-dns` → exit 0 | `neterror_error_for_commit` not called in the daemon `--with-network` path; the check was added for non-daemon paths only |
| 53 tab stale after N2 | No recovery after timeout | Daemon has no re-sync mechanism; `SharedState.nav_generation` advances but no CLI state is updated |

---

## 10. Should the daemon exist?

### Option A: Keep as-is, fix bugs
Fix the double-boundary bug, add `watchTargets("frame")` at daemon startup, add a version
field to `DaemonInfo`, and fix the `--headers` source-selection regression. These are
individually small. **Drawback**: the fundamental architecture — no per-actor queue, no
Front cache, no reconnect, no target-lifecycle tracking — means each new protocol feature
will hit another variant of the same class of bugs. The session-51/52/53 cycle of "works
in direct mode, broken in daemon mode" will continue.

### Option B: Rewrite as a proper long-running RDP client + IPC server
Build everything Firefox's DevToolsClient does: per-actor request serialisation, Front
cache with invalidation on `targetDestroyed`, `watchTargets("frame")` before
`watchResources`, reconnect on Firefox restart. The result would be a true daemon that
any CLI invocation can use as a fully stateful Firefox session.
**Drawback**: large investment; the IPC protocol between CLI and daemon gets complex
(essentially reinventing the Front/actor abstraction in a different process).

### Option C: Kill the daemon, embed in a library other tools call
Make `ff-rdp-core` the long-running session object. The CLI becomes one consumer of a
`Session` struct that holds a live RDP connection, watcher subscription, and event buffer.
An LLM agent or TUI embeds the library directly rather than shelling out to `ff-rdp`. The
daemon problem disappears because the session state lives in-process.
**Drawback**: requires a stable public API for `ff-rdp-core`; the CLI would need a
"persistent mode" (socket or stdio) anyway for agent use, which is the IPC problem
restated.

### Verdict: Option B is right, but in phases

The daemon _must exist_: the watcher-subscription buffering use case (persistent
`network-event` capture across CLI invocations) cannot be implemented any other way with
a fork-exec CLI. The pain is not the daemon's existence but its incompleteness as an RDP
client.

The path forward is to evolve the daemon into a proper RDP session manager:
1. Add `watchTargets("frame")` before `watchResources` (single-line fix, highest payoff).
2. Add `DaemonInfo.protocol_version`; CLI rejects or restarts daemon on mismatch.
3. Add `target-available-form` / `target-destroyed-form` handling in `firefox_reader_loop`
   so the daemon can evict stale actor IDs and re-resolve.
4. Add per-actor request serialisation in the accept loop (a `HashMap<ActorId, VecDeque<frame>>` with a dedicated sender thread).

Option C (library embedding) is the long-term correct architecture for agent use, but it
does not replace the daemon for the current CLI-first model.

---

## 11. Concrete proposal for a "fixed daemon"

### Module layout (no new files needed — changes to existing)

**`daemon/server.rs`** additions:

```
SharedState:
  + target_actor: Mutex<String>         // refreshed on target-available-form
  + console_actor: Mutex<String>        // refreshed on target-available-form
  + per_actor_queue: Mutex<HashMap<String, VecDeque<Value>>>  // outbound serialisation
  + protocol_version: u32               // checked against DaemonInfo on connect
```

**`daemon/registry.rs`** additions:
- `DaemonInfo.protocol_version: u32` — bump on any breaking IPC message shape change.

**`daemon/buffer.rs`** fix:
- `record_nav_boundary` called by `firefox_reader_loop` on `tabNavigated` is correct.
- `handle_store_events` must NOT call `record_nav_boundary` again; instead it appends to
  the existing boundary's bucket or inserts events before recording the boundary so
  `start_index` reflects the post-insert count. Concretely: record boundary at
  `total_inserted + events.len()`, insert events, then record the boundary — or accept
  the nav_url only as metadata for the most-recently-recorded boundary.

### Message shape changes

**No new message types needed for the critical fixes.** The `store-events` handler
in `server.rs` needs to record the boundary _after_ inserting events (or not at all,
relying on the `tabNavigated`-triggered boundary that already fired).

### Lifecycle protocol

```
daemon startup:
  1. connect TCP to Firefox
  2. list_tabs → get tab_actor
  3. get_watcher → watcher_actor
  4. watch_targets("frame")          ← MISSING TODAY
  5. watch_resources([...])
  6. start_listeners(console, PageError, ConsoleAPI)
  7. write registry with protocol_version = CURRENT_VERSION

firefox_reader_loop additions:
  - target-available-form → update SharedState.target_actor, console_actor
  - target-destroyed-form → clear target_actor, console_actor, call rpc_client
    with {"from": "daemon", "type": "target-destroyed"} so CLI can surface error

CLI connect:
  - read DaemonInfo.protocol_version
  - if version != EXPECTED_VERSION: kill daemon, spawn new one
```

### Concurrency model (no rewrite needed)

The existing thread-per-client model is adequate for the current use case (one RPC client
at a time). The single change needed: replace the "last writer wins" `rpc_writer` with a
mailbox channel so in-flight frames from the Firefox reader thread are never interleaved
with the RPC client's response. This eliminates the "heavy-SPA auth timeout" class of bug.

Specifically: the Firefox reader thread puts frames into a `mpsc::Sender<Value>` rather
than writing to `rpc_writer` directly. The RPC client handler thread owns the
`mpsc::Receiver<Value>` and drains it alongside the auth handshake, ensuring the greeting
is never blocked by event dispatch.
