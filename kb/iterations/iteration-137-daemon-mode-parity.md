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
first_call_sites:
  - primitive: ff_rdp_core::target_events_from_packets
    site: >-
      crates/ff-rdp-cli/src/commands/frame_targets.rs (request_frame_targets replay
      of the daemon snapshot)
status: done
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

## Acceptance Criteria [7/7]

- [x] live_137_frame_targets_via_daemon: `enumerate_frame_targets` returns the same non-zero
      target count via daemon and `--no-daemon` on a multi-frame page — PASSED, 2 frames in
      both modes (`fetch_frame_targets` / `target_events_from_packets`)
- [x] live_137_consent_accept_via_daemon: `consent accept` on a Sourcepoint site returns
      `{"cmp":"sourcepoint","action":"accepted"}` **without** `--no-daemon` — verified live
      against theguardian.com immediately after `navigate` (`detect_and_accept`)
- [x] live_137_click_cross_origin_via_daemon: cross-origin frame click succeeds in daemon mode —
      PASSED, `tag: "A"`, `meta.frame_url: "https://example.com/"`
- [x] live_137_concurrent_commands: 4 concurrent proxied commands all succeed (queue via
      `claim_rpc_slot_queued`); no `0ms` duration appears in any error — PASSED 4/4
- [x] live_137_network_source_parity: `network` reports the same source and row count in both
      connection modes on a settled page — PASSED with `--source performance-api`
      (`NetworkSource`), 3 rows in both modes; `meta.source_reason` names the rule
- [x] `unit_no_daemon_live_test_guard` (Theme D) proves a direct-only live suite declares
      daemon-mode coverage; `unit_no_daemon_grandfather_list_only_shrinks` keeps the
      `GRANDFATHERED` exemption list shrinking and `unit_no_daemon_guard_detects_a_violation`
      pins the matcher (crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs). It caught this
      iteration's own new suite before it was annotated.
- [x] iteration-129 dogfood_path re-run verbatim and passing — `is_client_target_teardown` +
      `fetch_frame_targets` make `consent accept` report
      `{"cmp":"sourcepoint","action":"accepted"}`, `click --frame` list 3 frames (was
      `0 frame(s) available: `), and 4/4 concurrent `page-text` succeed

Supporting unit coverage: `unit_frame_targets_snapshot_tracks_lifecycle`,
`unit_frame_targets_snapshot_pruned_on_target_switch`,
`unit_frame_targets_request_returns_snapshot`,
`unit_frame_targets_replay_matches_direct_rules`,
`unit_rpc_slot_queue_waits_for_release`, `unit_daemon_busy_reports_real_wait`,
`unit_daemon_queued_notice_is_not_an_error`, `unit_rpc_queue_budget_exceeds_heartbeat`,
`unit_timeout_error_never_reports_zero_ms`,
`unit_zero_after_ms_renders_without_a_duration_claim`.

## What the wire actually showed (Theme A)

The plan's stated root cause — "target events are consumed by the daemon's reader before the
temporary sink ... can observe them" — was **wrong**, and the plan's own warning to verify
first was the right call. Two real causes, both confirmed against Firefox 153:

1. **`watchTargets` is not repeatable on a connection.**
   `ParentProcessWatcherRegistry.watchTargets` only adds the target type to the watcher's
   session data, so the daemon (which subscribes once at startup) makes every proxied
   client's `watchTargets("frame")` a no-op — the drain window is empty by construction and
   no event sink placement could have fixed it. Also, without
   `isServerTargetSwitchingEnabled: true` the daemon received **no** `target-available-form`
   at all (`daemon status` reported `target_count: 0` for whole sessions).
2. **`navigate` was tearing the daemon's subscription down.**
   Once switching was enabled, `navigate`'s three `unwatchTargets("frame")` teardown calls
   (`commands/navigate.rs:899,1275,1634`) landed on the *shared* connection, and under
   server-side target switching `unwatchTargets` destroys **every** target — top level
   included. Captured in the daemon log: two `target-available-form` immediately followed by
   four `target-destroyed-form`, leaving zero live targets after every navigation.

The daemon now drops client `unwatchTargets` frames (`is_client_target_teardown`) — safe
because the method is `oneway`, and correct because the subscription is daemon-owned.

`daemon status` gained `live_target_count` (targets alive now, vs. the cumulative
`target_count`) — the two diverging is the exact signature of this bug — and the
`frame-targets` reply carries `watcher_ready`, so a daemon that has not established its
subscription yet returns a `daemon_watcher_not_ready` error instead of an empty snapshot
presented as fact.

## Notes

- Highest-priority iteration in the session-63 backlog: one bug (Theme A) accounts for two
  MAJOR findings and silently voids a whole shipped iteration.
- Do not trust the stated root cause without confirming on the wire — iter-135 is the cautionary
  precedent, where three plausible hypotheses were all wrong and the real cause was ours.
- iter-136 learnings that apply to Theme A/B work in `watcher.rs`:
  - `unwatchResources`, `clearResources`, `unwatchTargets`, `clearPicker` are **oneway** —
    Firefox never replies. Any new/moved sink-installation or cleanup code around
    `enumerate_frame_targets` (`watcher.rs:365-385`) that sends these must use a fire-and-forget
    send, not a `send`+`recv` pair — the latter blocks until the socket read timeout. If you add
    a raw send in a live test, prefer `send_raw_oneway` (`crates/ff-rdp-core/tests/support/recording.rs`)
    over `send_raw` for these types; `send_raw` now panics up-front on them
    (`unit_send_raw_rejects_oneway`).
  - A blocking accept/read loop with no deadline can deadlock a test's `join()` even after the
    real work under test has already succeeded (iter-136 Theme B, `live_cookies_httponly`). If
    Theme B's concurrency-cap fix touches the daemon's connection-accept loop, give it an
    explicit bound/timeout so a live test can assert it always terminates, not just that it
    eventually accepts.
  - Any live test that flips global Firefox/daemon state for the duration of the test (here:
    concurrency cap, network source mode) should restore it even if a mid-test assertion panics
    — wrap the state-dependent body in `catch_unwind` and always run the restore, as
    `live_accessibility_tree` now does for the platform accessibility service.
  - `ac-fidelity-check.sh` matches `## Acceptance Criteria` case-sensitively; this plan's
    heading was `## Acceptance criteria` (lowercase c) and the hard gate silently reported
    "nothing to check" instead of validating the 7 ticked boxes — already fixed here. Double
    check any other iteration plan you copy this file's headings from.
