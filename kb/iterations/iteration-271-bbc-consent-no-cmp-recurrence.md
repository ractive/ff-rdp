---
title: "Iteration 271: diagnose recurring BBC consent_no_cmp"
date: 2026-09-14
type: iteration
status: planned
branch: iter-271/bbc-consent-no-cmp-recurrence
tags: [iteration, carry-over, consent, live-tests]
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_144_session_hygiene_followup::live_144_bbc_cmp_dismissed -- --include-ignored --exact --nocapture --test-threads=1
  # Before choosing a fix, retain an attributable failed occurrence's navigate envelope,
  # current URL, document identity/readiness and real banner DOM on an owned browser.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
---

# Iteration 271: BBC consent detection fails in the sweep and exact isolation

Filed from the separately owed242 sweep at mergedmain
`19f4e236a70399c2984e6d46eb15bdac37a55181`. No271 implementation is authorized in the252–257
queue.

## Measured observations

`live_144_session_hygiene_followup::live_144_bbc_cmp_dismissed` failed after
navigation to `https://www.bbc.com/news` returned success, when `consent accept`
returned exit1, `error_type: consent_no_cmp`, `cmp: null`, `action: null`,
`status: no_cmp_detected`. The same exact dual-gate serial test failed again
in5.37s (command wall5.59s), with the same envelope.
Sweep Firefox58160/debug55257 is recorded in the launch log; both tests use
the direct route. Failed-occurrence navigate envelopes, current URL, document
identity/readiness and banner DOM were not retained. Successful navigation
alone therefore does not distinguish a delayed banner, a site/region variation,
a changed adapter contract or a different loaded document. No cause is claimed.

Original144's verified behavior was a real native BBC adapter click on
`#bbccookies-continue-button`, followed by proof that its control was gone.
Preserve that history; neither `--allow-no-cmp` nor accepting a null action
establishes dismissal. This failure is distinct from262's Sourcepoint
`detected_not_actioned` and target-promotion observations and from242's HN
empty-title mechanism, even though each touches a third-party page.

Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/sweep-failures.txt`,
`isolated-live_144_session_hygiene_followup__live_144_bbc_cmp_dismissed.log`,
`sweep-live-launches.log` and `isolation-results.jsonl`.

## Tasks [0/4]

- [ ] Reproduce with an exclusively owned browser and retain the failing navigate,
      URL/document/readiness and banner DOM evidence, including language/region,
      profile/consent state and the actual route; retain passing controls too.
- [ ] Identify whether detection, readiness or the real site's contract explains
      the error, without inferring a cause from test timing or an isolated pass.
- [ ] Implement only the demonstrated scoped correction, or record evidence that
      the current product behavior is correct and explicitly decide the real-site
      test contract; do not silently weaken actual-dismissal assertions.
- [ ] Add a meaningful bounded regression for the chosen contract, verify against
      real Firefox/site evidence, and run the required ordered gates and closing
      dual-gate sweep with every unrelated failure dispositioned.

## Acceptance Criteria [0/4]

- [ ] An attributable failing occurrence explains the BBC no-CMP result with actual
      page/readiness/banner evidence; the two original failures remain recorded.
- [ ] The chosen behavior distinguishes no banner from a failed dismissal and does
      not claim acceptance without an observed action and post-action evidence.
- [ ] A regression fails without the demonstrated correction and passes with it,
      or a no-product-change outcome is justified by measured site/test-contract
      evidence with the original requirement explicitly retained as unmet if needed.
- [ ] Real-Firefox verification and the required ordered gates/sweep are recorded;
      remaining failures have explicit owners, with no retry-only masking.

## Out of scope

Executing this plan in the252–257 takeover; changing unrelated consent adapters;
claiming this is262's Sourcepoint cause; changing the screenshot fix or weakening
BBC assertions simply to make the sweep green.

## Iteration257 additional recurrence, 2026-09-14

The screenshot repair's distinct344-name closing sweep failed the same BBC
test with `consent_no_cmp`, `cmp:null`, `action:null`, `no_cmp_detected` on the
direct route (Firefox54579/debug56165). Exact serial dual-gate isolation failed
again in4.77s (Firefox74544/debug49201) with the same envelope. All seven257
screenshots and its new guard passed; this is not a screenshot regression.
As before, failed-occurrence page/banner DOM and document identity were not
retained, so the original attributable-cause requirement remains unmet. No271
implementation, cause, test weakening or new consent behavior is claimed.
Guardian's concurrent daemon-route no-CMP result is separately preserved under262;
the shared error type does not establish a shared mechanism.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/sweep.log`,
the exact isolated144 log and both launch logs. Original tasks/ACs stay untouched.
