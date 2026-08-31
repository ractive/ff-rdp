---
title: "Iteration 227: the daemon wedges after ~25 sustained hops and never recovers"
type: iteration
date: 2026-08-31
status: planned
branch: iter-227/daemon-wedge-after-sustained-hops
depends_on:
  - 224
dogfood_path: |
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
tags: [iteration, daemon, reliability, defect, carry-over]
---

# Iteration 227: the daemon wedges after sustained use

## Why

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

## Candidates

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

## Themes

- **A — Reproduce and instrument.** Drive 60 hops with the daemon traceable (blocked on
  [[iteration-226-daemon-frame-desync-root-cause]] Theme A) and a thread dump / heartbeat log,
  and record where the dispatcher is when the wedge starts.
- **B — Make a stuck client unable to stop the daemon.** A write deadline on every
  daemon→client write, and a dispatcher that drops a client it cannot write to within it
  rather than blocking on it. Pair with 226 Theme B's single per-client writer so the deadline
  has exactly one place to live.
- **C — Say something.** A wedged daemon must be diagnosable from `ff-rdp doctor` and from
  `daemon status`: last dispatched frame, dispatcher liveness, RPC-slot owner and age.

## Tasks

### A. Diagnose [0/2]
- [ ] Reproduce the wedge with the dispatcher instrumented; record in this plan's Outcome which
      thread is blocked and on what
- [ ] Run the same loop with `--no-daemon` to the same hop count, to separate a Firefox-side
      stall from a daemon-side one

### B. Fix [0/2]
- [ ] A write deadline on daemon→client writes; a client that misses it is dropped (with the
      `daemon_client_closed` frame from iter-224 where the socket still accepts one)
- [ ] The dispatcher can never block indefinitely on a single client

### C. Cover [0/2]
- [ ] Unit: a client that never reads does not stop the dispatcher from serving another client
- [ ] `tests/live/live_227_*.rs`: a sustained-hop loop (N ≥ 40) asserting every hop succeeds

## Acceptance Criteria [0/3]

- [ ] 60 consecutive daemon hops against the real page all succeed
- [ ] A deliberately non-reading client is dropped within the deadline and no other client is
      delayed by more than it
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- The frame desync — [[iteration-226-daemon-frame-desync-root-cause]].

## References

- [[iteration-224-with-page-daemon-connection-reset]] — where the wedge was observed
- `crates/ff-rdp-cli/src/daemon/server.rs` — `forward_to_rpc_client`, `event_dispatcher_loop`
