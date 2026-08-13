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
status: done
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

## Acceptance Criteria [5/5]

- [x] `shell_154_unrun_ac_fails`: `ac-fidelity-check.sh` exits 1 on
      `tools/tests/ac-fidelity-check/unrun-live-ac.md`, and its output names the offending AC and
      suggests untick-or-defer
- [x] `shell_154_evidenced_ac_passes`: the same script exits 0 on
      `tools/tests/ac-fidelity-check/evidenced-live-ac.md` and on
      `tools/tests/ac-fidelity-check/deferred-ac.md` — a legitimate deferral is not caught by
      Theme A's denial list
- [x] `shell_154_missing_run_evidence_fails`, `shell_154_blank_line_continuation_is_read`,
      `shell_154_deferral_mention_does_not_launder`, `shell_154_allow_wording_escape_hatch`,
      `shell_154_future_verified_date_rejected`,
      `shell_154_theme_a_sees_nested_and_lazy_continuations`: the six defects PR #193's review
      found in the first cut each have a fixture or sandbox plan pinning them, and all 11 tests
      in `crates/xtask/tests/ac_fidelity_check.rs` pass
- [x] `shell_154_iter151_prefix_would_have_failed`: replaying iteration-151's **pre-fix** AC block
      (the two chunk ACs ticked with self-declared non-execution wording, recoverable from
      `6d07c8c`) makes the gate exit 1 — proving this iteration fixes the case that motivated it,
      rather than a case invented to be catchable
- [x] check_154_baselines_unmoved: after the `ac-fidelity-check.sh` change,
      `cargo run -p xtask -- check-discipline-regression` still
      reports `61v=FAIL, 61t=PASS` and both mirrors in sync after the change

## Resolution

All three themes shipped, plus a structural fix the plan did not anticipate.

**The plan's diagnosis was incomplete.** It attributed the iter-151 pass to the slug heuristic
alone. Replaying `6d07c8c` through the pre-fix script (exit 0, confirmed on the wire before any
edit) showed a second, independent cause: the script's loop matched `- [x] <text>` line by line,
so the AC's *continuation* lines — where the wording about non-execution actually lived — were
never read at all. A denial list bolted onto the old loop would have caught nothing. So the AC
block is now folded (bullet + indented continuations, whitespace collapsed) before checking.
The evidence heuristics deliberately keep reading only the first line: widening their input
hands them more tokens to match and can turn a should-fail plan green, which is what the pinned
`61v=FAIL` baseline guards. Only the two new checks — which can only ever add failures — read
the folded text.

**Theme A** (denial list, eight literal phrases, case-insensitive) fires after the
`[deferred — …]` short-circuit, so a legitimate deferral still passes even though it necessarily
carries the same wording. `deferred-ac.md` pins that.

**Theme B shipped**, against the plan's invitation to drop it. The argument that decided it:
Theme A alone rewards silence. It catches a *confession*, and once it exists the honest-but-ticking
case becomes the silent-ticking case — and this repo has already watched a plan get reworded to
route around a gate. Theme B requires an *assertion*: `[verified: <YYYY-MM-DD>, <measured
result>]`, ISO date plus at least one further digit in the bracket. It is forgeable, like every
check a diff-reading script can make; the cost is bounded by restricting it to `live_*` ACs,
where the risk actually is (live tests are `#[ignore]`-gated, so nothing downstream ever runs
them). Recorded as [[decision-log]] DEC-030. Note the fixtures show Theme B doing work Theme A
cannot: iter-151's chunk-B AC never used a denied phrase — it said "same status as
`live_151_chunk_a_leaves_no_orphans` above" — and is caught only by the missing run evidence.

**Theme C**: the PASS line, the script header/`--help`, `CLAUDE.md` and `CONTRIBUTING.md` now
state that the gate reads a plan and a diff and cannot verify a test was executed. Confirmed
there is no run log it could consult instead: neither ralph-loop nor new-ralph-loop ever invokes
`cargo test-live`.

**What this does not fix.** A diff-reading script still cannot verify a test ran; the ceiling in
the plan's "What is and is not achievable" stands. Both new checks are satisfiable by an agent
willing to type a false sentence.

### Review round (PR #193, 2026-08-13)

An independent review of the first cut found six real defects, all reproduced before fixing.
Two were worse than the bug being fixed:

1. **The widened `[deferred` accept was a strictness regression.** It reads the folded text and
   `continue`s past every later check, so an unanchored substring match let any AC that merely
   *mentioned* `[deferred` launder itself — a plan that failed *before* iter-154 passed after it.
   Now anchored to the end of the AC (trailing period tolerated, closing `)` not), and mentions
   inside backticks are excluded. This also falsified the script comment and DEC-030's claim that
   "only the two new checks read the folded text"; both corrected to name three consumers.
2. **The denial list was plain substrings**, so it fired on ordinary wording: `not run` inside
   "not run*ning*" / "can*not run*", and `time budget` on any latency AC. Now whole-word ERE;
   `time budget` dropped (it earned nothing — iteration-151's AC is caught by `not exercised` in
   the same sentence); `could not run` folded into `not run`. The residual class — an AC that
   legitimately *describes* behaviour — gets `[allow-ac-wording: <reason ≥10 chars>]`. **The
   review's sharpest point: this PR had already reworded its own AC 3 to get past its own gate.**
   A check that punishes honest wording teaches agents to launder wording, so the escape hatch is
   load-bearing, not a convenience.
3. **A blank line before a continuation dropped it**, rejecting an AC that *did* carry
   `[verified: …]` in a second paragraph of the same list item.
4. **Two Theme A bypasses**: a column-0 lazy continuation, and a confession parked under an
   unticked sub-checkbox. Folding now follows Markdown's own list-item semantics, in two passes.
5. **The remedy message omitted the only correct fix for a false positive**, so an agent
   following it literally would untick or defer a completed AC.
6. **The loop's own driver prompts still said "soften the AC text"** — the exact behaviour this
   gate forbids — and never mentioned `[verified: …]`. Fixed in all four mirrored locations
   (`run-iteration.sh`, `ralph.workflow.js`), so future iterations do not meet Theme B blind.

Also: `[verified: 9999-99-99, 0]` was accepted (now calendar-shaped and not in the future);
`CLAUDE.md`'s canonical AC example failed the gate once ticked (now shows the annotation); and
`awk -v` escape-processing silently rewrote `\[` to `[`, so the first folding regex matched
nothing and every check was skipped — caught only because the fixtures were re-run rather than
assumed. Regexes are now passed via `ENVIRON`.

Copilot raised `\b` portability in `grep -oE`; **not reproduced** — stock `/usr/bin/grep` on
macOS and GNU grep both honour it, and it matches the pre-existing heuristic-1 pattern. Its other
finding (no test for the Theme B diagnostic) duplicated the local review's.

**Verification** (2026-08-13): pre-fix replay of `6d07c8c` exits 0; post-fix exits 1 naming
`live_151_chunk_a_leaves_no_orphans` on its non-execution wording. All four copies of the script
are byte-identical. `check-discipline-regression`: mirrors in sync (3 + 5 files), replay baselines
OK (61v=FAIL, 61t=PASS). 11/11 tests in `ac_fidelity_check.rs` pass.

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
  make a hand-driven skill-edit iteration into a test-harness refactor. Filed as
  [[iteration-155-live-skip-reports-green]] (2026-08-13), before this PR merged, per
  CLAUDE.md's carry-over rule.
- Verify on the wire before fixing: across 135–151 the stated root cause diverged from reality at
  least eight times. Here that means replaying the actual iter-151 plan text through the actual
  script, not reasoning about what the regex ought to do.
</content>
