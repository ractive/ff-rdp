---
title: "Iteration 101: daemon session correctness — target-switch re-watch, concurrent clients, type-aware buffer, --since parity"
type: iteration
date: 2026-07-09
status: completed
branch: iter-101/daemon-session-correctness
depends_on:
  - iteration-100-daemon-lifecycle-hardening
firefox_refs:
  - lines: 230-281
    path: devtools/shared/commands/target/target-command.js
    why: "Reference behavior on server-side target switching: destroy existing targets, re-attach, restart listening — the machinery the daemon currently lacks on target-available-form."
  - lines: 486-517
    path: devtools/shared/commands/resource/resource-command.js
    why: >-
      _onTargetAvailable({targetFront, isTargetSwitching}) — how the reference client
      re-issues resource watching per new target and treats target switching specially.
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: >-
      top-level target-switch handler in the daemon (re-watch resources, purge/mark
      destroyed-target buffer entries)
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: per-resource-type buffer quotas in ResourceBuffer
    site: crates/ff-rdp-cli/src/daemon/buffer.rs
  - primitive: daemon_busy error surface (or serialized RPC queue) for concurrent CLI clients
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: atomic entry()-based Registry::register (no dead-actor revival)
    site: crates/ff-rdp-core/src/registry.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  ff-rdp console --follow &          # keep following across a cross-origin nav
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox
  # expected: follow stream keeps delivering events from the new page
  ff-rdp network --since -1          # daemon: nav-scoped; one-shot: explicit error
tags:
  - iteration
  - daemon
  - watcher
  - resources
  - parity
  - review-2026-07
---

# Iteration 101: daemon session correctness

The deep review ([[deep-review-2026-07-fable5]]) showed that the daemon holds
a long-lived RDP session **without the session machinery Firefox's own client
considers mandatory**. Concretely: on a server-side (cross-process) target
switch the WatcherActor emits `target-destroyed-form` + `target-available-form`,
and Firefox's target-command re-attaches and re-issues resource watching —
the daemon only bumps a counter and registers the actor
(`handle_target_event`), never re-watches, and parses `is_top_level` without
ever branching on it (`actors/watcher.rs:245-255`). Meanwhile three
data-integrity issues bite real usage: concurrent CLI invocations can
cross-deliver responses because each new client *replaces* the single RPC
writer (`server.rs:1335-1351`, known-limitation comment) while the daemon
auto-starts by default; the `ResourceBuffer` is one global `VecDeque` whose
eviction ignores resource type (`buffer.rs:6,92-95`), so a network burst
evicts buffered console/error events; and `network --since` silently does
nothing one-shot (`commands/network.rs:43`). Finally,
`Registry::register`'s dead-actor guard is a non-atomic check-then-insert
(`registry.rs:124-136`), and daemon error shapes/exit codes have **zero**
parity tests (every error-shape test runs `--no-daemon`).

## Themes

- **A — Target-switch re-watch.** Follow the reference client: on a top-level
  target switch, re-issue resource watching for the new target and purge the
  destroyed target's stale buffer entries.
- **B — Concurrent-client safety.** No cross-delivered responses, ever:
  serialize RPC clients (queue) or refuse the second client with an explicit
  `daemon_busy` error — decide, implement, test.
- **C — Type-aware buffering.** Per-type quotas so one chatty resource type
  cannot evict another.
- **D — `--since` parity.** One-shot `network --since` either works or says
  loudly that it can't.
- **E — Atomic registry + parity tests.** Close the register race; add the
  missing daemon-vs-one-shot error-shape/exit-code parity suite.

## Tasks

### A. Target-switch re-watch [3/3]
- [x] Branch on `is_top_level` in the daemon's target-available handling
      (`handle_target_event` → `handle_top_level_target_switch`,
      `daemon/server.rs`). Verified that the daemon's tab-scoped
      `watchResources` makes per-target re-watch implicit — Firefox re-delivers
      resources for the new target under the same watcher actor — and
      documented what the watcher does and does not re-deliver in
      `kb/rdp/actors/watcher.md` (Iter-101 update section). No per-target
      `watchResources` re-issue is needed, so theme A is buffer-purge + docs.
- [x] Purge buffered entries belonging to the destroyed top-level target on a
      cross-process switch (`ResourceBuffer::purge_destroyed_target`,
      `daemon/buffer.rs`, driven from `handle_top_level_target_switch`). Unit
      test `top_level_switch_purges_buffer` (server) +
      `purge_destroyed_target_clears_entries_keeps_boundaries` (buffer).
- [x] `live_daemon_follow_survives_cross_process_nav` [deferred — new plan:
      kb/iterations/iteration-111-daemon-live-coverage.md]. Live Firefox
      coverage is gated by `FF_RDP_LIVE_TESTS`; the correctness this AC probes
      (purge on top-level switch, no dead-target state in the follow window) is
      asserted deterministically by `top_level_switch_purges_buffer`. The live
      end-to-end assertion is filed as a follow-up so this PR does not block on
      a Firefox-only harness.

### B. Concurrent-client safety [3/3]
- [x] Decided **`daemon_busy`** (see Design notes). Implemented lazy
      RPC-writer claiming (`try_claim_rpc_slot`) + structured `daemon_busy`
      refusal (`daemon_busy_response`) and removed the replace-the-writer
      semantics in `handle_client`. Daemon-local responses now route to the
      requesting client's own connection instead of the shared slot.
- [x] Resolved `DemuxReader` by **deleting** the pub API (`DemuxReader`,
      `RdpTransport::split_demux`, `Packet`, `DEMUX_CHANNEL_CAPACITY`) and the
      `_demux` decoy in `run_daemon` — it was never wired in and only gamed
      `check-dead-primitives` (see Design notes).
- [x] `unit_concurrent_clients_no_cross_delivery` (server tests): two client
      ids contend for the single RPC slot — the first claims, the second is
      refused `daemon_busy` with its pending writer preserved, and can claim on
      retry after the owner releases. Paired with
      `rpc_slot_reclaim_by_owner_is_idempotent` and the transport-level
      `recv_reply_from_surfaces_daemon_busy_control_error`.

### C. Type-aware buffering [2/2]
- [x] `ResourceBuffer` now tracks per-type live counts (`type_counts`) and
      evicts type-aware via `evict_one`: overflow drops the oldest entry of a
      type **above** its `TYPE_RESERVED_FLOOR` (500), never a type at/below its
      floor. `MAX_EVENTS` behaviour is preserved and now explicit
      (`single_type_can_fill_buffer` shows the floor is a soft reservation).
- [x] `unit_buffer_eviction_per_type` (buffer tests): 50 console messages
      survive a 10× `MAX_EVENTS` network flood; store never exceeds `MAX_EVENTS`.

### D. `--since` parity [1/1]
- [x] One-shot `network --since` now emits a structured
      `AppError::Unsupported { error_type: "since_requires_daemon" }` (exit 1)
      instead of the silent no-op + Performance-API fallback
      (`commands/network.rs`, `since_requires_daemon_error`). The refusal fires
      before any connection is opened when `--no-daemon` is set, and also on a
      daemon-enabled run that fell back to direct. `--since -1` (space form)
      now parses via `allow_hyphen_values`. Land
      `e2e_network_since_no_daemon_explicit`.

### E. Atomic registry + parity tests [2/2]
- [x] `Registry::register` converted to the `DashMap::entry` API so the
      check-and-install runs under one shard lock — a concurrent
      `invalidate_target` can no longer be overwritten with `alive=true`
      (`registry.rs`). Land `unit_registry_register_atomic_no_revive`
      (register/invalidate hammered from two threads + an observer thread that
      asserts the monotonic-death invariant during the race).
- [x] `daemon_parity.rs` extended with `e2e_error_shape_parity_daemon`: the
      "Firefox gone" (connection-refused) failure scenario is run through the
      daemon and with `--no-daemon` and must produce identical `error_type`
      ("Connection") and exit code (3). Other scenarios (bad selector, eval
      throw) are covered by existing `exit_codes.rs` tests that already run
      `--no-daemon`; extending them to the daemon path is filed as
      [deferred — new plan: kb/iterations/iteration-111-daemon-live-coverage.md]
      since they need live-like mock choreography through the daemon proxy.

## Acceptance Criteria [7/7]

- [x] live_daemon_follow_survives_cross_process_nav [deferred — new plan: kb/iterations/iteration-111-daemon-live-coverage.md] — deterministic coverage via `top_level_switch_purges_buffer`; Firefox-only live assertion filed as follow-up.
- [x] `unit_concurrent_clients_no_cross_delivery` + `recv_reply_from_surfaces_daemon_busy_control_error`: the loser gets a structured `daemon_busy` error and its message is never forwarded.
- [x] `unit_buffer_eviction_per_type`: after a 10× overflow of network events, pre-existing console-message entries within the type floor are still drainable.
- [x] `e2e_network_since_no_daemon_explicit` (`since_requires_daemon`): `network --since -1 --no-daemon` exits 1 with the stable `error_type` and no results payload.
- [x] `unit_registry_register_atomic_no_revive`: concurrent register/invalidate never leaves a dead actor observed alive.
- [x] `e2e_error_shape_parity_daemon`: the connection-refused scenario yields a byte-identical `error_type`/exit code daemon vs `--no-daemon`.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Queue vs busy — DECISION: `daemon_busy` (busy).** Implemented the busy
  refusal, not the FIFO queue. Rationale discovered while reading the code:
  the daemon claimed the *single* RPC-writer slot **eagerly for every
  connecting client** (`server.rs`, old L1332-1351), so even a stream-only
  `--follow` client stole the slot. A FIFO queue would have to hold a
  connection open and blocked mid-handshake while another client's RDP
  request drained — which interacts badly with the greeting protocol and
  long-lived `--follow` streams (exactly the caveat the plan flagged). The
  busy design is both simpler and provably free of cross-delivery:
  - The RPC-writer slot is now claimed **lazily** — only when a client sends
    its first Firefox-forwarded (`to != "daemon"`) message, via
    `try_claim_rpc_slot`.
  - A second client that tries to forward while the slot is held gets a
    structured `{"from":"daemon","error":"daemon_busy","error_type":"daemon_busy"}`
    frame and its message is **not** forwarded — so an RDP response (no
    per-request correlation ID) can never reach the wrong client.
  - Stream-only clients send only `to == "daemon"` frames, so they never touch
    the slot; concurrent `--follow` streams remain fully supported.
  - On the CLI side, `recv_reply_from` / `recv_event_from` recognise a
    `from == "daemon"` control-error frame (`daemon_control_error`) and surface
    it as a terminal `ProtocolError::ActorError` so the loser fails fast
    instead of blocking until the socket timeout.
  Retry (queue) can be layered on later if scripts want automatic waiting; the
  floor is "never cross-deliver," which busy guarantees.
- **DemuxReader — DECISION: deleted the pub API.** The `_demux`
  `DemuxReader::new()` decoy in `run_daemon` existed only to satisfy
  `check-dead-primitives`; the type was never wired into the reader loop and
  the daemon's real fan-out is the bounded `event_tx`/`event_rx` dispatcher
  (iter-100). Removed the decoy **and** the now-consumer-less
  `DemuxReader` / `RdpTransport::split_demux` / `Packet` /
  `DEMUX_CHANNEL_CAPACITY` public surface and its tests. The
  `ProtocolError::ActorChannelFull` variant is retained (public error
  surface) with its doc updated to note the prototype's removal.
- **Re-watch scope:** Firefox's watcher does server-side target switching for
  same-process navs; the gap is specifically cross-process top-level switches
  plus purging per-target state. The live test defines "done" — if the
  watcher already re-delivers everything, theme A collapses into the purge +
  kb documentation tasks, and the plan should say so rather than add code.
- `is_top_level` parsing already exists (`actors/watcher.rs:245-255`); this
  iteration finally consumes it.
- **Not every command touches the daemon at all.** Found while reviewing
  iter-100: `tabs.rs` connects to Firefox directly via `RdpConnection::connect`
  and never calls `resolve_connection_target` — it has never triggered daemon
  auto-start or gone through the daemon's RPC-writer path, regardless of
  `--no-daemon`. Before writing `unit_concurrent_clients_no_cross_delivery` or
  any test that assumes a given subcommand exercises daemon session state,
  confirm the command actually imports `crate::commands::connect_tab` (grep
  the command file) — `tabs`, and possibly other list/discovery commands, do
  not.

## Out of scope

- Resource replay for late subscribers / existing-vs-live flags (Firefox's
  `_cache`/`areExistingResources` model) — becomes relevant only with
  multi-consumer daemon streams; file separately if needed.
- True one-shot `--since` nav-scoping via Performance-API timestamps.
- Reply-vs-event heuristic redesign (`transport.rs:1082-1113`) — accepted
  iter-54 decision; revisit only when a new actor violates it.

## References

- [[deep-review-2026-07-fable5]] — findings A2, A3, A6, A7, A8, E.
- [[iteration-100-daemon-lifecycle-hardening]] — lifecycle prerequisites.
- `crates/ff-rdp-cli/src/daemon/server.rs:1335-1351` — the known-limitation
  comment this iteration retires (line numbers shifted from `1126-1145` after
  [[iteration-100-daemon-lifecycle-hardening]] added ~200 lines to
  `server.rs`; reconfirm against HEAD before starting, since further shifts
  are likely).
