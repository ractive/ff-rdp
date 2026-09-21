---
title: "Iteration 275: Guardian consent failures with ready daemon targets"
type: iteration
date: 2026-09-19
status: planned
branch: iter-275/guardian-consent-ready-target-failures
first_call_sites: []
dogfood_path: |-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_137_daemon_mode_parity::live_137_consent_accept_via_daemon -- --ignored --exact --nocapture --test-threads=1
  # Retain failed-occurrence document identity, daemon route, target forms,
  # top-level DOM and the actual CMP-frame DOM before cleanup.
tags: [iteration, carry-over, consent, live-tests]
---

# Iteration 275: Guardian consent failures with ready daemon targets

Filed, not executed, by [[iteration-262-daemon-live-target-never-promoted]].
This owns the separate ready-target consent outcomes; it does not own target
lifecycle loss, and filing it does not satisfy 262's three-green-sweeps AC.
[[iteration-271-bbc-consent-no-cmp-recurrence]] owns BBC, not these Guardian cases.

## Evidence

At source `38add406f711434f520fa06494b2e02ab3f5e0b2`, Firefox 156,
September 19, 2026, the bounded 262 investigation ran twelve instances of the
named 137 test: six serial, then six concurrently. All original assertions
were unchanged; probes 2–12 added failure-only diagnostics before cleanup.

- Probes 1, 4, 5, 6, 7 and 10 reached live targets, then returned
  `consent_not_actioned`, Sourcepoint `detected_not_actioned`. Probe 4 reached
  targets in 70ms on debug51940, then failed. Its retained top document was
  `https://www.theguardian.com/europe`, `readyState=complete`, with
  `<html lang="en" data-previous-scroll-y="-0px" class="sp-message-open">`.
  Its wire trace includes the CMP frame identity and the failed accept-control
  evaluation. The **CMP frame's own DOM was not captured**; do not infer a
  changed selector or label from the top-level page alone.
- Probe 2 reached targets in 24ms on debug51827, then returned `consent_no_cmp`.
  A subsequent failure-time snapshot reached that same Guardian URL through
  the daemon with `readyState=complete` and `<html lang="en">`. Iframe attributes
  and lifecycle packets are retained. Snapshot time follows the failed command;
  this does not establish what was present during the detection attempt.
- Probes 3 and 9 passed. Three other probes failed target readiness before
  consent and remain owned by 262. Isolation success diagnoses neither case.

Artifacts: `.git/ralph-loop/20260919-queue/iter262/`, `probe-N.log`,
`home-N/.ff-rdp/daemon.log`, `derived/page-N.json`, and
`diagnostic-only.patch`. Exact commands, environment, start/end and exit status
are in `probe-N.meta`; diagnostics preserve the top DOM, iframe attributes,
route and status before the owned browser is cleaned up.

The current consent failure hint also suggests `ff-rdp dom --frames`, but
`dom --help` exposes no `--frames` option. Correct the recovery hint alongside
the supported inspection path when addressing this consent work.

## Tasks [0/3]

- [ ] Reproduce each outcome separately and retain the failed occurrence's CMP
      frame DOM, document identity and lifecycle timing, including any later
      change between detection and diagnostics.
- [ ] Establish each cause before changing adapter behavior or timing; retain
      the historical Sourcepoint-action and Guardian-no-CMP observations separately.
- [ ] Implement and verify only an evidence-backed repair, or document a proved
      site/environment change and the resulting explicit supported behavior.

## Acceptance Criteria [0/3]

- [ ] Each outcome has an evidence-backed explanation; unavailable evidence is
      explicit and neither error category is silently treated as success.
- [ ] Any repair has a live before/after demonstration and meaningful regression
      coverage; no assertion, return path or deadline is weakened to obtain green.
- [ ] Required ordered workspace gates and a full dual-gate live sweep pass for
      the final implementation, with all failures and profile summaries accounted for.

## Scope boundary

No new paid benchmark work. Do not execute 262's lifecycle repair or 271's BBC
adapter work from this plan. Coordinate resulting behavior/evidence back into
262, whose mandatory consecutive named-test sweeps remain unchanged.
