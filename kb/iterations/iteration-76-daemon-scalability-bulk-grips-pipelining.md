---
title: "Iteration 76: Daemon scalability — streaming bulk recv, grip release, per-actor pipelining"
type: iteration
date: 2026-05-24
status: done
branch: iter-76/daemon-scalability-bulk-grips-pipelining
depends_on:
  - iteration-72-transport-polish
  - iteration-73-spec-fidelity-gates
  - iteration-74-protocol-correctness-oneway-events-lifecycle
firefox_refs:
  - lines: 40-56
    path: devtools/shared/transport/transport.js
    why: "Transport contract: typed + bulk packets share one channel; the demux thread must handle both."
  - lines: 138-200
    path: devtools/shared/transport/transport.js
    why: >-
      startBulkSend / BulkPacket framing — the wire shape recv_bulk_with_handler must
      consume in chunks.
  - lines: 490-512
    path: devtools/shared/transport/transport.js
    why: >-
      onBulkPacket streaming-read API — direct precedent for the streaming receiver
      we are adding.
  - lines: 11-22
    path: devtools/shared/specs/heap-snapshot-file.js
    why: >-
      transferHeapSnapshot uses BULK_RESPONSE — primary new consumer of
      recv_bulk_with_handler.
  - lines: 22-35
    path: devtools/shared/specs/screenshot.js
    why: >-
      screenshot.capture returns json today (base64 PNG inside a string); the bulk
      path lets us switch to byte-accurate transfer for the daemon `--bulk` flag.
  - lines: 205-218
    path: devtools/shared/specs/object.js
    why: >-
      objectSpec.release marker — the grip-release method ScopedGrip<Object> must
      invoke on Drop.
  - lines: 780-795
    path: devtools/server/actors/object.js
    why: >-
      release() server-side implementation confirms it is the only way to free the
      actor; protocol.js owns the framing.
  - lines: 40-45
    path: devtools/server/actors/string.js
    why: Long-string actor release path — equivalent for the longStringActor grip.
  - lines: 58-85
    path: devtools/shared/specs/string.js
    why: longstring type marshalling; clarifies which actor IDs need release.
kb_refs:
  - kb/rdp/protocol/transport.md
  - kb/rdp/protocol/message-format.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: ff_rdp_core::transport::Transport::recv_bulk_with_handler
    site: >-
      crates/ff-rdp-cli/src/commands/screenshot.rs (used when --bulk or expected size
      > 4 MiB)
  - primitive: ff_rdp_core::transport::Transport::split
    site: >-
      crates/ff-rdp-cli/src/commands/daemon.rs (daemon mode spawns the demux reader
      thread)
  - primitive: ff_rdp_core::transport::DemuxReader
    site: >-
      crates/ff-rdp-core/src/transport.rs (returned by Transport::split alongside the
      writer half)
  - primitive: ff_rdp_core::actors::object::Grip
    site: >-
      crates/ff-rdp-core/src/actors/dom_walker.rs (issued grips on
      inspector.getNodeActorFromObjectActor)
  - primitive: ff_rdp_core::actors::watcher::ResourceGripGuard
    site: >-
      crates/ff-rdp-core/src/actors/watcher.rs (auto-release for grips returned in
      watched resources)
dogfood_path: |
  # 1. Streaming bulk screenshot — bytes match the base64 path bit-for-bit.
  ff-rdp screenshot --bulk -o /tmp/a.png https://example.com
  ff-rdp screenshot       -o /tmp/b.png https://example.com
  cmp /tmp/a.png /tmp/b.png    # exit 0
  
  # 2. Daemon mode: 1000 console grips, all released after subscribe drop.
  ff-rdp daemon start &
  ff-rdp daemon subscribe console-message --count 1000
  ff-rdp daemon unsubscribe console-message
  ff-rdp daemon inspect --jq '.object_pool.live_grips'   # expect 0
  
  # 3. Two-actor pipelining: overlapping console + walker requests both succeed.
  ff-rdp daemon eval --tail 'for(let i=0;i<50;i++) console.log(i)' &
  ff-rdp daemon walker query 'body > *'                  # returns within budget
tags:
  - iteration
  - daemon
  - performance
  - protocol
---

Three concurrency-shaped issues from the review (F3, L4, C1) all hit
the same code path: the transport assumes one outstanding RPC at a
time, no streaming consumers, and no destructor-driven cleanup. That
worked for the synchronous CLI; the daemon (iter-37/38) papered over
the limits by serializing requests through a mutex. Realistic
workloads (heap snapshots, busy console subscribers, the iter-62 page
map indexer) exceed what serialization can hide.

This is the biggest of the post-review iters — it rewires the daemon
reader, but leaves the synchronous CLI path untouched. Both paths
share `Transport::actor_request`; the difference is whether a demux
reader thread is running.

## Themes

- **A — Streaming bulk receive (F3).** No full-body buffer alloc.
- **B — Grip release on Drop (L4).** Generic `Grip<Kind>` for object
  and long-string actors; auto-release for watched-resource grips.
- **C — Per-actor pipelining (C1).** Daemon-only demux reader fans
  packets into per-actor channels; multiple outstanding requests to
  different actors no longer serialize.

## Tasks

### A. Streaming bulk receive
- [ ] Add `pub fn recv_bulk_with_handler<W: Write>(&mut self, actor: &ActorId, kind: &str, out: &mut W) -> Result<u64>` to `crates/ff-rdp-core/src/transport.rs`. Reads the bulk header (per `devtools/shared/transport/transport.js:138-200`), then copies bytes from the reader into `out` in chunks (8 KiB), enforcing `max_frame_bytes()` from iter-75 theme A. Returns total bytes written.
- [ ] Wire `crates/ff-rdp-cli/src/commands/screenshot.rs`: add `--bulk` flag; when set OR when the expected size (from screenshot args.fullpage × DPR × viewport) exceeds 4 MiB, use the bulk path. Spec note: `screenshot.capture` currently returns base64 JSON; the bulk variant is a daemon-side fast path (see Design notes).
- [ ] Wire `crates/ff-rdp-cli/src/commands/memory.rs` heap-snapshot: invoke `memory.saveHeapSnapshot` (per `devtools/shared/specs/memory.js`) then `heapSnapshotFile.transferHeapSnapshot` (per `devtools/shared/specs/heap-snapshot-file.js:11-22`), routing the BULK_RESPONSE through `recv_bulk_with_handler` into a file writer.
- [ ] Update `kb/rdp/protocol/transport.md`: remove the line saying ff-rdp doesn't consume bulk packets.

### B. Grip release on Drop
- [ ] Generalise `ScopedGrip` (currently in `crates/ff-rdp-core/src/actors/console.rs`) to `pub struct Grip<K: GripKind>` parameterised by a marker trait (`ObjectGrip`, `LongStringGrip`).
- [ ] `impl Drop for Grip<K>`: enqueue an `actor_send` of the actor's release method (`object` per `devtools/shared/specs/object.js:213`, long-string per `devtools/server/actors/string.js:40-50`) onto a transport-shared release queue; queue is drained by the demux reader thread (theme C) in daemon mode and by the next `actor_request` call in synchronous mode.
- [ ] Extend issuing sites to wrap returned grip actor IDs:
  - DOM walker: `crates/ff-rdp-core/src/actors/dom_walker.rs` — every `disconnectedNode`/`nodeActor` returned by walker methods (per `devtools/shared/specs/walker.js`).
  - Watcher resources: `crates/ff-rdp-core/src/actors/watcher.rs` — grip actor IDs embedded in `consoleAPICall` / `evaluationResult` packets.
- [ ] Add `pub struct ResourceGripGuard` over the resource-bus subscription that owns its grips; dropping the guard drops the grips, releasing them.
- [ ] Tests: `grip_drop_enqueues_release`, `resource_grip_guard_releases_all`.

### C. Per-actor pipelining (daemon)
- [ ] Add `pub fn split(self) -> (DemuxReader, TransportWriter)` to `crates/ff-rdp-core/src/transport.rs`. `DemuxReader` owns the read half + a `HashMap<ActorId, mpsc::Sender<Packet>>`; spawns a reader thread that demuxes packets by `from:` into the per-actor channels. Unknown actors fall back to the event/resource sink (consistent with iter-74's no-drop invariant).
- [ ] Per-actor channel is bounded (default 64); back-pressure surfaces as `RdpError::ActorChannelFull` rather than unbounded memory growth.
- [ ] In daemon mode (`crates/ff-rdp-cli/src/commands/daemon.rs`), replace the existing single-reader mutex with `Transport::split`. `actor_request` in daemon mode looks up (or registers) the per-actor channel and recv's the head; per-actor FIFO is preserved per `kb/rdp/protocol/message-format.md:84-91`.
- [ ] Synchronous CLI path: unchanged. `Transport::actor_request` keeps its current `send → recv_reply_from` shape; the demux thread is only spawned when `split()` is called.

## Acceptance Criteria [0/9]

- [ ] `live_bulk_screenshot_smaller_than_base64`: `crates/ff-rdp-cli/tests/live_bulk_screenshot.rs::live_bulk_screenshot_smaller_than_base64` — PNG produced by `--bulk` matches the base64-path PNG bytewise (`cmp` exit 0) and peak memory is < 1.3× output size (vs ~4× for base64). Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `live_heap_snapshot_streams_to_disk`: `crates/ff-rdp-cli/tests/live_heap_snapshot.rs::live_heap_snapshot_streams_to_disk` — 50 MiB snapshot lands on disk; resident-set-size delta during transfer < 16 MiB. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `recv_bulk_with_handler_chunked`: `crates/ff-rdp-core/src/transport.rs::recv_bulk_with_handler_chunked` — unit test with mock reader confirms 8 KiB-chunk copies, no full-body alloc.
- [ ] `grip_drop_enqueues_release`: `crates/ff-rdp-core/src/actors/object.rs::grip_drop_enqueues_release` — dropping a `Grip<ObjectGrip>` adds a release entry to the transport queue with the correct actor ID and method name.
- [ ] `live_grip_release_no_leak`: `crates/ff-rdp-cli/tests/live_grip_release.rs::live_grip_release_no_leak` — capture 1000 console grips, drop the subscriber, assert Firefox's object-pool size returns to baseline (queried via watcher inspection). Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `demux_reader_per_actor_fifo`: `crates/ff-rdp-core/src/transport.rs::demux_reader_per_actor_fifo` — unit test, packets from actor A and B interleaved on the wire arrive in per-actor FIFO order on the channels.
- [ ] `demux_reader_unknown_actor_to_sink`: same test file — packet from an unregistered actor reaches the fallback sink.
- [ ] `live_pipeline_two_actors`: `crates/ff-rdp-cli/tests/live_pipeline.rs::live_pipeline_two_actors` — issue overlapping requests against console + walker via daemon mode; both replies arrive within 2× single-request latency (not the 2× serialised latency the mutex enforces today). Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The bulk screenshot path is a *transport-level* optimisation, not a
spec change: Firefox's `screenshot.capture` still returns json, but
the daemon-side `--bulk` flag swaps the carrier for the daemon's own
local consumers (HTTP API streaming the PNG to a downstream client).
For the synchronous CLI, base64 stays the default because it's one
syscall fewer; the dogfood `cmp` AC pins the byte-equivalence.

`Grip<K>` is parameterised rather than two concrete types because the
release-queue plumbing is identical; only the method name and the
actor-name pattern differ. The release queue lets us avoid sending
packets from a destructor (the same reasoning as iter-71's `gc()`).

The per-actor pipelining only changes the *demux*, not request
generation: callers still see a synchronous `actor_request`. Firefox
guarantees per-actor reply ordering (cited above), so a single
`mpsc::Sender` per actor with FIFO drain is sufficient; we do not
need a request-ID echo. A future async-front would need its own
layer on top — out of scope.

The release queue + per-actor channels together close
`kb/rdp/from-our-codebase/open-gaps.md:36` (`actor-leak-in-daemon`).

## Out of scope

- An async public API. The demux reader is a daemon-internal
  implementation detail; the public `Transport` interface stays
  synchronous.
- WebSocket transport. TCP-on-localhost only, per Firefox.
- Spec-level changes to screenshot.capture. The bulk variant is a
  daemon transport choice; spec correctness work is iter-77 S1.

## References

- [[iteration-72-transport-polish]]
- [[iteration-73-spec-fidelity-gates]]
- [[iteration-74-protocol-correctness-oneway-events-lifecycle]]
- Protocol review report (2026-05-24): F3, L4, C1
- `kb/rdp/protocol/transport.md`
- `kb/rdp/protocol/message-format.md` §"Request / reply pairing"
- `kb/rdp/from-our-codebase/open-gaps.md` (`actor-leak-in-daemon`)
