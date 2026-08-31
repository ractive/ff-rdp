---
title: "Iteration 226: the CLI↔daemon frame stream desynchronises — find the byte that starts it"
type: iteration
date: 2026-08-31
status: planned
branch: iter-226/daemon-frame-desync-root-cause
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
tags: [iteration, daemon, framing, defect, carry-over]
---

# Iteration 226: the CLI↔daemon frame stream desynchronises

## Why

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

## Candidates, in the order the evidence favours

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

## Themes

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

## Tasks

### A. Make the daemon traceable [0/2]
- [ ] Find why `tracing` emits nothing in the `_daemon` process and fix it; a `--log-level`
      passed to `daemon` (or inherited `RUST_LOG`) must reach `~/.ff-rdp/daemon.log`
- [ ] Capture one desync with `ff_rdp_core::transport=trace` on both sides; record the frame
      before it and the first bad byte in this plan's Outcome

### B. Remove the hazards [0/2]
- [ ] One writer per client socket, shared behind a mutex, for every daemon→client write
- [ ] Auth read and loop read share one `FramedReader`

### C. Cover [0/2]
- [ ] Unit: concurrent writes to one client socket produce a stream that decodes frame-for-frame
- [ ] `tests/live/live_226_*.rs`: the 40-hop loop asserting zero `abandoning client` lines and
      zero `meta.page_reconnects`

## Acceptance Criteria [0/3]

- [ ] 40 daemon hops against the real page log zero `abandoning client` lines and report
      `meta.page_reconnects: 0` on every hop
- [ ] The Outcome names the writer (or reader) that corrupted the stream, with the trace
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- The client-side reconnect from 224 — it stays as defence in depth even once the desync is
  gone.
- The daemon wedge — [[iteration-227-daemon-wedge-after-sustained-hops]].

## References

- [[iteration-224-with-page-daemon-connection-reset]] — the Outcome this starts from
- `crates/ff-rdp-cli/src/daemon/server.rs` — `handle_client`, `forward_to_rpc_client`
- `crates/ff-rdp-core/src/transport.rs` — `FramedReader` / `FramedWriter`, `encode_frame`
