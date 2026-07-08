---
title: "Iteration 101: daemon session correctness — target-switch re-watch, concurrent clients, type-aware buffer, --since parity"
type: iteration
date: 2026-07-09
status: planned
branch: iter-101/daemon-session-correctness
depends_on:
  - iteration-100-daemon-lifecycle-hardening
firefox_refs:
  - lines: 230-281
    path: devtools/shared/commands/target/target-command.js
    why: >-
      Reference behavior on server-side target switching: destroy existing
      targets, re-attach, restart listening — the machinery the daemon
      currently lacks on target-available-form.
  - lines: 486-517
    path: devtools/shared/commands/resource/resource-command.js
    why: >-
      _onTargetAvailable({targetFront, isTargetSwitching}) — how the reference
      client re-issues resource watching per new target and treats target
      switching specially.
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: >-
      top-level target-switch handler in the daemon (re-watch resources,
      purge/mark destroyed-target buffer entries)
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: per-resource-type buffer quotas in ResourceBuffer
    site: crates/ff-rdp-cli/src/daemon/buffer.rs
  - primitive: >-
      daemon_busy error surface (or serialized RPC queue) for concurrent
      CLI clients
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
tags: [iteration, daemon, watcher, resources, parity, review-2026-07]
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
writer (`server.rs:1126-1145`, known-limitation comment) while the daemon
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

### A. Target-switch re-watch [0/3]
- [ ] Branch on `is_top_level` in the daemon's target-available handling:
      re-issue `watchResources` for the daemon's active resource set against
      the new target where required, mirroring
      `resource-command.js:486-517` semantics (server-side watching makes
      most re-watch implicit — verify against a live cross-origin nav and
      document what the watcher does and does not re-deliver in
      `kb/rdp/actors/watcher.md`).
- [ ] Purge (or generation-mark) buffered entries belonging to the destroyed
      target so drains never mix dead-target state into post-nav windows.
- [ ] Land `live_daemon_follow_survives_cross_process_nav` (console --follow
      keeps delivering across an example.com → wikipedia.org navigation).

### B. Concurrent-client safety [0/3]
- [ ] Decide queue-vs-busy in a short design note in this plan (see Design
      notes below for the starting position), then implement: either a FIFO
      of RPC clients (one in-flight at a time, others wait with a bounded
      timeout) or an immediate structured `daemon_busy` error with retry
      hint. Remove the replace-the-writer semantics at `server.rs:1126-1145`.
- [ ] Resolve the `DemuxReader` question honestly: wire `split_demux`
      (`transport.rs:887-1056`) into the daemon as iter-77 intended, or
      delete the pub API — and in either case remove the `_demux` decoy at
      `server.rs:331-335` that games `check-dead-primitives`.
- [ ] Land `unit_concurrent_clients_no_cross_delivery` (two mock clients
      issue requests concurrently; each receives only its own reply, or the
      second receives `daemon_busy`).

### C. Type-aware buffering [0/2]
- [ ] Give `ResourceBuffer` per-type quotas (or a reserved floor per type)
      so eviction of type X never removes the last entries of type Y
      (`buffer.rs:6,92-95`); make `MAX_EVENTS`/`MAX_BOUNDARIES` behavior
      explicit and tested.
- [ ] Land `unit_buffer_eviction_per_type` (flood N network events; earlier
      console-message/error-message entries below their floor survive).

### D. `--since` parity [0/1]
- [ ] One-shot `network --since` (`commands/network.rs:43`): emit an explicit
      structured error (`error_type: "since_requires_daemon"`) instead of the
      current silent no-op + Performance-API fallback. (Implementing true
      one-shot nav-scoping is out of scope — the honest error is the floor.)
      Land `e2e_network_since_no_daemon_explicit`.

### E. Atomic registry + parity tests [0/2]
- [ ] Convert `Registry::register`'s check-then-insert to the `DashMap`
      entry API so a concurrent `invalidate_target` cannot be overwritten
      with `alive=true` (`registry.rs:124-136`); land
      `unit_registry_register_atomic_no_revive` (hammer register/invalidate
      from two threads; a dead actor is never observed alive afterwards).
- [ ] Extend `daemon_parity.rs` with an error-shape/exit-code parity suite:
      the same failing scenarios (bad selector, unknown tab, eval throw,
      Firefox gone) run with and without a daemon and must produce identical
      `error_type` and exit codes; land `e2e_error_shape_parity_daemon`.

## Acceptance Criteria [0/7]

- [ ] live_daemon_follow_survives_cross_process_nav: events from the
      post-navigation page appear in the still-running `--follow` stream.
- [ ] unit_concurrent_clients_no_cross_delivery: no client ever receives a
      reply to another client's request (queue) or the loser gets a
      structured `daemon_busy` error (busy) — whichever design lands.
- [ ] unit_buffer_eviction_per_type: after a 10× overflow of network events,
      pre-existing console-message entries within the type floor are still
      drainable.
- [ ] e2e_network_since_no_daemon_explicit: `network --since -1 --no-daemon`
      exits non-zero with `error_type: "since_requires_daemon"` (no silent
      unfiltered output).
- [ ] unit_registry_register_atomic_no_revive: concurrent
      register/invalidate never leaves a dead actor marked alive.
- [ ] e2e_error_shape_parity_daemon: for each covered failure scenario,
      `error_type` and exit code are byte-identical daemon vs `--no-daemon`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Queue vs busy:** starting position is the bounded FIFO queue — parallel
  `ff-rdp` calls in scripts are the very pattern that triggers the bug, and
  making them "just work" beats making them fail politely. If the queue turns
  out to interact badly with `--follow` streams, fall back to `daemon_busy`
  for RPC while keeping streams multi-subscriber (they already are).
- **Re-watch scope:** Firefox's watcher does server-side target switching for
  same-process navs; the gap is specifically cross-process top-level switches
  plus purging per-target state. The live test defines "done" — if the
  watcher already re-delivers everything, theme A collapses into the purge +
  kb documentation tasks, and the plan should say so rather than add code.
- `is_top_level` parsing already exists (`actors/watcher.rs:245-255`); this
  iteration finally consumes it.

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
- `crates/ff-rdp-cli/src/daemon/server.rs:1126-1145` — the known-limitation
  comment this iteration retires.
