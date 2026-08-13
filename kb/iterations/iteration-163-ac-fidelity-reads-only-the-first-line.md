---
branch: iter-163/ac-fidelity-full-text-evidence
date: 2026-08-14
depends_on: []
dogfood_path: |
  # Reproduce the false positive on iter-158's own plan.
  bash tools/ralph-loop/scripts/ac-fidelity-check.sh \
    --plan kb/iterations/iteration-158-launch-lifecycle-and-harness-honesty.md \
    --base origin/main
  # → on main: 5 ❌ "ticked AC with no evidence in diff", every one of them an AC
  #   whose evidence sits on the SECOND wrapped line. After the fix: 0 failures,
  #   and the iter-61v / iter-61t replay baselines still FAIL / PASS.
status: planned
title: "Iteration 163: ac-fidelity-check's evidence heuristics read only an AC's first wrapped line"
type: iteration
tags:
  - iteration
  - tooling
---

# Iteration 163: `ac-fidelity-check`'s evidence heuristics read only an AC's first wrapped line

Carry-over from [[iteration-158-launch-lifecycle-and-harness-honesty]], filed before that PR
merges per CLAUDE.md's carry-over rule.

## The defect

`tools/ralph-loop/scripts/ac-fidelity-check.sh` builds two views of each acceptance criterion:

- `text` — the AC's **first** wrapped line (`text="${BASH_REMATCH[1]}"`, :189)
- `full_text` — the whole folded, unwrapped AC (:192)

iter-154's two additions (the non-execution wording scan and the `live_*` run-evidence
requirement) correctly use `full_text`. The three *evidence* heuristics that decide whether a
ticked AC resolves to anything in the diff — test-function slug, backtick-quoted symbol,
`::`-qualified / SCREAMING_SNAKE token — all still read `text`.

An AC whose evidence lands on the second wrapped line therefore fails with
`❌ ticked AC with no evidence in diff`, even when the symbol it names is unambiguously present.
Measured on iter-158, 2026-08-14: 5 of 8 ticked ACs failed this way, while
`grep -cF` against the very same `git diff origin/main...HEAD` found
`unit_158_port_wait_error_names_bind_timeout` ×2, `unit_158_launch_rejects_occupied_port_before_spawn`
×2, `unit_158_record_survives_failed_stop` ×2, `unit_158_single_stop_ladder_implementation` ×2,
`unit_158_no_live_test_skips_on_missing_firefox` ×2 and `AppError::User` ×29.

This is the worst possible failure mode for this particular gate. Its stated purpose is to stop
agents rewording ACs to get past checks, and CLAUDE.md says so in bold: *"NEVER reword an
acceptance criterion to make a gate pass."* A false positive here pushes an agent to do exactly
that — move a symbol onto the first line, or pad the first line with a backtick — which is the
reword reflex the gate exists to suppress.

## Secondary defect

The slug regex is `\b(live|test|bench)_[a-z0-9_]+` (:359). It does not recognise `unit_*`, which
is the prefix CLAUDE.md's own AC-checkbox convention produces for non-live tests and which
iter-158's plan used throughout. A `unit_*` AC gets no slug-existence check at all — the gate
neither verifies the named function exists nor benefits from that heuristic's evidence.

## Themes

### Theme A — the evidence heuristics read the full AC

Change heuristics 1, 2 and 3 to operate on `full_text`. Keep `text` for the one thing it is
right for: the truncated echo in the failure message.

Risk to watch: `full_text` is the folded AC *including* any `[verified: …]` /
`[deferred — new plan: …]` annotation, so a symbol appearing only inside an annotation could now
count as evidence. Strip the bracketed annotations before running the heuristics.

### Theme B — recognise `unit_*` slugs

Extend the slug regex to `\b(unit|live|test|bench)_[a-z0-9_]+` so a `unit_*` AC gets the same
"this function must exist somewhere in the workspace" guarantee the other prefixes get. This
tightens the gate; expect it to surface ACs in older plans that name a `unit_*` test which was
never written.

### Theme C — mirror both copies

`~/.claude/skills/ralph-loop/scripts/` and `tools/ralph-loop/scripts/`, plus the same file under
`~/.claude/skills/new-ralph-loop/scripts/` and `tools/new-ralph-loop/scripts/`.
`check-discipline-regression` fails on drift.

**This iteration modifies `~/.claude/skills/` and therefore cannot run through ralph-loop
itself** — drive it by hand in a regular Claude session (CLAUDE.md, "Skill-edit iterations").
That constraint is why iter-158 documented the false positive rather than fixing it inline.

## Acceptance Criteria [0/4]

- [ ] test_163_evidence_found_on_second_wrapped_line: a fixture plan whose ticked AC carries its
      only backtick-quoted symbol on the second wrapped line passes `ac-fidelity-check.sh`
      against a diff containing that symbol; the same AC fails on the pre-fix script
- [ ] test_163_annotation_only_symbol_is_not_evidence: a ticked AC whose only diff-resolvable
      symbol appears solely inside its `[verified: …]` annotation still fails, so folding in the
      annotation does not create a new escape hatch
- [ ] test_163_unit_slug_must_exist: a ticked AC naming `unit_163_does_not_exist` fails with the
      "no matching `fn` in the workspace" message, exactly as a `live_*` slug does today
- [ ] test_163_replay_baselines_unchanged: `cargo run -p xtask -- check-discipline-regression`
      still reports `61v=FAIL, 61t=PASS` and both script mirrors in sync

## Notes

- The five iter-158 ACs this defect fired on were left **ticked** rather than reworded. The
  evidence was verified by hand (`grep -cF` against `git diff origin/main...HEAD`) and the
  counts are recorded above, so this plan carries its own reproduction data.
- Related: [[iteration-158-launch-lifecycle-and-harness-honesty]] (where it was found),
  [[iteration-154-ac-fidelity-run-evidence]] if present (the `full_text` view this defect fails
  to reuse).
