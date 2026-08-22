---
title: "Iteration 181: `assert_network` in `ff-rdp run` is a race by construction on the direct route"
type: iteration
date: 2026-08-22
status: planned
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
tags: [iteration, network, runner, playbook, race]
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

### A. Playbook-scoped subscription [0/3]
- [ ] `run` arms one `network-event` subscription for the whole script when any step needs it
- [ ] Events from step N are visible to an `assert_network` at step N+1
- [ ] The subscription survives an intervening `navigate`, or the limitation is documented

### B. Daemon parity [0/1]
- [ ] `route: "daemon"` behaviour and output are unchanged, with a test pinning it

### C. Proof under load [0/2]
- [ ] `live_62_page_map_index::live_runner_page_map_resolution` passes 8/8 under the `-j6` load
      generator
- [ ] `empty_buffer_hint` stops appearing for this playbook, and the hint text is revised to match
      whatever contract now holds

## Acceptance Criteria [0/3]

- [ ] A `click` followed by `assert_network` is deterministic on the direct route under sustained
      load, measured the same way iteration 179 measured the failure
- [ ] `kb/reference/script-format.md`'s "assert_network's subscription window" section is rewritten
      to describe the new contract — it currently documents the race as the contract
- [ ] The daemon route is demonstrably unaffected

## Out of scope

- **Relaxing `live_62`'s assertion**, `#[ignore]`-ing it, or routing it through the daemon purely
  to dodge the direct-route race. The direct route is the one with the defect; a green test that
  no longer exercises it is worse than a red one.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — the diagnosis this fix implements
- [[iteration-180-live-sweep-cost-and-parallelism]] — source of the `-j6` load generator
