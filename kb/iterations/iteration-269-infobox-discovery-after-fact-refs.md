---
title: "Iteration 269: remaining infobox discovery costs after fact refs"
type: iteration
date: 2026-09-13
status: done
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

### A. Diagnose before selecting a change [3/3]
- [x] Reproduce the recorded query/filter and handle-discovery paths with an owned
      browser; distinguish intended filtering from misleading affordances.
- [x] Compare the existing way to request the Developer fact/ref against the
      routes the agents chose; retain compatibility, bounded payloads and exact
      destinations. Document the smallest proposed improvement with evidence.
- [x] Audit why the judge accepted URL navigation when the task said click, and
      why a pipeline-masked CLI error is absent from upstream error_count; retain
      historical scores and report stricter observations separately.

### B. Validate the selected outcome [2/2]
- [x] Implement a justified, bounded improvement with meaningful tests, or record
      evidence that no product change is warranted. A broader default, help or
      query-semantics change needs its own explicit design decision.
- [x] Record a controlled validation approach before any new paid run, preserving
      model/prompt identities, turn-count definition, per-run adoption/hunts,
      separate query recovery, real task actions and tool errors. Never rerun
      iteration 255 to replace its results.

## Acceptance Criteria [4/4]

- [x] The original three infobox trajectories have a reproducible explanation for
      every handle-hunt and query-recovery command, including the invalid option.
- [x] The chosen outcome preserves literal/regex compatibility and fact-link
      budgets, or explicitly documents a separately approved contract change;
      linked retained facts remain clickable and misses remain explainable.
- [x] Any new comparison reports per-run turns, grades, action fidelity, tool
      errors, model/CLI/prompt identities and separate agent/judge list costs;
      missing results cannot count as cheap successes and old grades stay intact.
- [x] The measured result is recorded whatever it is; iteration 255's 8.333 and
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

## Implementation preflight — 2026-09-19

Dependencies255/256 are delivered. Preserve fact refs, Formation matching and literal-query semantics. A stable-release-only query excluding Developer is not evidence of broken refs, and a compound literal miss does not authorize token-OR matching. Preserve255's original7/12/6 turns (mean8.333), unmet<=5 criterion and distinct256 datasets. Analyze recorded paths and reproduce ordinary owned-browser interactions, then choose a supported discovery improvement or document correct existing behavior. Coordinate overlapping action/error analysis with270. No new paid comparison, smoke, model probe or judge is authorized; any such requirement stays pending with a separately bounded proposal.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Outcome — 2026-09-19

No product change is warranted. The existing targeted route returns the Developer
fact with a registered, clickable PSF ref; the two apparent misses in the retained
traces are the documented consequences of narrower queries. Keeping the current
literal and regex contracts also preserves the bounded fact-link payload and the
zero-match explanation.

### Owned-Firefox reproduction

The reproduction used baseline `03ba1fd9e2d74193e99b3ff51d4e732b8e7b5337`,
this checkout's `ffrdp` helper, a private `$FF_RDP_HOME`, an owned Firefox on port
27106 (PID 45399), and a private Cargo target. Raw commands, JSON and cleanup
evidence are retained under
`.git/ralph-loop/20260919-queue/iter269/`.

- `navigate ... --with-page --query 'stable release'` returned only the
  `Stable release` fact. `Developer` remained visible in `query_fact_keys`; its
  absence from the retained result is intended filtering, not a lost link.
- `page-text --query 'Python Software Foundation'` found the value but minted no
  refs, `a11y summary` contained no PSF entry, and positional
  `dom "a[href*='Python_Software_Foundation']"` returned seven matches with
  seven refs. Those are the value-only, absent-a11y and DOM recovery paths seen
  in the retained runs.
- `navigate ... --with-page --query 'stable release infobox developer'`
  returned `matches:0` and candidate keys including `Developer` and
  `Stable release`. The default query is one case-insensitive literal, so this
  is an explained miss rather than evidence for token-OR matching.
- `dom --selector '.infobox' | head` produced pipeline statuses `2 0`: clap
  rejected the unsupported option while `head` succeeded. Without `pipefail`,
  the shell tool result therefore appeared successful and the upstream
  `error_count` omitted the embedded CLI error.
- `navigate ... --with-page --query Developer` returned the PSF link as ref
  `e4` with `page_refs_registered:true`. `click --ref e4 --with-page --query
  'founded formed'` reported `clicked:true`, reached exactly
  `https://en.wikipedia.org/wiki/Python_Software_Foundation`, and returned the
  `Formation` fact. The owned browser stopped, port 27106 closed, and the user's
  desktop Firefox PID 1112 remained running.

### Retained trajectory and grading audit

Iteration255 run1 used a stable-release-only view, then page text, a11y and DOM
before clicking the PSF ref; run3 used the same narrow view, then page text and
DOM before its click. Run2 supplied the compound literal, tried a second compound
query, snapshot grep and invalid `dom --selector` before positional DOM worked;
it then used page text, a11y and two eval forms to recover the URL. It navigated
to that URL instead of performing the requested click. The historical judge
still returned PASS, so the raw grade remains PASS while the stricter action
observation is recorded as a click-fidelity miss. The broader action/error
accounting stays with
[[iteration-270-benchmark-ambient-action-and-extraction-gaps]].

The original infobox turns remain **7/12/6 (mean 8.333)**, the `<=5` criterion in
iteration 255 remains unticked, link-follow remains 4/4/4, and the three original
grades remain PASS. The distinct iteration 256 baseline and SessionStart datasets
are unchanged.

### Controlled validation boundary

No paid comparison, smoke, model probe or judge ran. A future paid comparison is
not justified by this outcome. If a separately approved contract change later
needs comparison, pre-register the exact old and candidate product revisions,
the original task/prompt hashes, agent and judge model identities, CLI version,
three repetitions, turn-count rule, per-run query recovery and handle hunts,
actual click-versus-navigation fidelity, every top-level and embedded tool error,
raw grade, and separate agent/judge list costs. Missing or failed rows must remain
missing or failed rather than becoming cheap successes; historical rows and
grades must not be replaced.

## Carry-over

| Observation | Disposition |
|---|---|
| Iteration 255 infobox mean 8.333 remains above `<=5`. | **No plan:** the owned reproduction found a direct supported Developer/ref/click route and no product defect or approved contract change. File a bounded new plan only if a future controlled trace uses the targeted route and still demonstrates avoidable discovery cost. |
| Run2 substituted URL navigation for the requested click, and its invalid option was shell-masked. | **Fold:** strict action and observed-versus-upstream error accounting are already owned by [[iteration-270-benchmark-ambient-action-and-extraction-gaps]]. Historical scores remain unchanged. |
| No new paid comparison was run. | **No plan:** no measurement is missing for the selected no-product-change outcome. A future approved comparison must use the controlled boundary above. |
