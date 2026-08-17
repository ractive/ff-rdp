---
branch: iter-169/navigate-status-delivery
date: 2026-08-16
depends_on:
  - iteration-166-navigate-status-is-null-under-the-daemon
dogfood_path: |
  # Theme A — the document's status update is sometimes never delivered at all.
  # Measured on iter-166's branch, idle machine, 12 consecutive runs:
  #   1 of 12 fails; on `main` (pre-166) 3 of 12 fail. The failing envelope is
  #   {"status":null,"status_reason":"no_status_reported", "ready_state":"complete"}
  #   after a full 2000 ms grace window, so the update is LOST, not merely late.
  for i in $(seq 1 20); do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
      cargo test -q -p ff-rdp-cli --test live live_138_navigate_reports_404 \
      -- --ignored --test-threads=1 2>&1 | grep -E '^test result'
  done
  # → expect 20 ok. Any FAILED reproduces the defect; the panic message prints
  #   the whole envelope, and `status_reason` names which half is missing.

  # Theme B — the other three navigation verbs report no status at all.
  ff-rdp launch --headless --debug-port 7402
  ff-rdp --port 7402 navigate https://example.com --jq '.results | keys'
  # → includes "status" and "status_reason"
  ff-rdp --port 7402 reload --jq '.results | keys'
  # → 2026-08-16: ["committed_url","elapsed_ms","ready_state"] — no status key
  #   at all, so `--jq '.results.status'` yields null for a reason no caller can
  #   see. iter-130 Theme B promised all four verbs report the same shape.
status: planned
title: "Iteration 169: navigate's document status update is occasionally never delivered, back/forward/reload report no status at all, and the live suite kills a Firefox it does not own"
type: iteration
tags:
  - iteration
  - navigate
  - network
  - carry-over
---

# Iteration 169: `navigate`'s document status update is occasionally never delivered, `back`/`forward`/`reload` report no status at all, and the live suite kills a Firefox it does not own

Carry-over from [[iteration-166-navigate-status-is-null-under-the-daemon]], filed before that PR
merges per CLAUDE.md's carry-over rule. Both items were found *by* iter-166 — the first by its
live sweep, the second while writing DEC-040 — and neither is fixed by it.

## Theme A — the status update is lost, not late

iter-166 fixed the matching bug that made `navigate` report `status: null` for every ordinary
page. It did **not** fix a second, rarer failure underneath it:
`live_138_navigate_reports_404` still fails roughly 1 run in 12 on an idle machine, and it failed
once in iter-166's own live sweep.

What is known, measured on iter-166's branch:

- the failing envelope is
  `{"status":null,"status_reason":"no_status_reported","ready_state":"complete","elapsed_ms":~160}`.
  `no_status_reported` (new in iter-166) means the document's `network-event` resource **was**
  identified — so the request is not being mis-matched — and no `resources-updated-array` entry
  carrying a status was ever seen for it;
- iter-166 raised the post-commit grace window for exactly this case from 300 ms to 2000 ms,
  which cut the failure rate from **3 in 12 to 1 in 12** (both measured, 12 runs each, same
  machine, idle). The residual failures exhaust the full 2000 ms. So the remaining cases are not
  a latency problem that a longer wait will solve — the update is **not arriving at all**;
- the document itself loaded: `ready_state` is `complete` and `committed_url` is right, so the
  404 response line demonstrably reached Firefox.

Where to look, in the order the evidence favours:

1. the daemon's `network-event` stream (`start_daemon_stream`). `navigate` asks the daemon to
   stream `network-event` *after* subscribing but the daemon manages that resource type
   centrally; if an update is delivered to the daemon between the `watchResources` and the
   `startStream`, whether it is buffered or dropped decides this bug. iter-164 worked in this
   exact area (`unwatchResources` handling) and iter-159/DEC-037 documents the ownership rule;
2. `ResourceCommand::dispatch_event`'s fan-out — whether a `resources-updated-array` frame with
   no matching subscriber is dropped silently;
3. Firefox itself coalescing the update for a small, immediately-complete localhost response.
   Rule this out last, and only with a packet-level capture (`--record`), not by inference.

Fix the delivery, not the symptom. Do **not** widen the grace window further: it is already 2000
ms and the measurement above shows more time does not help.

### Added 2026-08-17 — this is bigger than `live_138`, and the 2000 ms claim above does not hold

Measured on `main` at `4d639e2` (post-166, post-167, post-168), two full dual-gate sweeps run by
hand on an otherwise-idle machine, `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
cargo run -p xtask -- live-sweep`, `executed=270 skipped=0 preexisting=0` both times:

- **iter-166's own acceptance test fails, 2 sweeps out of 2** —
  `live_166_navigate_document_status::live_166_navigate_reports_document_status`, on the
  "daemon, no trailing slash" leg. It was the *only* failure in the second, fully clean sweep
  (269 passed / 1 failed). So the defect is not confined to `live_138`'s 404 fixture, and it is
  not rare: it is the ordinary `https://example.com` daemon path, reproducing on demand under
  sweep load while passing in isolation.
- **The failing envelopes returned in 200 ms and 250 ms**, not after the full 2000 ms:

  ```
  {"navigated":"https://example.com","status":null,"status_reason":"no_status_reported",
   "committed_url":"https://example.com/","ready_state":"complete","elapsed_ms":250}
  ```

  That contradicts the bullet above ("the residual failures exhaust the full 2000 ms"), which was
  measured only against `live_138`. Under load, `no_status_reported` is emitted an order of
  magnitude *before* the budget it is supposed to have waited out.

So Theme A now has two candidate defects, and the second one is cheaper to check first:

0. **the 2000 ms budget is not being applied on this path at all** — either the "document request
   has been identified" condition that selects the 2000 ms branch is false (in which case
   `no_status_reported` is the *wrong reason string*, and the envelope is lying about which of the
   three cases occurred — the exact failure `status_reason` was added to prevent), or the branch is
   selected and the wait is not actually performed. Read `DocumentStatusTracker`'s reason
   derivation against the grace-window selection before touching the stream. A 250 ms envelope is
   evidence the wait never happened; it is not evidence about delivery.

Only if the budget *is* genuinely being spent do candidates 1–3 above apply.

## Theme B — `back`/`forward`/`reload` report no status key at all

`nav_action.rs` builds `{committed_url, ready_state, elapsed_ms}` and stops there, so
`--jq '.results.status'` on a `reload` yields `null` — indistinguishable from `navigate`'s
meaningful `null`, and without the `status_reason` that iter-166 added specifically to make that
distinction. iter-130 Theme B promised all four navigation verbs report the same envelope shape;
they do not.

iter-166 deliberately left this alone (recorded as "not in scope" in DEC-040) because iter-138
Theme A scoped the status field to `navigate` only, and widening it mid-iteration would have gone
past the plan. It is a real gap all the same: `reload` is the verb most likely to be used to
re-check a page that was failing.

Two honest outcomes, and the iteration should pick one on evidence rather than assume:

- subscribe those three verbs to `ResourceType::NetworkEvent` as well, so they report a real
  status (they already pass `network_observed: false` to `wait_for_doc_complete`, so the wiring is
  one flag and one subscription); or
- keep them unsubscribed but emit the two keys anyway, with
  `status: null, status_reason: "not_observed"` — which is exactly what the enum variant was
  introduced to say, and costs no extra round-trip.

The second is cheap and honest; the first is more useful. Measure what the subscription costs on
a `reload` before choosing.

## Theme C — something in the CLI live suite kills a Firefox it does not own

iter-166 ran the sweep twice, both times with a hand-started
`firefox -no-remote -profile … --start-debugger-server 6000 --headless` so the `ff-rdp-core` tier
would execute rather than be reported `preexisting`. On the **second** run that Firefox was dead
by the time the core tier started: all 9 core tests failed instantly with
`ConnectionFailed(… ConnectionRefused)` in `0.00s`, and the process (a PID recorded at launch) was
gone. On the **first** run, same command, same machine, the same Firefox survived and all 9
passed. Restarting it after the second sweep and re-running the four core targets gave 9/9 `ok`
immediately.

So the CLI tier kills an unrelated, externally-started Firefox some of the time. That is worth
naming rather than shrugging at: it is a *scoping* failure, and process scoping is exactly what
`live_110_kill_scoping` exists to protect. Candidates, in order:

1. `live_110_kill_scoping` itself, or whatever it exercises — a kill that matches on process name
   or profile prefix rather than on the PID the suite launched;
2. `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` — a prune
   that decides "no Firefox is running" and cleans up more than it owns;
3. `daemon stop` scoping (iter-110's subject), if a daemon on one port can reach a Firefox it did
   not start.

This is **not** the same defect as [[iteration-168-livefirefox-drop-does-not-wait-for-exit]],
which is about a Firefox the suite *did* own outliving its `LiveFirefox`. Here the suite kills one
it never owned. Fix the scoping, not the sweep procedure — telling operators "do not run anything
else on 6000" would defeat the reason the core tier uses a fixed port.

### Added 2026-08-17 — did not reproduce in two hand-run sweeps; iter-168's instance was not ours

Two dual-gate sweeps on `main` at `4d639e2`, same hand-started port-6000 Firefox: the browser
survived both (~13 min of CLI tier each) and the core tier reported **9/9 passed** both times. So
the symptom is not reliable, and one of the three recorded instances is now explained away:
iteration 168's sweep had a **human** kill Firefox processes on this machine mid-CLI-tier
(21:37–21:40 against a 21:31–21:45 tier). That accounts for iter-168's 7 core-tier reds and for
`live_128_meta_route`'s empty registry file — see the note in
[[iteration-172-daemon-registry-torn-read-on-autostart]].

iter-166's sweep-2 instance is **not** explained: nobody was killing anything then. So this theme
stays open on that one observation, but it is a single unreproduced event, not a pattern —
size the work accordingly, and start by trying to reproduce it under sweep load before hunting
scoping candidates 1–3.

A related mechanism was measured while re-running, and it is *not* this theme: killing the test
runner mid-test orphans that test's browsers, because `LiveFirefox::drop` never runs. A sweep
killed during `live_158_launch_survives_contended_bind` left four Firefox processes alive for over
an hour; they then broke the *next* sweep's `live_158` (port 7101 held by an orphan) and
`live_96_profile_cleanup` (four profile dirs "still owned by a live process"). Every marker read
`spawned by unknown test`, so iter-151's owner-test marker does not survive a killed runner —
that gap belongs with [[iteration-171-stale-owner-pid-marker-and-pid-reuse]].

## Acceptance Criteria [0/6]

- [ ] the delivery path is identified — which of Theme A's three candidates loses the update,
      recorded in this plan with the measurement that settled it, before the fix
- [ ] `live_138_navigate_reports_404` passes 20 consecutive runs on an idle machine, and the
      count is quoted in the PR body (a single pass proves nothing — the pre-fix rate is 1 in 12)
- [ ] `back`/`forward`/`reload` emit `status` and `status_reason` on every path, with a live test
      asserting both keys are present on all three verbs
- [ ] no grace window in `navigate.rs` is longer than the 2000 ms iter-166 set — the fix must be
      in delivery, not in waiting
- [ ] the process that kills the externally-started port-6000 Firefox is identified by name, with
      the reproduction that found it — not inferred from the candidate list
- [ ] a full `live-sweep` with a hand-started Firefox on 6000 leaves that Firefox alive, verified
      by checking its PID after the sweep rather than by the core tier merely passing

## Notes

- `status_reason` is the instrument that makes Theme A diagnosable at all; before iter-166 this
  failure was an unexplained `null` identical to five other causes. Keep it.
- Related: [[iteration-166-navigate-status-is-null-under-the-daemon]] (parent),
  [[iteration-138-navigation-truthfulness-2]] (which added `live_138_navigate_reports_404` and the
  300 ms grace window), [[iteration-130-navigation-truthfulness]] (the four-verb parity promise),
  and DEC-037 in `kb/decision-log.md` (daemon-owned resource subscriptions).
