---
branch: iter-154/ac-fidelity-evidence
date: 2026-08-12
depends_on:
  - kb/iterations/iteration-151-residual-live-firefox-leak.md
dogfood_path: |
  # The gate must FAIL on a plan whose ticked AC admits its test never ran:
  bash tools/new-ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/unrun-live-ac.md --base origin/main
  # → exit 1, naming the offending AC
  # The gate must still PASS on a plan whose ticked ACs carry run evidence:
  bash tools/new-ralph-loop/scripts/ac-fidelity-check.sh --plan tools/tests/ac-fidelity-check/evidenced-live-ac.md --base origin/main
  # → exit 0
  # And the pinned historical baselines must not move:
  cargo run -p xtask -- check-discipline-regression
  # → replay baselines OK (61v=FAIL, 61t=PASS)
first_call_sites: []
status: planned
title: "Iteration 154: ac-fidelity-check passes ACs whose tests never ran"
type: iteration
tags:
  - iteration
---

# Iteration 154: `ac-fidelity-check` passes ACs whose tests never ran

**This is a skill-edit iteration.** `ac-fidelity-check.sh` lives in `~/.claude/skills/` (two
copies: `ralph-loop/scripts/` and `new-ralph-loop/scripts/`) and is mirrored into
`tools/{ralph-loop,new-ralph-loop}/scripts/`. Per CLAUDE.md, skill-edit iterations **cannot run
through ralph-loop** — drive this by hand in a regular session. Edit every copy;
`check-discipline-regression` fails on mirror drift.

## The defect

`ac-fidelity-check.sh` decides a ticked AC is "backed by evidence" when its text *references* a
test slug that resolves in the branch diff, a backticked symbol present in the diff, or the
`[deferred — …]` form. It never establishes that the named test **ran**, because it only ever
sees a diff — the test function existing in `+` lines is the whole proof.

Observed on 2026-08-12, PR #188 ([[iteration-151-residual-live-firefox-leak]]). Two ACs were
ticked whose own text said:

> `live_151_chunk_a_leaves_no_orphans` … *"implemented and compiled; gated behind
> `FF_RDP_LIVE_SUITE_CHECK=1` … and **not exercised end-to-end in this session's time budget**"*

`ac-fidelity-check`: **PASS**. `check-iteration-ready`: **11/11 PASS**. The slugs were present
in the diff, so the heuristic was satisfied by a plan that openly documented its own
non-execution. The ACs were unticked by hand in review and only earned back after the
post-batch sweep produced real numbers (`16f8c8c`).

So a ticked AC currently certifies *"someone wrote a plausible sentence and added a function
with a matching name"*. CLAUDE.md says an AC without a named test is not done; the gate enforces
the naming and nothing else, while reading — to anyone glancing at a green
`check-iteration-ready` — as though it enforced the doing.

## What is and is not achievable

Be honest about the ceiling: **a diff-reading script cannot verify a test ran.** Do not design
toward that. Two things *are* achievable, and one is a documentation fix:

1. Catch the self-incriminating case. The iter-151 ACs said so in plain text. A gate that fails
   on a ticked AC containing "not exercised", "not run", "never run", "implemented and compiled",
   "not verified", "could not run", "out of time budget" (etc.) would have caught the real
   regression that motivated this plan, and costs nothing.
2. Require positive run evidence for live ACs specifically. A ticked AC naming a `live_*` test is
   the high-risk case (live tests are `#[ignore]`-gated and never run in CI, so nothing else in
   the pipeline will ever execute them). Require such an AC to carry a machine-checkable evidence
   annotation, and fail when it does not.
3. Stop the gate overstating itself. Its PASS line and its `--help` should say what it checked.

## Themes

### Theme A — fail on self-declared non-execution

Add a denial list to `ac-fidelity-check.sh`: a **ticked** AC whose text matches a
non-execution phrase fails, regardless of whether its slug resolves in the diff. Case-insensitive.
The remedy the message should suggest is untick, or annotate `[deferred — new plan: …]`.

Keep the list short and literal; do not attempt sentiment analysis. Start from the phrasings
actually observed: `not exercised`, `not run`, `never run`, `not executed`, `implemented and
compiled`, `not verified`, `could not run`, `time budget`.

This is the theme that would have caught iter-151. Do it first, and confirm it does by replaying
that plan's pre-fix state (see Theme D).

### Theme B — live ACs must carry run evidence

For a ticked AC naming a slug matching `live_*`, require an evidence annotation in the AC text,
e.g.:

```
- [x] live_151_chunk_a_leaves_no_orphans: … — [verified: 2026-08-12, 109 passed / 0 failed,
      0 orphans, main @ ae0fa44]
```

Decide the exact form and record it in [[decision-log]]; a bare `[verified: …]` with a
non-trivial payload (date + a measured quantity) is enough — the point is that a human had to
paste a real result, not that the script can validate the number. Fail a ticked `live_*` AC with
no such annotation, with a message naming the required form.

Weigh the cost honestly before committing: this makes every live AC noisier, and its whole value
is friction. If the review concludes the friction is not worth it, say so in the Resolution and
implement only Themes A and C rather than shipping a rule nobody follows.

### Theme C — stop the gate claiming more than it checks

`ac-fidelity-check.sh`'s success line currently reads as a general endorsement. Reword it to
state its actual scope — that it verified each ticked AC *references* resolvable evidence, and
that it cannot and does not verify any test was executed. Same in the script header comment and
in `CLAUDE.md`'s description of the gate, which today says ticked ACs must be "backed by test
evidence" without qualifying what the automated check can see.

Cheap, and it is the part that stops the next person trusting a green gate the way this session's
review agent did.

### Theme D — regression fixtures, and do not move the pinned baselines

`check-discipline-regression` replays two real merged plans and requires `61v=FAIL`, `61t=PASS`.
Any heuristic change must keep both. Run it before and after.

Add fixtures under `tools/tests/ac-fidelity-check/` (the existing convention —
see `tools/tests/lint-dogfood-script/` and `tools/tests/branch-protection/`):

- `unrun-live-ac.md` — a ticked live AC whose text admits non-execution → gate must FAIL
- `evidenced-live-ac.md` — the same AC carrying a `[verified: …]` annotation → gate must PASS
- `deferred-ac.md` — the existing `[deferred — new plan: …]` form → still PASS (guards against
  Theme A's denial list swallowing a legitimate deferral)

## Acceptance Criteria [0/4]

- [ ] shell_154_unrun_ac_fails: `ac-fidelity-check.sh` exits 1 on
      `tools/tests/ac-fidelity-check/unrun-live-ac.md`, and its output names the offending AC and
      suggests untick-or-defer
- [ ] shell_154_evidenced_ac_passes: the same script exits 0 on
      `tools/tests/ac-fidelity-check/evidenced-live-ac.md` and on
      `tools/tests/ac-fidelity-check/deferred-ac.md` — a legitimate deferral is not caught by
      Theme A's denial list
- [ ] shell_154_iter151_prefix_would_have_failed: replaying iteration-151's **pre-fix** AC block
      (the two chunk ACs ticked with "not exercised end-to-end" text, recoverable from `6d07c8c`)
      makes the gate exit 1 — proving this iteration fixes the case that motivated it, rather
      than a case invented to be catchable
- [ ] check_154_baselines_unmoved: `cargo run -p xtask -- check-discipline-regression` still
      reports `61v=FAIL, 61t=PASS` and both mirrors in sync after the change

## Notes

- Edit **all four** copies of `ac-fidelity-check.sh` (`~/.claude/skills/ralph-loop/scripts/`,
  `~/.claude/skills/new-ralph-loop/scripts/`, and both `tools/` mirrors). The 138–142 batch shipped
  a fix to one mirrored copy and silently missed the unmirrored one, and an iteration plan got
  reworded to route around the resulting false failure — that is the failure mode this repo added
  `check-discipline-regression` to prevent.
- Do not weaken the existing heuristics while adding these. The gate's current checks are useful;
  the problem is what it does *not* check plus what it *implies* it checked.
- **Related, deliberately out of scope — should be its own iteration.** A live test that skips
  (early `return` when `FF_RDP_LIVE_TESTS` / `FF_RDP_LIVE_NETWORK_TESTS` is unset) is counted by
  libtest as **passed**, not ignored. Measured 2026-08-12: with only `FF_RDP_LIVE_TESTS=1`, nine
  files / eighteen tests under `crates/ff-rdp-cli/tests/live/` silently no-op and report green,
  including the known-red `live_109_throttle_block::live_block_url_pattern`. So a "green live
  sweep" can mean "did not run". That is the same *green-means-nothing* family as this plan but a
  different mechanism in different code (test harness, not the gate), and folding it in here would
  make a hand-driven skill-edit iteration into a test-harness refactor. File separately.
- Verify on the wire before fixing: across 135–151 the stated root cause diverged from reality at
  least eight times. Here that means replaying the actual iter-151 plan text through the actual
  script, not reasoning about what the regex ought to do.
</content>
