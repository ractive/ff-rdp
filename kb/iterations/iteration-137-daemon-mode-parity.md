---
branch: iter-137/daemon-mode-parity
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-129-consent-and-cross-origin-frames.md
  - kb/iterations/iteration-136-core-live-test-repairs.md
dogfood_path: |
  # iteration-129's own dogfood_path, which currently FAILS as written:
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://www.theguardian.com --port 6100
  ff-rdp consent accept --port 6100
  # → must report {"cmp":"sourcepoint","action":"accepted"} WITHOUT --no-daemon
  ff-rdp click 'body' --frame guardian --port 6100
  # → must list a non-zero frame count
  for i in 1 2 3 4; do ff-rdp page-text --port 6100 & done; wait
  # → all 4 must succeed
first_call_sites: []
status: planned
---

# Iteration 137: daemon-mode parity — frame targets, concurrency, network source

**Everything [[iteration-129-consent-and-cross-origin-frames]] shipped works only with
`--no-daemon`.** In the default daemon mode — what every real invocation uses — frame
enumeration silently returns zero targets. Found in [[dogfooding-session-63]], reproduced
independently by two agents on theguardian.com and bbc.com/news.

It shipped green because **every iter-129 live test passes `--no-daemon`**
(`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs:38`). The tests and the
iteration's own `dogfood_path` disagreed, and the tests won.

## Evidence

```
$ ff-rdp consent accept --port 6100                → {"cmp": null,          "action": null}
$ ff-rdp consent accept --port 6100 --no-daemon    → {"cmp": "sourcepoint", "action": "accepted"}

$ ff-rdp click 'body' --frame zzz --port 6300            → "0 frame(s) available: "
$ ff-rdp click 'body' --frame zzz --no-daemon --port 6300 → "7 frame(s) available: https://www.bbc.com/news, ..."
```

The page genuinely has 5 frames (`window.frames.length === 5`, one cross-origin). Not even
the top-level target comes back through the proxy.

## Themes

### Theme A — frame target enumeration through the daemon proxy

Root cause (from dogfood triage, **verify on the wire before fixing**): target events are
consumed by the daemon's reader before the temporary sink installed in
`crates/ff-rdp-core/src/actors/watcher.rs:365-385` can observe them. The daemon owns the
socket; a transient sink installed by a proxied command never sees packets the daemon reader
already drained.

This is the same event-sink class of bug as iter-129's own Note 1 and iter-130's review
finding — a sink installed too late, or on the wrong side of the reader. Fix it so
`enumerate_frame_targets` returns the same targets in both modes.

Fixing this alone restores `consent accept`, the cross-origin `click` frame-scan, and `--frame`.

### Theme B — the 2-connection concurrency cap

```
$ for i in 1 2 3 4; do ff-rdp page-text --port 6300 & done
  2 succeed, 2 fail: {"error":"operation timed out after 0ms (phase: recv)"}
$ same with --no-daemon → 4/4 succeed
```

Two defects: the cap itself, and `"timed out after 0ms"`, which is not a real duration and
tells the user nothing. Consequence beyond parallelism: `network --follow` concurrent with
`navigate` is impossible in daemon mode, so "capture the network during a load" has no
working form (`navigate --with-network` is the substitute, and per iter-138 it drops fields).

Either raise/remove the cap, or — if a cap is deliberate — queue rather than fail, and emit
an honest error naming the cap when the queue is full.

### Theme C — `network` returns different data per mode

Same page, same moment: daemon → 77 rows, `source: watcher`; `--no-daemon` → 137 rows,
`source: performance-api`. iter-128 made the *output modes* agree; the *connection modes*
still don't. Make the source selection deterministic and identical, or state the difference
in `meta` so it is at least visible.

### Theme D — live tests must cover the default path

The discipline failure that let this ship. For every iter-129 theme, add a live test that runs
**without** `--no-daemon`. Where a test genuinely needs a direct connection, it must be paired
with a daemon-mode sibling. Consider a guard that flags new live tests using `--no-daemon`
without a daemon-mode counterpart.

Re-run [[iteration-129-consent-and-cross-origin-frames]]'s `dogfood_path` verbatim as part of
this iteration and confirm it passes as written.

## Acceptance criteria

- [ ] live_137_frame_targets_via_daemon: `enumerate_frame_targets` returns the same non-zero
      target count via daemon and `--no-daemon` on a multi-frame page
- [ ] live_137_consent_accept_via_daemon: `consent accept` on a Sourcepoint site returns
      `{"cmp":"sourcepoint","action":"accepted"}` **without** `--no-daemon`
- [ ] live_137_click_cross_origin_via_daemon: cross-origin frame click succeeds in daemon mode
- [ ] live_137_concurrent_commands: 4 concurrent proxied commands all succeed (or queue and
      succeed); no `0ms` duration appears in any error
- [ ] live_137_network_source_parity: `network` reports the same source and row count in both
      connection modes on a settled page
- [ ] unit_no_daemon_live_test_guard (Theme D): guard proving new `--no-daemon` live tests
      have daemon-mode coverage
- [ ] iteration-129 `dogfood_path` re-run verbatim and passing

## Notes

- Highest-priority iteration in the session-63 backlog: one bug (Theme A) accounts for two
  MAJOR findings and silently voids a whole shipped iteration.
- Do not trust the stated root cause without confirming on the wire — iter-135 is the cautionary
  precedent, where three plausible hypotheses were all wrong and the real cause was ours.
