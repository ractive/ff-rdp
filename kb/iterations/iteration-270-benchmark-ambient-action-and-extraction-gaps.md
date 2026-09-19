---
title: 'Iteration 270: ambient guidance, action fidelity and remaining benchmark extraction gaps'
type: iteration
status: planned
date: 2026-09-13
branch: iter-270/benchmark-ambient-action-and-extraction-gaps
depends_on: [256]
dogfood_path: |
  Read iteration 256's retained 84 trajectories and reproduce only the documented
  command/query paths on an exclusively owned Firefox with this checkout's ffrdp
  helper. Record the proposed guidance and validation design before changes.
  A new paid comparison requires a separately bounded plan and authorization;
  never replace the original baseline or SessionStart rows.
first_call_sites: []
tags: [iteration, carry-over, benchmark, agent-ergonomics]
---

# Iteration 270: preserve useful browser-first guidance without losing action fluency

Filed from iteration 256's single baseline 42 + real SessionStart 42 measurement
on 2026-09-13. Original source/harness was `8c400376aa1d8ee85bc0bcee2a7bb2a898a9e277`;
full tables, exact first-tool IDs, original/delivered argv, raw tool results and
costs live under `.git/ralph-loop/20260912-validation-efficiency/iter256-measurement/`
and [[axi-benchmark-comparison]]. No implementation is authorized in the current
252–257 queue.

## Measured problems

- Successful browser-first calls increased from 1/42 to 37/42, while average turns
  increased from 5.476 to 6.976. Both conditions scored 41/42 upstream passes.
  `--with-page` adoption fell from 40/42 to 5/42. The actual 533-byte hook names
  a11y, page-text and console, but provides no navigate-with-page or click/type
  syntax. This is a candidate explanation to test, not proof that default-on
  output would improve outcomes. The two matrices ran sequentially, not interleaved.
- The baseline click-through means were infobox 6.667/link 4.000/search 6.333;
  treatment 9.000/10.333/12.667. Only baseline link-follow met ≤ 5. Infobox-specific
  query/ref discovery remains with [[iteration-269-infobox-discovery-after-fact-refs]].
- Baseline tabular/deep extraction means6.667/7.667 missed ≤ 6; treatment 5.333/7.667
  met only tabular. Every one of these extraction runs used `--query`; compound
  literal misses and fallback extraction remain after adoption. GitHub issue
  investigation baseline run 2 omitted one full title and failed; treatment passed 3/3.
- Command errors were6 in 5 baseline runs and19 in 14 treatment runs. Examples:
  nonexistent content/js/nav/tab-new/find-ref(s), unsupported --url/--expression/
  --value, positional ref/type confusion, empty refs and guessed CSS selectors.
  Preserve each exact trace; do not add aliases or change syntax merely to legalize
  agent mistakes. Baseline search 1's hidden-button timeout and treatment infobox 3's
  empty-ref error are masked by shell recovery, so upstream error counts5/18 omit them.
- Task fidelity differs from answer grading. All six Makefile runs skip the initial
  repository-root navigation. All three treatment link-follow runs use direct URL
  navigation after failed clicks (raw grades PASS/FAIL/PASS). Treatment infobox 2/3
  also substitute URLs (PASS/PASS). Baseline search 1 types into the box but reaches
  the article through a direct Special:Search URL after its click times out (PASS).
  No historical score is to be rewritten; stricter observations belong beside it.

## Tasks [0/4]

- [ ] Reproduce the retained command/query failures and explain the actual hook/help
      information each agent saw; separate invalid syntax, valid zero matches,
      truncated output and correct product errors before proposing changes.
- [ ] Propose a bounded improvement to the opt-in home/SessionStart next steps,
      including correct navigate/act-and-see/ref/type syntax where useful; preserve
      compact output, literal/regex query compatibility and existing defaults.
      Coordinate infobox evidence with269 without absorbing its scope.
- [ ] Define and validate an action-fidelity/error audit that retains upstream
      grades, first actual top-level tool IDs, first-command failures and all paid
      invocation failures; benchmark helpers must not hide omitted steps or errors.
- [ ] Implement only the evidence-supported scoped outcome, or document why no
      product change is warranted; add appropriate regressions and retain measured
      target misses. Specify controlled validation before requesting any new paid run.

## Acceptance Criteria [0/4]

- [ ] Every listed failure and numeric target miss has a trace-backed explanation
      and a bounded disposition; historical256 results remain unchanged.
- [ ] Guidance and any implementation preserve current command/query/default
      contracts and correct browser ownership; broader changes require a separate
      explicit design decision.
- [ ] Validation reports action fidelity, observed errors, first-command success,
      extraction adoption, task means, original model/prompt/CLI identities and
      separate agent/judge list costs; no missing or failed row becomes a cheap success.
- [ ] Required ordered gates and relevant live evidence pass for any product change;
      remaining requirements stay unticked with their actual dispositions.

## Out of scope

Executing this follow-up during252–257; rerunning or replacing the84 measurements;
turning on --with-page or hooks by default; rewriting upstream tasks/grades to
remove required actions; repairing screenshot or daemon lifecycle defects here.

## Implementation preflight — 2026-09-19

Dependency256 is delivered;254 already supplied installation-target support. Audit the actual trimmed533-byte hook payload and retained trajectories, not merely full home output or shared-idiom unit tests. Preserve the measured distinctions: browser-first1/42 to37/42, with-page40/42 to5/42, mean turns5.476 to6.976, and both grades41/42 despite action-fidelity problems. These samples support investigation, not a causal default-on decision. Keep guidance opt-in and coordinate269's overlapping action/error audit. No new paid comparison, smoke, model probe or judge is authorized; use retained traces and ordinary local reproduction, leaving any paid measurement explicitly pending.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.
