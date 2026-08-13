---
branch: iter-156/ac-fidelity-names-its-test
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-154-ac-fidelity-evidence.md
  - kb/iterations/iteration-155-live-skip-reports-green.md
dogfood_path: |
  # An AC naming an ordinary CI unit test must NOT be forced to carry [verified: …]
  # merely because its prose mentions a live_* test:
  bash tools/new-ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/prose-mentions-live-ac.md --base origin/main
  # → exit 0
  # An AC that genuinely names a live_* test as its own test still requires evidence:
  bash tools/new-ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/unevidenced-live-ac.md --base origin/main
  # → exit 1
  # And the pinned historical baselines must not move:
  cargo run -p xtask -- check-discipline-regression
  # → replay baselines OK (61v=FAIL, 61t=PASS)
first_call_sites: []
status: obsolete
title: "Iteration 156: ac-fidelity Theme B fires on any live_* token, not the test the AC names"
type: iteration
tags:
  - discipline
  - tooling
---

# Iteration 156: Theme B fires on prose, not on the AC's own test

> **CLOSED OBSOLETE 2026-08-13.** This fixes friction in `ac-fidelity-check.sh`, a gate that
> [[analysis-2026-08-13-what-ff-rdp-became]] §5 recommends shrinking to a single rule and
> eventually replacing outright — see [[iteration-162-discipline-machinery-removal]] Phase 3.
> Refining a heuristic that is scheduled for deletion is the exact spiral the step-back was
> commissioned to stop. The underlying observation stands and is recorded in the analysis;
> the fix is subsumed by the shrink.

**This is a skill-edit iteration.** `ac-fidelity-check.sh` has four copies
(`~/.claude/skills/{ralph-loop,new-ralph-loop}/scripts/` and both `tools/` mirrors). Per
CLAUDE.md these cannot run through ralph-loop — drive it by hand.

## The defect

[[iteration-154-ac-fidelity-evidence]] Theme B requires a ticked AC naming a `live_*` test to
carry `[verified: <YYYY-MM-DD>, <measured result>]`. The implementation keys on **any** `live_*`
token anywhere in the AC's folded text:

```sh
live_slugs=$(printf '%s' "$full_text" | grep -oE '\blive_[a-z0-9_]+' || true)
```

It cannot distinguish *the test this AC names* from *a live test this AC talks about*.

Observed 2026-08-13, one day after Theme B shipped, on PR #194
([[iteration-155-live-skip-reports-green]]). All three of that plan's ACs name `test_155_*`
tests — ordinary unit tests that run on every `cargo test --workspace` in CI — but their text
references `live_109_throttle_block::live_block_url_pattern` as the *subject under test*. Theme B
fired on the prose and demanded run-evidence annotations for tests CI had already executed.

The implementing agent complied honestly: the `[verified: …]` payloads it added contain real
measured numbers. The cost was not a false green — it was **ceremony**, and worse, it taught the
agent that the remedy for a mis-firing gate is to satisfy it rather than report it. On the same
PR the agent also rewrote a test path from `live_109_throttle_block::live_block_url_pattern` to
`live_109_throttle_block ⋅ live_block_url_pattern` — a Unicode substitution that changed no gate
outcome (verified: both forms yield identical slugs to every heuristic) and only made a Rust path
non-copy-pasteable. That is regex-dodging reflex, and a mis-firing check is what trains it.

Any iteration *about* live-test infrastructure trips this. [[iteration-152-live-guard-coverage-sweep]]
is next in line.

## What is achievable

Theme B's intent is sound and stays: live tests are `#[ignore]`-gated and never run in CI, so an
AC that rests on one needs a human-pasted result. The bug is purely in **which slug the rule reads**.

## Themes

### Theme A — key on the AC's own test, not on every token

The repo's AC convention (CLAUDE.md, "AC checkbox convention") is `- [x] <test_name>: <asserted
post-condition>` — the AC's own test is the **leading** slug of its first line. Restrict Theme B's
trigger to that slug; treat later occurrences as prose.

Decide and record in [[decision-log]] which rule to use, and be honest that the convention is not
universally followed in the existing 155 plans:

1. Leading slug of the AC's first line only.
2. Leading slug, plus any slug in a backticked `` `live_*` `` code span on the first line.
3. Any slug on the **first line** (not continuations) — looser, closer to today's behaviour.

Measure before choosing: run each candidate over all merged plans and report how many ACs change
verdict. A rule that silently reclassifies dozens of historical ACs is the wrong rule even if it
fixes iter-155's case. **Do not pick from reasoning alone** — the iter-154 review round found two
regexes that were obviously right and were not.

### Theme B — an escape hatch for the residual

Whatever rule Theme A picks will still misfire somewhere. iter-154 shipped
`[allow-ac-wording: <reason ≥10 chars>]` for the analogous Theme A false positive; the
run-evidence check has no equivalent. Add one (reuse the same annotation, or a
`[allow-ac-live-mention: <reason>]` sibling — decide and record which, and why one marker for two
distinct checks is or is not confusing).

### Theme C — do not weaken what works

The genuine catch must survive: an AC that really does name a `live_*` test as its own test and
carries no evidence still fails. `tools/tests/ac-fidelity-check/unevidenced-live-ac.md` pins this
today and must keep failing. Likewise the pinned `61v=FAIL / 61t=PASS` replay baselines.

## Acceptance Criteria [0/4]

- [ ] shell_156_prose_mention_does_not_require_evidence: `ac-fidelity-check.sh` exits 0 on a new
      fixture `tools/tests/ac-fidelity-check/prose-mentions-live-ac.md` — a ticked AC naming a
      `test_*` unit test whose body text mentions a `live_*` test — with no `[verified: …]`
      annotation present
- [ ] shell_156_named_live_test_still_requires_evidence: the existing
      `tools/tests/ac-fidelity-check/unevidenced-live-ac.md` still exits 1, and
      `evidenced-live-ac.md` still exits 0 — the genuine catch is unweakened
- [ ] shell_156_iter155_acs_pass_unannotated: iteration-155's three ACs, with their
      `[verified: …]` annotations **stripped**, pass the fixed gate — proving this iteration fixes
      the case that motivated it rather than one invented to be catchable (recover the pre-fix AC
      text from `f76684b^`)
- [ ] check_156_baselines_unmoved: `cargo run -p xtask -- check-discipline-regression` still
      reports `61v=FAIL, 61t=PASS` and all eight mirrored files in sync

## Notes

- Edit **all four** copies of `ac-fidelity-check.sh`; `check-discipline-regression` catches drift.
- Report the merged-plan verdict-change count from Theme A's measurement in the Resolution, even
  if it is zero. "I chose the rule that changed the fewest historical verdicts" is a finding;
  "I chose the obvious rule" is not.
- Related but separate: [[iteration-157-live-sweep-classifier-drift]] covers DEC-031's residual.
