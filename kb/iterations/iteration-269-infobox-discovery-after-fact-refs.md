---
title: "Iteration 269: remaining infobox discovery costs after fact refs"
type: iteration
date: 2026-09-13
status: planned
branch: iter-269/infobox-discovery-after-fact-refs
depends_on: [255, 256]
first_call_sites: []
dogfood_path: |
  Reproduce the three preserved iteration-255 infobox command sequences on an exclusively owned Firefox using this checkout's ffrdp helper. Compare a stable-release-only page query, the recorded compound literal miss, and a Developer/PSF-targeted page view. Record retained facts, clickable handles and zero-match explanations before proposing a change. Preserve the original six paid runs; a new paid comparison requires its own bounded plan and authorization.
tags: [iteration, agent-ergonomics, carry-over, benchmark]
---

# Iteration 269: remaining infobox discovery costs after fact refs

Filed from iteration 255's actual six-run measurement on 2026-09-13. Infobox
turns 7/12/6 (mean 8.333) missed ≤5; link-follow 4/4/4 remained at target. All
six judges passed. This plan preserves the distinction between a measured target
miss and a broken returned handle: 255's original mechanism ACs are verified,
but no measured agent consumed the Developer fact ref.

## Evidence

Product `7045195f66d0663e70fc68c2364b8003f7d5318c`, immutable harness
`5786329668c95479a4b91710764c7cee0e782f4a`. Raw evidence and every exact command,
tool-use ID, output and classification live under
`.git/ralph-loop/20260912-validation-efficiency/iter255-measurement/`;
[[axi-benchmark-comparison]] records the per-run costs, model/CLI caveats and grades.

- Runs 1 and 3 query stable release; Developer is outside the retained facts.
  They spend 3 and 2 subsequent text/a11y/DOM commands seeking the PSF handle.
- Run 2 queries `stable release infobox developer` as a whole literal, gets
  matches 0 with the candidate keys explicitly listed, then spends four commands
  recovering the information. `dom --selector` is invalid; its pipeline masks
  the argument error in the upstream error count. Four further text/a11y/eval
  commands recover the PSF URL, followed by navigate rather than the task's click.
- Formation answers immediately for `formed` / `formed founded` in all three
  runs. The earlier destination morphology-recovery cost is absent in this sample.
- Actual CLI 2.1.270 differs from the historical 2.1.241. n=3 and that difference
  do not prove a product regression from 8.0 to 8.333; no such claim is made.

## Tasks

### A. Diagnose before selecting a change [0/3]
- [ ] Reproduce the recorded query/filter and handle-discovery paths with an owned
      browser; distinguish intended filtering from misleading affordances.
- [ ] Compare the existing way to request the Developer fact/ref against the
      routes the agents chose; retain compatibility, bounded payloads and exact
      destinations. Document the smallest proposed improvement with evidence.
- [ ] Audit why the judge accepted URL navigation when the task said click, and
      why a pipeline-masked CLI error is absent from upstream error_count; retain
      historical scores and report stricter observations separately.

### B. Validate the selected outcome [0/2]
- [ ] Implement a justified, bounded improvement with meaningful tests, or record
      evidence that no product change is warranted. A broader default, help or
      query-semantics change needs its own explicit design decision.
- [ ] Record a controlled validation approach before any new paid run, preserving
      model/prompt identities, turn-count definition, per-run adoption/hunts,
      separate query recovery, real task actions and tool errors. Never rerun
      iteration 255 to replace its results.

## Acceptance Criteria [0/4]

- [ ] The original three infobox trajectories have a reproducible explanation for
      every handle-hunt and query-recovery command, including the invalid option.
- [ ] The chosen outcome preserves literal/regex compatibility and fact-link
      budgets, or explicitly documents a separately approved contract change;
      linked retained facts remain clickable and misses remain explainable.
- [ ] Any new comparison reports per-run turns, grades, action fidelity, tool
      errors, model/CLI/prompt identities and separate agent/judge list costs;
      missing results cannot count as cheap successes and old grades stay intact.
- [ ] The measured result is recorded whatever it is; iteration 255's 8.333 and
      unticked ≤5 criterion remain unchanged.

## Out of scope

Executing this plan during the resumed 252–257 queue; changing 255 source to
chase its score; replacing the six-run record; claiming zero CLI errors from
upstream error_count; changing --with-page defaults before 256's decision;
screenshot, daemon promotion, pre-auth or post-auth transport repairs.

## References

- [[iteration-255-infobox-facts-refs-and-query-matching]]
- [[iteration-256-act-and-see-benchmark-rerun]]
- [[axi-benchmark-comparison]]

## Iteration 256 reconciliation, 2026-09-13

The separate final256 baseline infobox runs 7/7/6 (mean6.667) used --with-page
and --query in all three, but still searched for the PSF handle (3/3/2 commands);
no Developer fact ref was consumed. All three PSF clicks succeeded and their
post-action Formation answer needed no recovery. The real SessionStart runs
11/9/7 (mean9.000) all used --query but only run 1 used --with-page. Run1 eventually
clicked a CSS selector; runs 2/3 directly navigated to PSF, with run 3 masking an
empty-ref error via shell recovery. All three raw judges passed. Runs2/3 queried
page-text with "founded formed" then recovered through "2001", so their destination
recovery is not evidence against255's post-action fact-key mechanism.

All six still miss ≤ 5. Preserve the original255 table and its target; this is an
additional dataset with its own model/argv/hook evidence in 256's measurement
artifacts. Existing diagnosis tasks cover these infobox paths unchanged. The
broader hook/action/other-extraction follow-up is
[[iteration-270-benchmark-ambient-action-and-extraction-gaps]]; neither plan is
executed by this reconciliation.
