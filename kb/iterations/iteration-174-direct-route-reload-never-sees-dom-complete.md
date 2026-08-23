---
title: "Iteration 174: on a direct connection `reload`/`back`/`forward` never receive dom-loading/dom-complete, so every one burns its whole events budget"
type: iteration
date: 2026-08-17
status: in-review
branch: iter-174/direct-route-reload-dom-complete
depends_on:
  - iteration-169-navigate-status-delivery-and-nav-verb-parity
first_call_sites: []
dogfood_path: |
  # Found by iteration 169 while adding `status`/`status_reason` to the three
  # history verbs; PRE-EXISTING, reproduced on `main` at 86262f0 with a binary
  # built from a clean worktree, so it is not caused by that iteration.
  
  ff-rdp launch --headless --debug-port 7402
  python3 -m http.server 8099 &   # any static origin will do
  
  # 1. The daemon route — fast, and reports the status.
  ff-rdp --port 7402 --timeout 30000 navigate http://127.0.0.1:8099/
  ff-rdp --port 7402 --timeout 30000 reload --jq '.results'
  # → 2026-08-17, main@86262f0:
  #   {"action":"reload","committed_url":"http://127.0.0.1:8099/",
  #    "ready_state":"complete","elapsed_ms":112}
  
  # 2. The same reload with --no-daemon — 21 seconds, on a static localhost page.
  ff-rdp --port 7402 --no-daemon --timeout 30000 reload --jq '.results'
  # → 2026-08-17, main@86262f0:
  #   {"action":"reload","committed_url":"http://127.0.0.1:8099/",
  #    "ready_state":"complete","elapsed_ms":21029}
  #   21 029 ms is exactly `split_wait_budget(30000).1` — the events phase was
  #   exhausted and `wait_for_readystate_complete` supplied the answer.
  
  # 3. Which events actually arrive (iteration 169 added this tracing):
  RUST_LOG=debug ff-rdp --port 7402 --no-daemon --timeout 8000 reload 2>&1 \
    | grep "document-event observed"
  # → direct:  event="will-navigate" url=            ... and nothing else, ever
  RUST_LOG=debug ff-rdp --port 7402 --timeout 8000 reload 2>&1 \
    | grep "document-event observed"
  # → daemon:  event="dom-loading"     url=http://127.0.0.1:8099/
  #            event="dom-interactive" url=http://127.0.0.1:8099/
  #            event="dom-complete"    url=
  
  # 4. `back`/`forward` are worse: on a direct connection they can exhaust the
  #    readystate fallback too and exit 124.
  ff-rdp --port 7402 --no-daemon --timeout 30000 back
  # → {"error":"navigate: document.readyState did not reach 'complete' (with
  #    fresh navigation) within 30078ms …","error_type":"Timeout"}
tags:
  - iteration
  - navigate
  - daemon-parity
  - carry-over
---

# Iteration 174: on a direct connection the three history verbs never receive `dom-loading`/`dom-complete`

Carry-over from [[iteration-169-navigate-status-delivery-and-nav-verb-parity]], filed before that
PR merges per CLAUDE.md's carry-over rule. **Pre-existing** — reproduced on `main` at `86262f0`
from a separate worktree build, so iteration 169 did not cause it; iteration 169 only made it
*visible*, because the readystate fallback it lands in reports `status_reason: "not_observed"`
where the old envelope simply had no `status` key to be wrong about.

## What is wrong

`wait_for_navigation_commit` (`commands/navigate.rs`) waits on `document-event` resources for
`reload`/`back`/`forward`. On a **direct** connection those resources stop after `will-navigate`:
`dom-loading`, `dom-interactive` and `dom-complete` are never delivered. The wait therefore always
runs to the end of `split_wait_budget(timeout).1` and falls through to
`wait_for_readystate_complete`, which polls `document.readyState` and — on a page that did load —
returns a correct-looking envelope. The command "works", 70× slower than it should, and reports
`status: null, status_reason: "not_observed"` because the fallback path never correlates a
document request.

Through the daemon the same three verbs get all three events and finish in ~112 ms, because the
daemon holds its own `watchTargets("frame")` + `watchResources` subscription on the shared
connection and forwards what arrives.

Why nobody noticed for four iterations: `live_130_reload_envelope` and
`live_138_back_forward_committed_url_is_top_frame` both run `--no-daemon` and both assert only
`committed_url` / `ready_state` / *presence* of `elapsed_ms` — every one of which the readystate
fallback supplies. Nothing asserted that the events path was the one that answered, and nothing
bounded `elapsed_ms`.

## Where to look, in the order the evidence favours

1. **the `watchTargets("frame")` / `watchResources` ordering and lifetime on the direct route.**
   `navigate`'s `run_core` and `wait_for_navigation_commit` issue the same prelude, and `navigate`
   works — but `navigate` sets `poll_enabled: true`, so its interleaved `document.readyState`
   fast path answers at ~300 ms and *hides* whether its events ever arrived either. Check
   `navigate --no-daemon --wait-strategy events` first: if that also burns the budget, the defect
   is in the prelude and covers all four verbs, not three.
2. **the `reload`/`goBack`/`goForward` raw dispatch.** These are sent as un-acked raw writes
   (deliberately — see `wait_for_navigation_commit`'s doc comment). Confirm on the wire that
   Firefox actually replies, and that the reload is not being issued against a target actor whose
   docshell the watcher is no longer watching.
3. **target switching.** `will-navigate` arriving alone is what a *target swap* looks like from a
   client that watched the old target only: the new `WindowGlobalTarget` gets its own
   `document-event` stream and our watcher never subscribed to it. iter-129's
   `enumerate_frame_targets` and iter-137's target-event forwarding are the relevant prior art,
   and the daemon route working is consistent with this reading (the daemon *does* keep
   `watchTargets` engaged across the swap).

Rule out (1) before touching (3); it is one command.

## Measured 2026-08-22 — it is candidate (1), the prelude, and it covers all four verbs

Run on FF154, `main` @ `7d457af`, a static `python3 -m http.server` origin, binary built from
this worktree. The plan's own step 1 settled it in one command:

```text
ff-rdp --port 7402 --no-daemon --timeout 30000 reload
  → elapsed_ms 21011                                        (reproduces the plan's 21029)
ff-rdp --port 7402 --no-daemon --timeout 30000 navigate <url> --wait-strategy events
  → {"error":"navigate: page did not fire dom-complete within the timeout …"} after 30.0 s
```

`--wait-strategy events` has no readystate fallback, so it does not merely run slow — it fails
outright. Per the plan's own rule ("if that also burns the budget, the defect is in the prelude
and covers all four verbs, not three"), candidates (2) raw dispatch and (3) target switching are
ruled out without being touched.

### What the wire says

`RUST_LOG=trace` on the failing `reload`, direct route:

```text
→ getTarget      (tabDescriptor)                ← frame form, consoleActor, etc.
→ getWatcher     (no arguments)                 ← watcher11
→ watchTargets   {targetType:"frame"}           ← bare ack, and NO target-available-form
→ watchResources ["document-event","network-event"] ← bare ack
→ reload         (raw)                          ← ack
← document-event  will-navigate                 from watcher11
← network-event   cause=document  url=…/b.html  from watcher11
← resources-updated-array  status 304           from watcher11
… 21 s of nothing, then the readystate fallback answers …
```

Every resource that arrives is emitted by the **parent** process. Every resource that is missing
is emitted by the **content** process. That split is the whole diagnosis: `dom-loading`,
`dom-interactive` and `dom-complete` come from the per-target `document-event` watcher that runs
in the content process, and Firefox never instantiated a watcher-owned target for the top-level
window global — hence no `target-available-form` either.

### The one-word cause

The direct route called `getWatcher` **without** `isServerTargetSwitchingEnabled: true`. The
daemon's `establish_watcher` (`daemon/server.rs`) has always passed it. iteration 129 already
documented that the flag gates `target-available-form` delivery
(`kb/research/frame-targets.md`); what nobody had connected is that it therefore also gates every
content-process resource, `document-event` included.

### After

Same machine, same page, `getWatcher {isServerTargetSwitchingEnabled: true}` on the direct route:

| command (`--no-daemon`, `--timeout 30000`) | before | after |
|---|---|---|
| `reload` | `elapsed_ms 21011`, `status null / not_observed` | `elapsed_ms 115`, `status 304` |
| `navigate --wait-strategy events` | timeout at 30 000 ms | `elapsed_ms 122`, `status 304` |
| `navigate` (default `both`) | `elapsed_ms ~300` from the readystate poll | `elapsed_ms 108` from events |
| `back` / `forward` | exit 0, `elapsed_ms 13` (see below) | exit 0, `elapsed_ms 6–10` |

`RUST_LOG=debug … | grep "document-event observed"` after the fix, direct route:
`will-navigate`, `dom-loading`, `dom-interactive`, `dom-complete` — the full cycle the daemon
route always had.

### One premise of this plan did not reproduce

The plan's step 4 asserts `back`/`forward` "can exhaust the readystate fallback too and exit
124". On this fixture they did **not**: on the unfixed binary `back --no-daemon` exited 0 in
13 ms. A history traversal restored from BFCache is resolved by the iter-138 same-document /
`location.href` path, which needs no `dom-complete`. So AC 3 is met after the fix, but it was
not failing here beforehand — the exit-124 case the plan recorded must need a page BFCache
declines, and this iteration did not reproduce or chase it. Recorded rather than reworded.

## Scope [4/5]

- [x] measure which of the three candidates it is, and record the measurement here before fixing
- [x] `reload --no-daemon` on a static localhost page resolves from the **events** path, not the
      readystate fallback
- [ ] `back`/`forward` `--no-daemon` no longer exit 124 on a page with history
      — **left unticked**: they exit 0 after the fix, but they also exited 0 *before* it on the
      fixture used here (13 ms, BFCache path). Nothing was measured to be broken, so nothing can
      be claimed fixed. See "One premise of this plan did not reproduce" above.
- [x] a live test that bounds `elapsed_ms` on the direct route rather than merely asserting the
      key exists — the gap that let this survive four iterations
- [x] `live_169_nav_verb_status_parity`'s `expect_reload_status` parameter is deleted and the
      direct leg asserts `status == 200` like the daemon leg

## Acceptance Criteria [4/4]

- [x] the cause is named with the measurement that settled it, recorded in this plan before the fix
- [x] `ff-rdp --no-daemon --timeout 30000 reload` on a static localhost page reports
      `elapsed_ms` under 2 000 ms, quoted in the PR body against the 21 029 ms measured here
- [x] `ff-rdp --no-daemon back` and `forward` exit 0 on a page with history, with a live test
      (`live_174_nav_verbs_resolve_from_events_{direct,daemon}`) — but read the "premise did not
      reproduce" section: this AC was already true before the fix on this fixture
- [x] the direct leg of `live_169_nav_verb_status_parity` asserts `status == 200` for `reload`,
      with `expect_reload_status` removed

## Notes

- Related: [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] (parent — added the
  `document-event observed` / `network-event resource observed` tracing this was localised with),
  [[iteration-130-navigation-truthfulness]] (the four-verb parity promise),
  [[iteration-138-navigation-truthfulness-2]] (Theme D, `elapsed_ms` honesty — which is what makes
  the 21 029 ms readable at all).
- The `--no-daemon` route is not exotic: every `live_*` test that uses `base_args` runs on it, and
  `CONTRIBUTING`'s daemon-parity rule exists precisely because the two routes drift.
