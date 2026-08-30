---
title: "Iteration 224: click --ref --with-page intermittently gets 'Connection reset by peer' from the daemon"
type: iteration
date: 2026-08-31
status: planned
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

### A. Diagnose [0/2]
- [ ] Capture a daemon trace of one reset; record which side closed the socket and on which
      packet, in this plan's Outcome
- [ ] State whether the `--no-daemon` route can reset the same way (it has no proxy, so
      probably not — but say so from evidence, not from the architecture)

### B. Fix [0/3]
- [ ] `collect_settled`: retry a transport reset inside a guarded collection, on a fresh
      connection, within `overall_deadline`; keep the immediate return for resets outside one
- [ ] Daemon: a proxy task that fails while a client request is pending writes a structured
      error (`error_type: "Transport"` or the `EvalTargetDestroyed` mapping) before closing
- [ ] `meta.page_retries` (or the existing attempt counter) reports how many attempts the
      returned view cost, so a flaky hop is visible in JSON

### C. Cover [0/1]
- [ ] `tests/live/live_224_*.rs`: the repeated-hop test from Theme C, plus the dogfood loop as
      a `.dogfood.sh` sourcing `dogfood-lib.sh`

## Acceptance Criteria [0/4]

- [ ] The dogfood loop above returns the destination view 5 of 5 on three consecutive runs
      (15 hops, 0 `Transport` errors), daemon route
- [ ] The Theme C live test passes in the live sweep and fails on `5a0071d` (record the
      failure count it observes there)
- [ ] Outcome names the side that closed the socket, with the trace excerpt
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- The reader excerpt's missing infobox — [[iteration-225-reader-excerpt-infobox]].
- Re-measuring the benchmark — [[iteration-213-act-and-see-benchmark-rerun]] Task E, after this
  and 225.

## References

- [[iteration-220-with-page-after-navigating-click]] — the settle/guard machinery this sits on
- [[axi-benchmark-comparison]] — the 2026-08-31 two-task re-measurement that surfaced it
- `crates/ff-rdp-cli/src/commands/page_view.rs` — `collect_settled`
- `crates/ff-rdp-cli/src/daemon/server.rs` — client proxy
