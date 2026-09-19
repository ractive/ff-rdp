---
title: "Iteration 272: Bound auto-wait while preserving timeout diagnostics"
type: iteration
date: 2026-09-19
status: planned
branch: iter-272/autowait-deadline-and-diagnostics
depends_on: [258]
first_call_sites: []
dogfood_path: |
  On an owned Firefox, compare click/type readiness timeouts on a blocked page
  with --wait-timeout smaller than --timeout. Retain the error's selector,
  match count and hidden/unstable classification alongside wall time.
  Exercise a responsive absent selector and a moving element too.
tags: [iteration, carry-over, latency]
---

# Iteration 272: Auto-wait needs a deadline-aware diagnostic contract

## Evidence

Iteration 258's source survey found `commands/js_helpers.rs::autowait_element`
uses the socket timeout inside readiness, settle installation, idle detection,
stability and selector diagnostics. The diagnostic is deliberately evaluated
after expiry to distinguish missing, hidden and unstable elements. Wrapping
each probe with the new read deadline unchanged would suppress that diagnostic
at precisely the point it is required. The same function has a separate 500 ms
stability allowance and an overall readiness budget. This requires an explicit
diagnostic policy and multi-stage regression coverage, beyond applying the
unchanged navigation-poll mechanism. This is source evidence, not a new live
failure measurement.

## Tasks [0/3]

- [ ] Capture blocked-page and responsive selector baselines on an owned browser.
- [ ] Bound all auto-wait stages with an absolute budget; retain useful diagnostics
      from observed evidence and define fallback wording when no probe answered.
- [ ] Cover blocked reads, push traffic, late replies and each diagnostic category
      without weakening the iteration 237 idle-page short-circuit.

## Acceptance Criteria [0/3]

- [ ] A scripted blocked console cannot extend auto-wait to the larger socket timeout.
- [ ] Missing, hidden and unstable selectors remain distinguishable when evidence
      is available; unavailable evidence is reported honestly.
- [ ] Ordered quality gates and a dual-gate live sweep pass, with measured owned-browser
      timing and preserved navigation/auto-wait behavior.

## Out of scope

Changing the global timeout, unrelated polling loops, or skipping diagnostics silently.
