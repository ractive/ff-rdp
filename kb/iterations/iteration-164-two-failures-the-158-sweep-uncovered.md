---
branch: iter-164/block-url-and-daemon-autostart
date: 2026-08-14
depends_on: []
dogfood_path: |
  # ── 1. URL blocking does not block ──────────────────────────────────────────
  ff-rdp launch --headless --debug-port 7201
  ff-rdp --port 7201 throttle --block favicon
  ff-rdp --port 7201 navigate https://example.com
  ff-rdp --port 7201 eval "(async () => { try { \
      await fetch('https://example.com/favicon.ico?x=' + Date.now(), {cache:'no-store'}); \
      return 'resolved'; } catch (e) { return 'rejected'; } })()"
  # → must print "rejected". On main it prints "resolved" — the block list is
  #   accepted by the CLI and reported by `throttle --status`, but the request
  #   still completes.
  ff-rdp --port 7201 throttle --status --jq '.results'
  # → the pattern must be listed, so the failure is in enforcement, not intake.

  # ── 2. Daemon autostart under load ──────────────────────────────────────────
  # Reproduce under real contention — this only fails when the machine is busy.
  for i in 1 2 3 4 5 6 7 8; do (ff-rdp launch --headless --debug-port $((7210+i)) \
      && ff-rdp --port $((7210+i)) eval 1 >/dev/null) & done; wait
  for i in 1 2 3 4 5 6 7 8; do ff-rdp --port $((7210+i)) daemon status \
      --jq '.results.running'; done
  # → eight `true`. On main, at load average ~18, at least one is false: the
  #   autostart handshake gives up and the caller silently falls back to a
  #   direct connection.
status: planned
title: "Iteration 164: URL blocking does not block, and daemon autostart gives up under load"
type: iteration
tags:
  - iteration
---

# Iteration 164: URL blocking does not block, and daemon autostart gives up under load

Carry-over from [[iteration-158-launch-lifecycle-and-harness-honesty]], filed before that PR
merges per CLAUDE.md's carry-over rule. Both defects were found by iter-158's qualified
`live-sweep` on 2026-08-14 and **neither is caused by that iteration's changes** — iter-158's
product diff touches only `commands/launch.rs` and `daemon/client.rs`.

```
LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230
219 passed · 2 failed · 32.7 min wall (load average 18.6)
```

## Defect 1 — `throttle --block <pattern>` does not block

```
---- live_109_throttle_block::live_block_url_pattern stdout ----
LiveFirefox: daemon proxy port=55398
panicked at crates/ff-rdp-cli/tests/live/live_109_throttle_block.rs:215:5:
  assertion `left == right` failed: a fetch of a blocked URL (matching 'favicon') must reject:
  "resolved"
    left: String("resolved")
   right: "rejected"
```

The in-page probe is the assertion: after `throttle --block favicon`, a `fetch()` of
`https://example.com/favicon.ico?x=<now>` **resolved**. The block list is accepted and echoed
back, so intake works; enforcement does not.

### Why this was never seen before

`live_block_url_pattern` is gated
`#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]`.
The 2026-08-13 baseline sweep ran with `FF_RDP_LIVE_NETWORK_TESTS` **unset** (`executed=197`), so
this test has never executed in a sweep. iter-158's run is the first with both gates set
(`executed=221`); the 24 extra tests are the network tier, and this is one of them.

Start by establishing whether it ever worked: the feature landed in `465e98b feat(throttle):
network throttling & URL blocking via network-parent actor`. Check whether the netmonitor's
block API changed shape (the test's own comment notes the blocked-flag field name "has varied
across Firefox versions", which is why it probes from inside the page rather than reading the
flag).

## Defect 2 — daemon autostart gives up under load, and the caller cannot tell

```
---- live_141_output_hygiene::live_141_text_empty_result_keeps_metadata stdout ----
panicked at crates/ff-rdp-cli/tests/live/live_141_output_hygiene.rs:59:5:
  live_141_text_empty_result_keeps_metadata: the proxy daemon did not start for Firefox
  on port 61670
```

`LiveFirefox::with_daemon` triggers autostart via an `eval`, sleeps 500 ms, then reads
`daemon status`. At load average 18.6 the daemon had not written its registry entry in time.

The product side of this is `resolve_connection_target`: when the daemon does not come up it
falls back to `ConnectionTarget::Direct` with a `deferred_warning` that is **dropped** when the
direct connection then succeeds. That is deliberate (the warning was benign noise on the happy
path) but it means a caller who asked for daemon mode and silently got direct mode has no
signal — the same class of dishonesty iter-158 removed from the test harness. Consider surfacing
it in `meta` under `--verbose` rather than discarding it.

The harness half is separate and cheaper: `with_daemon`'s fixed 500 ms sleep should be a bounded
poll.

### Why this surfaced now

Pre-iter-158, `firefox_with_daemon` returned `Option<LiveFirefox>` and every caller did
`else { return; }` — libtest reports that as `ok`. The condition is not new; the reporting is.
This is precisely what Theme D was for, and it found a real one on its first run.

## Acceptance Criteria [0/5]

- [ ] live_164_block_url_pattern_rejects: after `throttle --block favicon`, an in-page
      `fetch('…/favicon.ico')` rejects, and an un-blocked URL still resolves — i.e.
      `live_109_throttle_block::live_block_url_pattern` passes inside a full
      `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` sweep
- [ ] unit_164_block_patterns_reach_the_actor: a unit test asserts the block-list request sent
      to the network-parent actor carries the patterns the CLI accepted, so an intake-vs-
      enforcement regression is distinguishable without Firefox
- [ ] unit_164_with_daemon_polls_instead_of_sleeping: the harness helper waits for the registry
      entry on a bounded poll rather than a fixed 500 ms sleep, asserted against a stub registry
- [ ] live_164_daemon_autostart_survives_load: eight concurrent launch+`eval` pairs each report
      `daemon status --jq '.results.running' == true`
- [ ] unit_164_silent_direct_fallback_is_reported: when `resolve_connection_target` falls back to
      a direct connection after a failed autostart, the dropped `deferred_warning` is surfaced in
      `meta` under `--verbose` instead of being discarded

## Notes

- Do **not** fix these by loosening the tests. `live_block_url_pattern`'s in-page probe is the
  strongest available observation of blocking and must stay; `live_141`'s daemon assertion is
  what made the second defect visible at all.
- Related: [[iteration-158-launch-lifecycle-and-harness-honesty]] (the sweep that found both),
  [[analysis-2026-08-13-what-ff-rdp-became]] §3.2 (the `network` watcher regression is a
  *different* subsystem defect and is not in this plan either).
