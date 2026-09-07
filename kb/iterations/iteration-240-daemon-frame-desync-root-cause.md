---
title: "Iteration 240: the daemon's frame-stream desync and the ~25-hop wedge — root cause, single writer, write deadlines"
type: iteration
date: 2026-08-31
status: in-progress
branch: iter-240/daemon-frame-desync-root-cause
depends_on:
  - 224
dogfood_path: |
  # Same hop as 224, but the assertion is on the daemon log rather than the CLI:
  ff-rdp launch --headless
  for i in $(seq 1 40); do
    ref=$(ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page \
      --jq '[.results.page.interactive[] | select(.name == "Python Software Foundation")][0].ref' | tr -d '"')
    ff-rdp click --ref "$ref" --with-page --jq '.meta.page_reconnects'
  done
  grep 'abandoning client' ~/.ff-rdp/daemon.log
  # TODAY: ~2 in 40 hops log
  #   daemon: abandoning client N: client_frame_undecodable: invalid packet: unexpected byte 0x3d in length prefix
  #   and the CLI silently pays a reconnect (meta.page_reconnects: 1)
  # expected AFTER: 0 lines — the stream does not desynchronise, and page_reconnects stays 0
  # --- from iteration 241 ---
  ff-rdp launch --headless
  for i in $(seq 1 45); do
    ref=$(ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page \
      --jq '[.results.page.interactive[] | select(.name == "Python Software Foundation")][0].ref' | tr -d '"')
    ff-rdp click --ref "$ref" --with-page --jq '.results.page.headings[0].text'
  done
  # TODAY: hops 1..~25 succeed; then one hop returns
  #   warning: navigate: could not refresh target actors: operation timed out
  #   {"error":"operation timed out after 10000ms (phase: recv)", "error_type":"Timeout"}
  # and EVERY later hop times out the same way. The daemon logs nothing at all — no
  # "Firefox connection lost", no panic, no idle timeout. Only a daemon restart clears it.
  # expected AFTER: 45/45 hops succeed, or the daemon fails loudly and recovers.
tags: [iteration, daemon, framing, reliability, defect, carry-over]
dogfood_script: iteration-240-daemon-frame-desync-root-cause.dogfood.sh
---

# Iteration 240: the daemon's frame-stream desync and the ~25-hop wedge — root cause, single writer, write deadlines

> **Renumbered 226 → 240 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 226.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-241-daemon-wedge-after-sustained-hops]] as Part B. One branch, one PR, one carry-over sweep for both parts.

## Part A: the CLI↔daemon frame stream desynchronises — find the byte that starts it

### Why

[[iteration-224-with-page-daemon-connection-reset]] found *that* the daemon gives up on a
client mid-request, and made both sides survive it: the daemon writes a structured
`daemon_client_closed` frame before closing, and `page_view::collect_settled` rebuilds the
connection and collects again. What 224 did **not** find is why the stream desynchronises in
the first place. Its trace excerpt:

```text
daemon: abandoning client 42: client_frame_undecodable: invalid packet: unexpected byte 0x3d in length prefix
```

`0x3d` is `=`. The daemon's framer resumed reading in the middle of a client frame's payload
instead of at a `{len}:` prefix. `encode_frame` uses `str::len()` (bytes, not chars), so the
prefix itself is not the bug — something either wrote a partial frame, or two writers
interleaved on one socket, or a reader consumed bytes it then discarded.

224's fix means this now costs a reconnect (~100 ms) instead of the whole command, so this is
no longer user-visible. It is still a corrupt wire, and a corrupt wire is not a thing to leave
in a proxy.

### Candidates, in the order the evidence favours

1. **Two threads writing one client socket with no shared lock.** A client's socket is written
   by the dispatcher thread through `SharedState::rpc_writer`, and by its own client thread
   through the `own_writer` / heartbeat clones made from `reader.try_clone_stream()`. Nothing
   serialises them, so two concurrent `write_all`s can interleave — certainly for frames large
   enough to be split across kernel writes, which is exactly what a page view is. This
   corrupts the **daemon→CLI** direction, which fits the `failed to fill whole buffer` half of
   224's reproduction but *not* the `0x3d` line, which is the daemon's own reader.
2. **The auth `BufReader` discarding buffered bytes.** `handle_client` reads the auth frame
   through `FramedReader::from_stream(auth_stream)` — a `BufReader` over a clone — and then
   builds a *second* `FramedReader` over the original stream for the loop. Any bytes the first
   one buffered past the auth frame are dropped on the floor. Today the CLI waits for the
   greeting before sending more, so the window is closed by convention rather than by
   construction.
3. **A partial `write_all` on the CLI side.** If a write timeout is ever set on the CLI's
   transport, a `write_all` that fails partway leaves half a frame on the wire and maps to
   `ProtocolError::Timeout`, which `is_transient()` reports as retryable — a retry at any
   layer above would then send the frame again, on top of its own tail.

### Themes

- **A — Capture the bytes.** The blocker in 224 was that the daemon emits no `tracing` output
  at all: `RUST_LOG` *is* inherited by the `_daemon` process (confirmed with `ps eww`), and
  `eprintln!` lines reach `~/.ff-rdp/daemon.log`, but not one `tracing` line does. Fix that
  first — it is a one-line-scale defect standing in front of every daemon diagnosis — then run
  the hop with `ff_rdp_core::transport=trace` and read the frame boundaries either side of the
  desync.
- **B — Serialise per-client writes.** Whatever A finds, one `Arc<Mutex<FramedWriter>>` per
  client — used by the greeting, the daemon responses, the heartbeats, the RPC slot and the
  stream-subscriber list alike — removes candidate 1 by construction rather than by timing.
- **C — Close candidate 2 by construction.** Read the auth frame on the same `FramedReader`
  the loop then uses, so no buffered byte can be discarded.

### Tasks

#### A. Make the daemon traceable [0/2]
- [ ] Find why `tracing` emits nothing in the `_daemon` process and fix it; a `--log-level`
      passed to `daemon` (or inherited `RUST_LOG`) must reach `~/.ff-rdp/daemon.log`
- [ ] Capture one desync with `ff_rdp_core::transport=trace` on both sides; record the frame
      before it and the first bad byte in this plan's Outcome

#### B. Remove the hazards [0/2]
- [ ] One writer per client socket, shared behind a mutex, for every daemon→client write
- [ ] Auth read and loop read share one `FramedReader`

#### C. Cover [0/2]
- [ ] Unit: concurrent writes to one client socket produce a stream that decodes frame-for-frame
- [ ] `tests/live/live_226_*.rs`: the 40-hop loop asserting zero `abandoning client` lines and
      zero `meta.page_reconnects`

### Acceptance Criteria [0/2]

- [ ] 40 daemon hops against the real page log zero `abandoning client` lines and report
      `meta.page_reconnects: 0` on every hop
- [ ] The Outcome names the writer (or reader) that corrupted the stream, with the trace

(The mandatory `cargo fmt && cargo clippy … && cargo test …` criterion is listed once, at the
end of Part B, and covers both parts.)

### Out of scope

- The client-side reconnect from 224 — it stays as defence in depth even once the desync is
  gone.
- The daemon wedge — [[iteration-241-daemon-wedge-after-sustained-hops]] — now Part B of this plan.

### References

- [[iteration-224-with-page-daemon-connection-reset]] — the Outcome this starts from
- `crates/ff-rdp-cli/src/daemon/server.rs` — `handle_client`, `forward_to_rpc_client`
- `crates/ff-rdp-core/src/transport.rs` — `FramedReader` / `FramedWriter`, `encode_frame`

## Part B: the daemon wedges after ~25 sustained hops and never recovers (absorbed from iteration 241)

> **Renumbered 227 → 241 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 227.

### Why

Observed twice while measuring [[iteration-224-with-page-daemon-connection-reset]], on two
different daemon processes and two different Firefox instances:

- run 13 of one 60-hop loop, and run 26 of a 45-hop loop, produced
  `warning: navigate: could not refresh target actors: operation timed out` followed by a
  `phase: recv` timeout on the page-view collection;
- **every subsequent hop** timed out identically, for the remaining 20-plus hops;
- `~/.ff-rdp/daemon.log` recorded **nothing** across the wedge — no `daemon: Firefox
  connection lost`, no panic, no `idle timeout`, and (with 224's instrumentation in place) no
  `abandoning client`.

A daemon that stops answering and says nothing is worse than one that dies: the CLI reports a
generic 10 s timeout, so a caller cannot tell "this page is slow" from "your daemon is gone",
and no retry policy can help. 224 deliberately did not chase it — its reconnect makes a *lost*
connection survivable, but a wedged daemon answers the new connection just as slowly as the
old one.

### Candidates

1. **A blocked write to a dead client wedges the dispatcher.** Client sockets have a read
   timeout (30 s) but no write timeout. `forward_to_rpc_client` writes Firefox replies to the
   RPC-slot client with `send_raw`; if that client stopped reading and its send buffer filled,
   the dispatcher thread blocks in `write_all` forever. Nothing else routes Firefox traffic, so
   every client — including brand-new ones — then waits out its own timeout. This fits the
   observation exactly: silent, total, permanent, and cleared only by a restart.
2. **The RPC slot is never released.** If a client thread is stuck (see 1) its
   `ClientCleanupGuard` never runs, so the slot stays claimed. New clients would then queue and
   eventually get `daemon_busy` — which was *not* observed — so this is a consequence rather
   than the cause.
3. **Firefox's parent process stopped answering `getTarget`.** Would show as the same symptom
   but should also break `--no-daemon`; untested at the time.

### Themes

- **A — Reproduce and instrument.** Drive 60 hops with the daemon traceable (blocked on
  [[iteration-240-daemon-frame-desync-root-cause]] Theme A (Part A of this plan)) and a thread dump / heartbeat log,
  and record where the dispatcher is when the wedge starts.
- **B — Make a stuck client unable to stop the daemon.** A write deadline on every
  daemon→client write, and a dispatcher that drops a client it cannot write to within it
  rather than blocking on it. Pair with 226 Theme B's single per-client writer (Part A of this plan) so the deadline
  has exactly one place to live.
- **C — Say something.** A wedged daemon must be diagnosable from `ff-rdp doctor` and from
  `daemon status`: last dispatched frame, dispatcher liveness, RPC-slot owner and age.

### Tasks

#### A. Diagnose [0/2]
- [ ] Reproduce the wedge with the dispatcher instrumented; record in this plan's Outcome which
      thread is blocked and on what
- [ ] Run the same loop with `--no-daemon` to the same hop count, to separate a Firefox-side
      stall from a daemon-side one

#### B. Fix [0/2]
- [ ] A write deadline on daemon→client writes; a client that misses it is dropped (with the
      `daemon_client_closed` frame from iter-224 where the socket still accepts one)
- [ ] The dispatcher can never block indefinitely on a single client

#### C. Cover [0/2]
- [ ] Unit: a client that never reads does not stop the dispatcher from serving another client
- [ ] `tests/live/live_227_*.rs`: a sustained-hop loop (N ≥ 40) asserting every hop succeeds

### Acceptance Criteria [0/3]

- [ ] 60 consecutive daemon hops against the real page all succeed
- [ ] A deliberately non-reading client is dropped within the deadline and no other client is
      delayed by more than it
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles (covers both parts)

### Out of scope

- The frame desync — [[iteration-240-daemon-frame-desync-root-cause]] — now Part A of this plan.

### References

- [[iteration-224-with-page-daemon-connection-reset]] — where the wedge was observed
- `crates/ff-rdp-cli/src/daemon/server.rs` — `forward_to_rpc_client`, `event_dispatcher_loop`
