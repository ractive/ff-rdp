---
title: "Why ff-rdp's discipline rules are what they are"
type: reference
date: 2026-08-16
status: current
tags:
  - discipline
  - rationale
---

# Why ff-rdp's discipline rules are what they are

`CLAUDE.md` carries the rules as one-liners so they cost little context. This file carries the
evidence behind each one, so the reasoning survives without being re-read on every turn. Extracted
from `CLAUDE.md` on 2026-08-16, when 146 of its 206 lines were end-of-iteration procedure and
archaeology. The procedure went to the `iteration-close` skill; the archaeology is here.

See also [[analysis-2026-08-13-what-ff-rdp-became]] (the step-back that started the removals),
[[iteration-162a-discipline-removal-safe-phases]] and [[iteration-162b-ac-fidelity-shrink]].

## Why there is no AC-fidelity gate

`ac-fidelity-check.sh` parsed a plan's `## Acceptance Criteria` block and, for each ticked box,
looked for a test slug or code symbol in the branch diff. It existed from iter-61z to iter-162b.

It could never verify that a test *ran* — it only ever saw a plan file and a diff, as its own
header said. What it actually produced:

- **28 commits whose entire content is rewording an AC so the gate stops firing.** Eight say so in
  their own subject lines: *"backtick test slugs on same line for ac-fidelity heuristic"*, *"put a
  gate-resolvable symbol on AC4's first line"*, *"unwrap unit_145 AC line so ac-fidelity-check
  finds it"*. The clearest is `c51656d` (iter-86): one file, +12/−13, all in the plan, changing an
  AC from `perf audit`/non-headless to `perf vitals`/headless — different command, different mode
  — to match whichever test happened to exist. Gate: 11/11 PASS.
- **One catch, found by a human first.** PR #188 shipped two ACs reading *"implemented and
  compiled … not exercised end-to-end in this session's time budget"*. A human unticked them at
  `6273773`; iter-154 taught the gate that wording afterwards. It caught a confession, never a lie.
- **Zero catches and seven false positives across the 158–161 batch** (five in 158, two in 160),
  plus one *induced falsification*: five iter-158 ACs merged carrying `[x]` **and**
  `[deferred — new plan: …]` simultaneously, because a review agent needed the gate green. Undone
  at `7092fba`. A gate whose failure mode is making the plan lie cannot be justified by the plan's
  accuracy.
- **The `[verified: …]` requirement was falsified on 2026-08-13.** iter-153's ticked AC carried
  `[verified: 2026-08-13, … 3 passed / 0 failed]`. The annotation was **truthful** — that run
  happened and passed. It certified a broken feature anyway, because
  `live_153_replace_emits_single_envelope` passes in isolation and fails under contention.

The decisive fact came from the repo owner on 2026-08-14: *"I lost the overview how it works and I
never actually looked at the ACs anyway."* Every rule in the family policed the accuracy of a
record with no reader. ACs are load-bearing as **instructions** — the spec an implementing agent
works against — not as **records**.

What replaced it: tick honestly, the carry-over sweep, and the per-PR live sweep. All three are in
the `iteration-close` skill; none is a checker.

## Why `claims-vs-code.sh` went

It extracted "adds Foo::Bar"-style claims from commit messages and grepped the diff for evidence,
emitting a `## Claims vs code` PR section. Advisory from day one; it never fired. Its promotion to
a hard gate ([[iteration-61aa-claim-miss-hard-gate]]) is `status: obsolete`, and exactly one
`// allow-claim-miss:` exists in the tree (`crates/ff-rdp-cli/src/page_map/mod.rs:166`). The rule
survives as a review rule: commit-message claims must be backed by the branch diff.

## Why there is no `check-iteration-ready`

iter-162a deleted it. It hard-coded its own sub-check list, so every gate added or removed cost a
count bump and an assertion edit. Enumerate `cargo run -q -p xtask -- --help` instead.

## Why there is no `check-dead-primitives`

The "every new `pub` item needs a non-test consumer" rule is a **review** rule, not a gate.
iter-162a deleted the gate after it induced a decoy — `DemuxReader::new()` constructed in
`daemon/server.rs` purely to satisfy it, see the comment at `daemon/server.rs:750-756` — while 425
lines of genuinely dead public API shipped past it and survived every CI run until a human found
them.

## Why there is no `check-todo-annotations` and no pre-commit hook

The "every TODO/FIXME/XXX carries an issue link or `// allow-todo: <reason>`" rule is also a review
rule. The gate and the `.githooks/pre-commit` that duplicated it both went in iter-162a, having
guarded an empty set — 0 hits in `crates/ff-rdp-{core,cli}/src` — for their entire lifetime.

## Why nothing detects mirror drift

`~/.claude/skills/{ralph-loop,new-ralph-loop}/scripts/` mirror to `tools/*/scripts/` so skill
changes are reviewable in a PR diff. `check-discipline-regression` verified that until iter-162b
deleted it — the check existed *only* to keep four copies of two scripts in sync, and those
scripts are gone. This is a real loss of safety, accepted deliberately: the gate guarded an
obligation created by the machinery it guarded.

The obligation survives for `run-iteration.sh`, `ralph.workflow.js` and `smoke.workflow.js`, by
hand. A 3-of-4 edit is exactly what went wrong in iter-140/146 (`3dc5330`): a fix to
`ac-fidelity-check.sh` landed in the mirrored ralph-loop copy, was silently missed in the then-
unmirrored new-ralph-loop one, produced a false failure, and the response was to reword an
iteration plan rather than fix the tool. Verify with
`diff -r ~/.claude/skills/<skill>/scripts/ tools/<skill>/scripts/`.

## Why the live sweep is a standing policy rather than a gate

Live tests are `#[ignore]`-gated and never run in CI, so nothing downstream will ever execute
them. The gate that claimed to cover this read an annotation, not a run — see the iter-153 case
above. A pasted sweep reads execution. It caught three real failures on iter-160's branch before
its PR opened, one of which would have broken every cross-origin frame click.

iter-158 is what makes a sweep's `ok` trustworthy: 152 live-test call sites used to return early
when their env gate was unset, and libtest scored that `ok`. They panic now.

## Why plan `status:` has exactly one word per state

`planned | in-progress | in-review | done | obsolete`, enforced by `check-iteration-plan`. `done`,
never `completed` — the latter was a synonym the merge workflow wrote, and the 142 plans carrying
it were normalized on 2026-08-12. (`kb/research/` documents still use `completed`; the validator
does not govern them.)

## Why iteration-number uniqueness lives inside `check-iteration-plan`

Five plans were filed on a number another plan already held (44, 73, 171, 178, 183 — three of
them in the week of 2026-08-17). The cost was never the collision itself: it was that the second
author had already written the plan before anyone noticed, and the renumber then had to chase
every `[[wikilink]]`, carry-over row and PR-body reference that cited the old number. Iteration
179's merged PR body still carries a stale `iteration-184-dogfood-sentinel-shared-tmp-path` link
from exactly this churn.

[[iteration-162]]a deleted discipline gates on the reasoning that a gate nobody reads is worse
than a documented rule, and that decision stands. iter-187 is deliberately **not** a reversal:
it added no xtask subcommand and no required step. `check-iteration-plan` is already the thing
CLAUDE.md requires before a plan is filed; it simply stopped validating the file in isolation and
started looking at the directory it belongs to. Nobody has to remember anything new.

Two shapes are explicitly not collisions, because a loose regex gets each of them wrong:
letter-suffixed siblings (`iteration-162a-`, `iteration-162b-`) are distinct numbers, and
`.dogfood.sh` sidecars share a plan's stem without being plans. The check matches on a numeric
boundary and on `*.md` only.

The two pre-existing collisions (44 and 73, all four plans terminal) are exempt by **exact
file-name pair**, recorded in `LEGACY_COLLISIONS` in `crates/xtask/src/check_iteration_plan.rs`.
Renumbering terminal plans would break inbound links in merged PR bodies to no benefit; keying
the exemption on the pair rather than the number means a third plan claiming 44 still fails, so
the check is grandfathered rather than weakened.

## Why spec drift is annotated rather than blocked

When ff-rdp must send a field or call a method that is **not** declared in the published Firefox
spec dict but the server reads anyway, the call site carries `// allow-spec-drift: bug NNNN`,
naming a filed Mozilla Bugzilla issue. This makes the drift reviewable for the `rdp-spec-reviewer`
agent and pairs every drift with an upstream-fix tracker (iter-77). `bug TBD (<rationale>)` is for
the initial landing of a newly-discovered drift only; replace `TBD` with the real number in a
follow-up before the next release cut. The agent flags any `TBD` it sees.
