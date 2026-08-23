---
title: "Iteration 181: `assert_network` in `ff-rdp run` is a race by construction on the direct route"
type: iteration
date: 2026-08-22
status: done
branch: iter-181/playbook-scoped-network-subscription
depends_on: []
first_call_sites: []
dogfood_path: |
  # Product defect, isolated and measured in iteration 179. This plan carries
  # the FIX; 179 shipped only the diagnosis and the diagnostics.
  
  # 1. Read the contract that makes it a race. `run` opens a fresh connection
  #    per step; `execute_assert_network` -> `network::run_get_events_with_route`
  #    arms the `network-event` watcher when the step STARTS and unwatches when
  #    it ends. Firefox's watchResources does not replay history, so a request
  #    that completed before the arming is never delivered.
  #    crates/ff-rdp-cli/src/commands/network.rs   (run_get_events_with_route)
  #    crates/ff-rdp-cli/src/script/runner.rs      (run_script step loop)
  
  # 2. Reproduce deterministically by loading the machine. Measured 2026-08-22:
  #    idle (15-min load ~7)          -> 4/4 PASS
  #    under a -j6 sweep (1-min 138+) -> 8/8 FAIL, events_in_buffer=0 every time
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo nextest run -p ff-rdp-cli --test live --run-ignored all -j6 --no-fail-fast &
  sleep 75
  for i in $(seq 8); do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
      cargo test -q -p ff-rdp-cli --test live -- --ignored --exact \
      live_62_page_map_index::live_runner_page_map_resolution
    uptime
  done
  
  # 3. The zero is not a partial buffer and never will be: the playbook has
  #    exactly ONE request in flight, so losing the arming race yields 0, not N.
  #    Do not re-derive this; iteration 179 already did.
  
  # 4. The daemon route does NOT have this defect — it holds a standing
  #    subscription. Any fix must not regress that path.
tags:
  - iteration
  - network
  - runner
  - playbook
  - race
---

# Iteration 181: give `run` a playbook-scoped network subscription

Carry-over from [[iteration-179-live-62-runner-sees-no-network-events]], which established the
cause and shipped the diagnostics but deliberately did **not** attempt the fix.

## The defect

`ff-rdp run` executes each step against its own connection. `assert_network` therefore arms its
`network-event` watcher only once the step begins — *after* the `click` or `navigate` that
produced the request it is asked to assert on. The canonical playbook shape

```json
{"click":  {"selector": "button[type=submit]"}},
{"assert_network": {"url_contains": "/api/auth/sign-in", "status": 200}}
```

is a race between the arming sequence (connect → `getWatcher` → `watchResources`) and the
response. Idle, the arming usually wins. Loaded, it loses — and because the playbook has exactly
one request in flight, losing produces `events_in_buffer: 0`, which reads like a broken
subscription and is not one.

This affects **every** `assert_network` user on the direct route, not just `live_62`.

## What iteration 179 already established — do not redo it

| question | answer |
|---|---|
| Is the subscription per-step or playbook-long? | Per-step, direct route only |
| Why zero rather than a partial buffer? | One request in flight; losing the race loses all of it |
| Is the daemon route affected? | **No** — standing subscription, buffers across steps |
| Does load decide it? | Yes: 4/4 PASS at 15-min load ~7; 8/8 FAIL at 1-min load 138–220 |

## Themes

- **A — Subscribe for the playbook, not the step.** When a script contains any `assert_network`
  step, arm a `network-event` watcher before the first step and drain it at each assertion, so a
  request triggered by step N is visible to step N+1. The runner is deliberately stateless per
  step today; this is the change that makes it not be, and it needs to survive a step that
  navigates away.
- **B — Keep the daemon route on its existing path.** It is already correct; the new code must not
  double-subscribe or change what `route: "daemon"` reports.
- **C — Make `live_62` prove it.** `live_62_page_map_index::live_runner_page_map_resolution` must
  pass under the `-j6` load generator from the dogfood path, not merely idle.

## Tasks

### A. Playbook-scoped subscription [3/3]
- [x] `run` arms one `network-event` subscription for the whole script when any step needs it
- [x] Events from step N are visible to an `assert_network` at step N+1
- [x] The subscription survives an intervening `navigate`, or the limitation is documented
      [live_62's script navigates at step 1, clicks at step 3 and asserts at step 4 — it passes,
      so the watcher armed before the navigate still delivers the post-navigate request]

### B. Daemon parity [1/1]
- [x] `route: "daemon"` behaviour and output are unchanged, with a test pinning it

### C. Proof under load [2/2]
- [x] `live_62_page_map_index::live_runner_page_map_resolution` passes 8/8 under the `-j6` load
      generator [2026-08-23: 8/8 PASS at 1-min load 145 → 223, the band where iteration 179
      measured 8/8 FAIL; and `ok` in the full sweep]
- [x] `empty_buffer_hint` stops appearing for this playbook, and the hint text is revised to match
      whatever contract now holds

## Acceptance Criteria [3/3]

- [x] A `click` followed by `assert_network` is deterministic on the direct route under sustained
      load, measured the same way iteration 179 measured the failure [2026-08-23: same
      `cargo nextest -j6` generator, same 8 repetitions, 8/8 PASS]
- [x] `kb/reference/script-format.md`'s "assert_network's subscription window" section is rewritten
      to describe the new contract — it currently documents the race as the contract
- [x] The daemon route is demonstrably unaffected

## Results (2026-08-23)

Implemented as `crates/ff-rdp-cli/src/commands/network_watch.rs`: `PlaybookNetworkWatch` owns a
dedicated connection with `watchResources(["network-event"])` armed before the script's first
step, accumulates resources and updates for the whole run (capped at 4096 requests, oldest
evicted, eviction count surfaced as `diagnostics.evicted_requests`), and unwatches on `Drop`.
`run_script` arms it when a script contains an `assert_network` **or** a `run:` step — a
sub-script is not parsed until it executes, so a conservative arm is the only one that can
precede the parent's `click`. A nested `run:` inherits the subscription and hands it back even
when it bails.

Review pass (2026-08-23): the eviction cap was reachable only through a live Firefox connection
(`PlaybookNetworkWatch::arm`), so the 4096-request policy — real data loss beyond diagnostics —
had no direct test. Pulled the pure logic into `evict_overflow_from(&mut Vec<NetworkResource>,
&mut HashMap<...>, cap)` and unit-tested it directly: under cap, over cap (oldest-first, updates
dropped with their resource), and the cap=0 degenerate case.

`assert_network` now polls that buffer in 250 ms slices until it matches or the step `timeout`
expires, so the timeout became a ceiling on waiting for an in-flight request rather than a window
the request must land inside. Diagnostics gained `subscription: playbook | step | daemon`, and
the empty-buffer hint now differs by subscription: the playbook hint explicitly says the zero is
**not** the arming race, because repeating iteration 179's story there would send the next reader
down the path 179 spent four days on.

The daemon route is untouched: `arm` returns `Ok(None)` when the connection resolves to the
daemon, and `crates/ff-rdp-cli/tests/e2e/daemon_parity.rs::daemon_run_assert_network_uses_the_standing_subscription`
pins that a daemon-route `assert_network` still reports `route: "daemon"`, `subscription:
"daemon"`, and no direct-route hint.

`live_62`'s fixture now fires its `POST /api/auth/sign-in` with **no** delay. The old 150 ms
`setTimeout` existed only to keep the request inside the per-step watcher's window; removing it
means the request has completed before `assert_network` starts, so the test is green only if the
playbook buffer really carries step N's request into step N+1.

### Measurements

- Full live sweep, `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` →
  `LIVE_SWEEP_SUMMARY executed=275 skipped=0 preexisting=9 vanished=0 launch_timeout=0 total=284`,
  274 passed / 1 failed. The one failure is
  `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` (`live_target_count: 0` against
  theguardian.com) — unrelated to this change; see the carry-over row.
- Load reproduction, identical to the `dogfood_path` recipe: **8/8 PASS** at 1-minute load
  145 → 223. Iteration 179 measured **8/8 FAIL** with `events_in_buffer: 0` at load 138–220.

## Closing verification (2026-08-23, resumed session)

The closing live sweep quoted under **Measurements** was run at commit `2d66f38`, the last commit
that touches product source on this branch; everything after it is docs. That sweep is the record
for this PR. It was **not** re-run when the session resumed: four sibling `ff-rdp-profile` Firefox
instances were live and the machine sat at load 31/49/78 from parallel agents, and
`iteration-close` is explicit that a sweep taken under interference must not be treated as the
record. Killing those browsers to clear the machine is forbidden by the same policy.

Re-verified instead, in the resumed session:

- `live_62_page_map_index::live_runner_page_map_resolution` — **4/4 PASS** at 1-minute load 27–29.
  Weaker than the 8/8 at load 145–223 already recorded, but independent of the killed attempt.
- `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
  `cargo test --workspace -q` green (including
  `daemon_parity::daemon_run_assert_network_uses_the_standing_subscription`).
- All six xtask `check-*` gates exit 0: `check-iteration-plan`, `check-source-invariants`,
  `check-firefox-refs`, `check-actor-kb-sync --since origin/main`, `check-live-test-layout`,
  `check-dogfood-script` (SKIP — no `dogfood_script` field).

## Carry-over

| item | disposition |
|---|---|
| Sweep failure `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` (`live_target_count: 0` vs theguardian.com) | **folded** into [[iteration-190-live-sweep-only-failures]] — see its "Folded in from iteration 181's closing sweep" section; it is a second instance of that plan's Theme B class |
| Sweep `preexisting=9` — nine `ff-rdp-core` live tests wanted a port-6000 browser nobody had started, so they never executed | **no plan** — an unmet env precondition correctly classified as `ignored` (iter-173's mechanism working, `vanished=0`). If a later sweep reports `vanished>0` or the count drifts, that needs its own plan |
| Between steps nothing reads the subscription socket, so events queue in the kernel receive buffer; a very long playbook against a very chatty page can fill it and push queueing back onto Firefox | **no plan** — nothing measured; the tradeoff and the rejected alternative (a short read timeout desyncs `recv_from`'s `read_exact` mid-frame) are documented in `network_watch.rs`'s module docs. A real playbook stalling or missing an event attributable to this needs its own plan |
| The playbook buffer caps at 4096 requests and evicts oldest-first, so a long chatty run can now lose an old request | **closed in this PR** — evictions are counted and surfaced as `diagnostics.evicted_requests`, so a miss caused by eviction is never silent; the eviction policy itself now has direct unit tests (`unit_181_evict_overflow_*`), added in review |
| If arming the playbook subscription fails, `assert_network` falls back to per-step arming — iteration 179's race, restored for that run | **closed in this PR** — a stderr warning names the fallback and the diagnostics report `subscription: "step"` rather than `"playbook"` |
| `live_62` passed 4/4 idle in iteration 179 and 8/8 FAIL under load; one green run is not proof | **closed in this PR** — it is the defect this iteration fixes, and it was measured under the same `-j6` generator that produced the 8/8 failure (8/8 PASS at load 145–223), then again 4/4 at load 27–29 |

## Out of scope

- **Relaxing `live_62`'s assertion**, `#[ignore]`-ing it, or routing it through the daemon purely
  to dodge the direct-route race. The direct route is the one with the defect; a green test that
  no longer exercises it is worse than a red one.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — the diagnosis this fix implements
- [[iteration-188-live-sweep-cost-and-parallelism]] — source of the `-j6` load generator
