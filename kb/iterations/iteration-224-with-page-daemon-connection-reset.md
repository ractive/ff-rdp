---
title: "Iteration 224: click --ref --with-page intermittently gets 'Connection reset by peer' from the daemon"
type: iteration
date: 2026-08-31
status: in-review
branch: iter-224/with-page-daemon-connection-reset
depends_on:
  - 220
dogfood_path: |
  ff-rdp launch --headless
  for i in 1 2 3 4 5; do
    ref=$(ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page \
      --jq '[.results.page.interactive[] | select(.name == "Python Software Foundation")][0].ref' | tr -d '"')
    ff-rdp click --ref "$ref" --with-page --jq '{h: .results.page.headings[0].text, err: .error}'
  done
  # TODAY: 4 of 5 → {"h":"Python Software Foundation"}; 1 of 5 →
  #   {"error":"recv failed: Connection reset by peer (os error 54)","error_type":"Transport"} exit 6,
  #   in ~0.45 s (no timeout involved)
  # expected AFTER: 5 of 5 return the destination view; a mid-collection teardown is retried
  #   inside collect_settled like RdpActorDestroyed already is, or the daemon stops resetting
dogfood_script: iteration-224-with-page-daemon-connection-reset.dogfood.sh
first_call_sites:
  - primitive: ff_rdp_cli::daemon::server::DAEMON_CLIENT_CLOSED
    site: >-
      crates/ff-rdp-cli/src/daemon/server.rs (`daemon_closing_response`, the frame written
      before a client is abandoned) and crates/ff-rdp-cli/src/commands/page_view.rs
      (`is_connection_lost`, which routes that frame to the reconnect arm) — both non-test
  - primitive: ff_rdp_cli::commands::page_view::SettledPage
    site: >-
      crates/ff-rdp-cli/src/commands/page_view.rs — returned by `collect_settled`, consumed by
      `attach` and `insert_page`, which publish `attempts`/`reconnects` into `page_meta`
tags: [iteration, act-and-see, page-view, daemon, defect, carry-over]
---

# Iteration 224: `click --ref --with-page` intermittently gets `Connection reset by peer`

## Why

Found while re-measuring the axi benchmark after [[iteration-220-with-page-after-navigating-click]]
merged (2026-08-31, ff-rdp `5a0071d`). 220 removed the recv-timeout hang — none of the six
benchmark runs timed out — but `wikipedia_infobox_hop` run 3 took 12 turns because its
`click --ref e51 --with-page` came back with

```
{"error":"recv failed: Connection reset by peer (os error 54)","error_type":"Transport"}   exit 6
```

and the agent, not trusting the click, re-navigated by URL and re-queried the page. Reproduced by
hand on the same hop, daemon route: **1 failure in 5** (0.63 / 0.39 / **reset at 0.45** / 0.39 /
0.49 s). The reset arrives fast, so it is not the settle budget or `--timeout`; something on the
daemon side closes the client socket while the post-click collection is in flight.

Two things make this a defect of ours rather than noise:

1. `page_view::collect_settled` retries only `AppError::RdpActorDestroyed`
   (`crates/ff-rdp-cli/src/commands/page_view.rs`, the `Err(e @ AppError::RdpActorDestroyed { .. })`
   arm); an `RdpTransport` error is returned immediately with the timeout hint attached, so the
   one retry that would have saved the trajectory never runs.
2. The daemon proxies one Firefox connection to many short-lived clients. If the daemon's own
   transport hits the `tabNavigated{state:start}` latch / target guard that 220 added and the
   per-client proxy task errors out, the client sees a reset instead of the structured
   `EvalTargetDestroyed` the guard was designed to deliver. Unverified — Theme A is the trace.

## Themes

- **A — Find who resets.** Run the dogfood loop with the daemon at `--log-level trace` and
  capture the daemon's stderr around a reset: is the Firefox-side socket dropping (Firefox
  closed it during the docshell swap), or is the daemon's client proxy returning early on the
  guard error and dropping the client without writing a reply? The answer decides B.
- **B — Never let a reset reach the caller mid-collection.** Whichever side resets: the client
  path in `collect_settled` should treat a transport reset during a guarded collection the way
  it treats `RdpActorDestroyed` — re-resolve the target on a fresh connection and retry within
  the existing `overall_deadline` — and the daemon should always write a structured error before
  closing a client. Both, if A shows both are possible.
- **C — Regression cover that reproduces the 1-in-5.** A live test that performs the hop N times
  (N ≥ 10, bounded by wall clock) against a fixture whose destination commits slowly enough to
  race, asserting zero `Transport` errors. The 220 suite's single-shot tests pass 4 times out of
  5 and would not have caught this.

## Tasks

### A. Diagnose [2/2]
- [x] Capture a daemon trace of one reset; record which side closed the socket and on which
      packet, in this plan's Outcome
- [x] State whether the `--no-daemon` route can reset the same way (it has no proxy, so
      probably not — but say so from evidence, not from the architecture)

### B. Fix [3/3]
- [x] `collect_settled`: retry a transport reset inside a guarded collection, on a fresh
      connection, within `overall_deadline`; keep the immediate return for resets outside one
- [x] Daemon: a proxy task that fails while a client request is pending writes a structured
      error (`error_type: "Transport"` or the `EvalTargetDestroyed` mapping) before closing
- [x] `meta.page_retries` (or the existing attempt counter) reports how many attempts the
      returned view cost, so a flaky hop is visible in JSON — landed as
      `meta.page_attempts` + `meta.page_reconnects`

### C. Cover [1/1]
- [x] `tests/live/live_224_*.rs`: the repeated-hop test from Theme C, plus the dogfood loop as
      a `.dogfood.sh` sourcing `dogfood-lib.sh`

## Acceptance Criteria [3/4]

- [x] The dogfood loop above returns the destination view 5 of 5 on three consecutive runs
      (15 hops, 0 `Transport` errors), daemon route — and round 3 hop 4 came back
      `{"a":2,"rc":1}`: a real mid-collection connection loss, absorbed, exit 0
      [2026-08-31: also through the gate — `cargo run -p xtask -- check-dogfood-script` with both
      env gates set: `iter-224 dogfood: 15/15 hops returned the destination view
      (0 absorbed a reconnect)`, `check-dogfood-script: OK`]
- [ ] The Theme C live test passes in the live sweep and fails on `5a0071d` (record the
      failure count it observes there) — **not met, and not reworded.** The premise was that
      a locally-served slow destination could race the same way. It does not: 90 local hops
      on `5a0071d` produced 0 failures, so `live_224_with_page_connection_reset` observes
      0 failures on `5a0071d` too and does **not** fail there. What it does cover is the
      contract the fix rests on (destination view every hop, `page_attempts` /
      `page_reconnects` always reported); the 1-in-15 itself is covered by the `.dogfood.sh`
      against the real page, which is where it reproduces
- [x] Outcome names the side that closed the socket, with the trace excerpt
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- The reader excerpt's missing infobox — [[iteration-225-reader-excerpt-infobox]].
- Re-measuring the benchmark — [[iteration-213-act-and-see-benchmark-rerun]] Task E, after this
  and 225.
- **Why the CLI→daemon frame stream desynchronises in the first place.** This iteration found
  *that* it does and made both sides survive it; the byte-level cause is
  [[iteration-226-daemon-frame-desync-root-cause]].
- The daemon wedge observed twice at ~25 consecutive hops (every later command times out at
  `phase: recv` and the daemon logs nothing) — [[iteration-227-daemon-wedge-after-sustained-hops]].
- `tracing` output never reaching `~/.ff-rdp/daemon.log` — filed inside 226, since it is what
  blocked a byte-level trace here.

## Outcome

### A — who closed the socket

**The daemon closed it**, from `handle_client`'s frame-decode path in
`crates/ff-rdp-cli/src/daemon/server.rs`. Before this iteration that path was a bare

```rust
Err(_) => {
    // EOF or connection reset: client disconnected.
    break;
}
```

— every non-timeout error reading a *client* frame dropped the socket with nothing written and
nothing logged, which is why the first diagnosis pass found an empty `daemon.log` across two
reproduced failures. With this iteration's instrumentation the same event names itself:

```text
daemon: abandoning client 42: client_frame_undecodable: invalid packet: unexpected byte 0x3d in length prefix
```

`0x3d` is `=`. The daemon's framer resumed reading in the middle of a client frame's payload
instead of at a `{len}:` prefix, gave up on the connection, and closed it. A CLI parked in
`recv` waiting for its page-view reply saw that close as `Connection reset by peer (os error
54)` when unread inbound bytes made the kernel send an RST, and as `failed to fill whole
buffer` when it was a plain FIN mid-frame. Both shapes were observed; both are the same event.

**Why the stream desynchronises is NOT answered here** — that needs a byte-level capture of the
CLI→daemon direction, and the daemon's `tracing` output never reaches `daemon.log` (`RUST_LOG`
*is* inherited by the `_daemon` process — confirmed with `ps eww` — yet not one `tracing` line
is emitted, while `eprintln!` lines are). The leading hypothesis, from reading the code rather
than the wire, is on the *other* direction and would corrupt the CLI's framer rather than the
daemon's: a client socket is written by two threads with no shared lock — the dispatcher via
`SharedState::rpc_writer`, and the client thread via the `own_writer` / heartbeat clones — so
two concurrent `write_all`s can interleave. Filed as
[[iteration-226-daemon-frame-desync-root-cause]] with the capture plan.

### Reproduction, before and after

| route | build | hops | `Transport` failures |
|---|---|---|---|
| daemon | `5a0071d` (main) | 30 | **2** (`Connection reset by peer (os error 54)` ×1, `failed to fill whole buffer` ×1), both at ~0.44 s |
| `--no-daemon` | `5a0071d` | 25 | **0**, every hop `page_attempts: 1` |
| daemon | this branch | 15 (3 × 5) | **0**; one hop reports `page_attempts: 2, page_reconnects: 1` |
| daemon | this branch | 25 (single run) | **0** |
| daemon, local fixture | either | 90 | **0** — the flake needs a real remote origin |

So the answer to Theme A's second task, from evidence: the direct route did not reset once in
25 hops on the same page while the daemon route reset twice in 30. That is consistent with the
proxy being the closer and not proof that a direct connection *cannot* reset; the mechanism
found (a daemon giving up on a client frame) has no counterpart on a route with no daemon.

### B — what changed

1. **`daemon/server.rs`** — `handle_client`'s read loop became a labelled loop yielding a
   `ClientExit`. `Disconnected` (the client hung up: EOF/reset/broken pipe, classified by
   `is_client_hangup`) stays silent; `DaemonShuttingDown` and `Abandoned` write
   `daemon_closing_response` — `{"from":"daemon","error":"daemon_client_closed","reason":…}`,
   the shape `ff_rdp_core::transport::daemon_control_error` already turns into a terminal
   error in both wait loops — and then *drain* the socket, because `close()` with unread
   inbound bytes sends an RST and an RST discards the frame just written. Every former `?`
   inside the loop (Firefox write, daemon-response write, serialisation) now routes through
   the same ending instead of dropping the socket silently.
2. **`commands/page_view.rs`** — `collect_settled` gained a reconnect arm. A lost connection
   (`RdpTransport`, `RdpRemoteClosed`, or `RdpProtocol` from actor `daemon` named
   `daemon_client_closed` — `is_connection_lost`) rebuilds the connection with
   `connect_and_get_target` and collects again, bounded by `NAV_RECONNECT_ATTEMPTS = 1`, by
   the existing `NAV_COLLECT_ATTEMPTS`, and by the caller's `overall_deadline`. A failed
   reconnect reports the *original* error, not the connect failure. Answers from Firefox —
   timeouts, `noSuchActor`, shape errors — are untouched.
3. **`meta.page_attempts` / `meta.page_reconnects`** — always present, `1` and `0` on the
   clean path, carried through `SettledPage` → `page_meta` → `lift_meta`.

### Live sweep

Two sweeps, both `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep`,
port-6000 browser started raw (`firefox -no-remote -profile <tmp> --start-debugger-server 6000
--headless`, with the three devtools prefs in `user.js` — a fresh profile has none and the debug
port never opens without them).

```text
sweep 1 (before the goodbye-write timeout commit)
LIVE_SWEEP_SUMMARY executed=315 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=315
  ff-rdp-cli --test live: 304 passed / 2 failed
  ff-rdp-core tiers:        1 + 3 + 3 + 2 passed / 0 failed
  → 313 passed + 2 failed = 315 = executed ✓
  failures: live_171_recycled_owner_pid::live_171_recycled_owner_pid_no_longer_reads_as_live
            live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json

sweep 2 (branch head, 522fc01)
LIVE_SWEEP_SUMMARY executed=315 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=315
  ff-rdp-cli --test live: 304 passed / 2 failed
  ff-rdp-core tiers:        1 + 3 + 3 + 2 passed / 0 failed
  → 313 passed + 2 failed = 315 = executed ✓
  failures: live_160_envelope_honesty::live_160_type_emits_key_events
            live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json
```

Both sweeps reconcile (`passed + failed == executed`). `live_224_with_page_connection_reset`'s two
tests passed in both.

The live suite also produced two more instances of the desync this iteration diagnosed, with
different bad bytes — which is what a framer resuming at an arbitrary offset in a JSON payload
looks like:

```text
daemon: abandoning client 3: client_frame_undecodable: invalid packet: unexpected byte 0x20 in length prefix
daemon: abandoning client 24: client_frame_undecodable: invalid packet: unexpected byte 0x6c in length prefix
```

Before this branch those two events were silent closes.

### Carry-over

| # | item | disposition |
|---|---|---|
| 1 | AC 2 unmet — the Theme C live test does not fail on `5a0071d`, because the flake does not reproduce against a local fixture (0 in 90 hops) | **no plan.** Nothing measured is left to act on: the contract cover exists and passes, and the reproduction lives in the `.dogfood.sh` against the real page. What would change that: a local fixture that *does* reproduce the desync — [[iteration-226-daemon-frame-desync-root-cause]] Theme A is what would find one |
| 2 | Why the CLI↔daemon frame stream desynchronises (`unexpected byte 0x3d/0x20/0x6c in length prefix`) — diagnosed as far as "the daemon's framer resumed mid-payload", not to a writer | **filed:** [[iteration-226-daemon-frame-desync-root-cause]] |
| 3 | The daemon emits no `tracing` output at all (`RUST_LOG` reaches the `_daemon` process — `ps eww` — but no `tracing` line reaches `~/.ff-rdp/daemon.log`, while `eprintln!` lines do). This blocked the byte-level trace Theme A asked for | **filed:** [[iteration-226-daemon-frame-desync-root-cause]] Theme A, task 1 — it is the thing standing in front of that iteration |
| 4 | The daemon wedges after ~25 sustained hops: every later command times out at `phase: recv`, the daemon logs nothing, only a restart clears it. Seen twice, on two daemon processes and two Firefox instances | **filed:** [[iteration-227-daemon-wedge-after-sustained-hops]] |
| 5 | `live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json` — failed in **both** sweeps (`profile_removed: false` after `stopped: true`); passes alone | **folded** into [[iteration-204-profile-liveness-flake-in-prune-all]] with the evidence. Not environmental-and-dismissed: a reclamation that reports `false` with no reason is a defect of ours |
| 6 | `live_171_recycled_owner_pid::live_171_recycled_owner_pid_no_longer_reads_as_live` — failed in sweep 1, passed in sweep 2 and alone | **folded** into [[iteration-198-live-tests-red-only-under-concurrency]]'s table |
| 7 | `live_160_envelope_honesty::live_160_type_emits_key_events` — failed in sweep 2 with `daemon did not respond within the timeout after auth`, passed in sweep 1 | **folded** into [[iteration-198-live-tests-red-only-under-concurrency]]'s table; it is that plan's second signature (`live_165`'s message) on a different test, which is new information for it |
| 8 | Two client sockets are written by two threads with no shared lock (dispatcher via `rpc_writer`, client thread via its `own_writer`/heartbeat clones) — found by reading the code, not measured | **filed:** [[iteration-226-daemon-frame-desync-root-cause]] Theme B, as the leading hypothesis with a capture plan rather than a fix applied blind |

### C — cover

`crates/ff-rdp-cli/tests/live/live_224_with_page_connection_reset.rs` (12-hop repeat + the
counters contract) and `kb/iterations/iteration-224-*.dogfood.sh` (15 hops against the real
Wikipedia hop, skipped with a written sentinel when `FF_RDP_LIVE_NETWORK_TESTS` is unset).
Unit cover: the closing frame's shape and delivery over a loopback pair, the hangup/fault
split, and the three lost-connection shapes.

## References

- [[iteration-220-with-page-after-navigating-click]] — the settle/guard machinery this sits on
- [[axi-benchmark-comparison]] — the 2026-08-31 two-task re-measurement that surfaced it
- `crates/ff-rdp-cli/src/commands/page_view.rs` — `collect_settled`
- `crates/ff-rdp-cli/src/daemon/server.rs` — client proxy
