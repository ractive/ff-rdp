---
title: "Why 7 iterations (61m..61s) shipped with so many loose ends"
type: postmortem
date: 2026-05-23
status: published
tags: [process, ralph-loop, postmortem, iteration-design]
applies_to: [iter-61m, iter-61n, iter-61o, iter-61p, iter-61q, iter-61r, iter-61s]
related_iterations: [iter-61t, iter-61u, iter-61v, iter-61w]
---

# Postmortem: loose ends after iter-61m..61s

The stability roadmap iter-61m..61s landed as 7 merged PRs in a single ralph-loop run. A cross-cutting review immediately afterwards turned up enough gaps to fill 4 follow-up iterations (61t..61w): a Registry that nothing imports, a ResourceCommand bus that the daemon doesn't subscribe to, a `ScopedGrip` primitive with zero call sites, a `resources-destroyed-array` event that a TODO comment in the dispatcher literally names but doesn't handle, spec divergences (`unwatchTargets` oneway, `startedListeners` rename, `value: String` vs `longstring`) that mock tests cannot catch, and the user-visible behavioral fixes (navigate via `document-event`, screenshot JS fallback deletion, DPR=2 live test) that were explicitly deferred inside the very iterations that promised them.

Each piece looks like an oversight in isolation. The clustering is not — it's a process pattern. This page names the pattern so future roadmaps can be structured against it.

## Root causes (in roughly descending contribution)

### 1. The "carry-over notes" escape hatch

Every iteration's AC table had room for "deferred to follow-up." Git log shows the trail: `docs(iter-61q): add carry-over notes from iter-61p`, `docs(iter-61o): note concrete refactor candidates from iter-61n`, etc. Once that mechanism exists, it gets used. The iter-61l "every AC must have a passing live test" mandate slowed it but didn't kill it because the deferral happened at AC-write time, not at AC-verify time.

**Mitigation:** No AC may have a "deferred" or "carry-over" tick on the merging branch. Deferred work is filed as a new iteration plan *before* the current PR merges, with the carry-over commit blocking merge until the new plan exists.

### 2. Primitive-without-wiring is the natural shape of layered roadmaps

When iter-61p says "build Registry" and iter-61q says "use Registry via the bus", iter-61p legitimately ships green as just the primitive — wiring is "61q's problem." iter-61q then scopes itself, defers the wiring as "carry-over", and the loop never closes. No iteration was explicitly "integrate the previous three."

**Mitigation:** Every iteration that introduces a primitive must include at least one production call site of that primitive, not just unit tests. CI greps for `use crate::<new_module>` from outside `tests/` or `#[cfg(test)]`; zero hits fails the build. The plan template should require a "First call site:" frontmatter field naming the file path.

### 3. Mock-server tests don't catch wire-shape divergence

The four spec bugs found in the review (`unwatchTargets` waiting for non-existent ACK, `listeners` vs `startedListeners` response key, `value: String` vs `longstring` for header values, `dpr: f64` vs `string`) would not fire against the mock server. The mock returns whatever shape ff-rdp expects, so the mock and the client agree by construction. Only live Firefox with a >10 KB `Set-Cookie`, a real `unwatchTargets` shutdown path, or strict protocol.js type-checking would have caught them.

iter-61o was supposed to fix exactly this with the live-test substrate — but the substrate landed without the actual coverage of the paths that needed live testing. The capability shipped; the tests did not.

**Mitigation:** For every typed spec method added in iter-61s-style iterations, a live test against headless Firefox is mandatory before merge. The acceptance criteria must cite the live test name and what it asserts on the wire (e.g. `live_unwatch_targets_no_hang: process exits within 200ms after daemon stop`).

### 4. The implementer's incentive is "land green," not "land complete"

Each cmux child runs implement → review → merge against a checklist. When something is hard (DPR=2 screenshot test on a 5000px page, document-event subscription with neterror detection, daemon buffer rewrite on top of the bus), the rational move is to tick what's done, file a carry-over note, and ship. The PR review (Copilot, CodeRabbit, local) checks code quality, naming, and obvious bugs — not roadmap fidelity.

**Mitigation:** The implementer's review gate should include a step where the AC list is read aloud (by the agent, in PR description) with each item annotated `[verified live]`, `[verified unit]`, `[deferred — new plan: …]`. Reviewers and the orchestrator can then see deferrals explicitly. The current "I ticked the box" is too cheap.

### 5. No automated "primitive exists, nothing imports it" check

A 5-line CI script — for each new public module, count non-test usages, fail if zero — would have caught the dead-code Registry on its first merge. Nobody added it because nobody was thinking about it; you only add the check after the failure happens.

**Mitigation:** Add a `cargo xtask check-dead-primitives` target that scans the workspace for `pub` modules introduced in the last N commits with zero non-test imports. Run it in CI. Output: a list of dead primitives, exit code 1 if non-empty. Easy to write; high signal.

### 6. The TODO-comment-without-implementation pattern

In `crates/ff-rdp-core/src/resources/command.rs:184-188`, the implementer wrote a comment block mentioning both `resources-updated-array` and `resources-destroyed-array`, but only implemented two of the three. The system knew the work wasn't done — there was a comment! — and nothing turned that knowledge into a blocker.

**Mitigation:** A pre-commit hook scans the diff for new `TODO`, `FIXME`, `XXX`, or "and the X one" / "also handle Y" comment patterns. Each such pattern requires either (a) an issue link, or (b) explicit `// allow-todo: <reason>` annotation. Defaults to fail.

### 7. Plans written from kb research, not from live-verification feedback

The plans were good descriptions of what *should* exist. They assumed the implementer, while building it, would notice when something didn't actually work end-to-end against Firefox. In practice the implementer worked from the AC list. Once the AC list looked done, it was done — regardless of whether the dogfood path passed.

**Mitigation:** Every iteration plan must include a "Dogfood path" section with a concrete command sequence the user would run after merge, and an expected JSON output. The implement-phase ends only when that command runs green against live Firefox, before opening the PR.

### 8. Speed and scope

Seven non-trivial iterations in one ralph-loop run, each ~30-45 min of agent work. Doing it that fast favors breadth over depth — and "depth" is exactly what wiring layers together needs.

**Mitigation:** Cap ralph-loop runs at a depth that allows an integration iteration every 3-4 layers, and refuse to schedule another layer-building iteration without one. Or: lengthen the per-iteration timeout when the iteration is flagged `integration: true` in its frontmatter.

## What this changes

The 4 follow-up iterations (61t..61w) are roughly half "fix the bugs we should have caught" and half "wire the primitives we built." That ratio is the cost of the process gaps above. We can pay it once and call it learning; we should not pay it again on every roadmap.

### Candidate changes to CLAUDE.md

```diff
+ ## Iteration discipline
+
+ When writing or reviewing an iteration plan:
+ - Every new primitive must name its first non-test call site.
+ - Every spec method must cite a live Firefox test that exercises it.
+ - "Deferred to follow-up" requires a new iteration plan file to exist *before* the current PR merges.
+ - The AC list must include a dogfood command + expected JSON output.
+ - Pre-commit / CI runs `cargo xtask check-dead-primitives`.
```

### Candidate changes to the ralph-loop skill

- After an iteration's implement-phase finishes, the orchestrator should grep the diff for new `pub mod` declarations in `core` and check that at least one non-test consumer exists in `cli` or `daemon`. If not, treat as a partial failure and ask the user before merging.
- Each iteration's Phase 2 (review) should include "AC fidelity check": read the plan's AC list, confirm each item is verified, not just ticked.
- Add a `--integration` flag that bumps the per-iteration timeout and requires the iteration plan to declare `integration: true` in frontmatter.

## Follow-up: did the mitigations work? (2026-05-24, after iter-61t..61y merged)

### Scorecard

| Mitigation | Status | Where it landed |
|---|---|---|
| 1. No "deferred" AC ticks without a new plan filed first | ⚠️ **Partial** — discipline rule documented in CLAUDE.md, no mechanical gate yet (deferred to [[iteration-61z-discipline-skill-integration]] theme B) |
| 2. Every primitive must have a production call site (CI grep) | ✅ **Closed** — `cargo xtask check-dead-primitives` (iter-61y) + CI `discipline` job |
| 3. Every spec method needs a live Firefox test | ⚠️ **Partial** — convention documented; no mechanical enforcement |
| 4. AC list read aloud in PR with `[verified live]`/`[deferred]` per item | ⚠️ **Partial** — deferred to [[iteration-61z-discipline-skill-integration]] theme B |
| 5. `cargo xtask check-dead-primitives` | ✅ **Closed** — (same as #2) |
| 6. Pre-commit hook for unannotated TODO/FIXME | ✅ **Closed** — `.githooks/pre-commit` + `cargo xtask check-todo-annotations` |
| 7. "Dogfood path" required in iteration plans | ✅ **Closed** — `kb/iterations/_template.md` + `cargo xtask check-iteration-plan` |
| 8. Integration iteration every 3-4 layers | ❌ **Open** — no mechanism added; convention-only |

### Did the recurrence get caught?

The iter-61t..61v window itself exhibited the same failure mode that the original postmortem named:

- **iter-61u** PR-claimed `chromeContext` had been removed; the spec-layer comment said so, but `actors/console.rs:226,657` and `commands/eval.rs:240,333,341` still sent and branched on the field. **The PR review missed it.**
- **iter-61v** PR-claimed a typed `RdpError::Navigation{cause}` enum with `DnsFail/CertError/ConnReset/Timeout`. `grep -rn 'RdpError::Navigation' crates/` returned zero. The corresponding AC was ticked. **The PR review missed it.**
- **iter-61v** PR-claimed gating on `dom-loading | dom-interactive | dom-complete`. Only the first and last were matched in `navigate.rs:155-189`. **The PR review missed it.**

Both the post-61s review and the post-61v review caught these by *cross-cutting reading* — diffing claims against code at a level no single PR review covers. That's the gap the iter-61z mitigations (#1 and #4 above) are designed to mechanize. Until they land, the pattern can recur.

### What the close-out arc did fix structurally

- iter-61t actually wired Registry/bus/ScopedGrip/Resource::Destroyed: 12/14 previous-review findings closed in code (the kb-review agent confirmed in the post-61v audit).
- iter-61u landed the seven spec fixes (oneway, longstring, six watcher methods, dpr-as-string, console renames). The `chromeContext` claim was a partial failure — the spec layer was cleaned, the wire wasn't.
- iter-61v landed the bus throttle = 0, the screenshot JS-fallback deletion, the document-event subscription, and `live_screenshot_full_page_dpr2` *placeholder*. iter-61x landed the real DPR=2 test on the third attempt.
- iter-61w landed all 4 themes of code (security A/B/C + LongString cap hoist E + deadline-fix F) but only 5/12 promised tests; the other 5 became iter-61x theme I — yet another carry-over.
- iter-61x corrected the iter-61u/v claim/code gaps and absorbed iter-61w's test carry-over. Its PR title "honest commits" is itself an artifact of the discipline pattern.
- iter-61y honestly marked themes D and E as `[0/2]` deferred rather than ticking with `[deferred — …]` and called the deferral out explicitly in its merge commit message. This is itself a small win — the iteration declined to lie about its own coverage.

### Conclusion

Five of eight mitigations are closed mechanically (CI checks, hooks, plan template). The two highest-leverage ones (claims-vs-code, AC fidelity) are deferred to iter-61z because they live in the ralph-loop skill, not the repo. Until 61z lands, the discipline rules are documented but not enforced — and the iter-61u/v recurrence shows that documented-but-not-enforced is not enough. **Land iter-61z before starting any new layered roadmap.**

## References

- [[iter-61t-wire-the-foundations]] — wires Registry / bus / ScopedGrip / destroyed-array
- [[iter-61u-spec-and-front-correctness]] — fixes the spec/Front divergences
- [[iter-61v-navigate-and-screenshot-completion]] — closes the deferred user-visible work
- [[iter-61w-security-hardening-and-cleanup]] — security + bulk packet + kb refresh
- [[iter-61x-honest-commits-and-cleanup]] — corrects the iter-61u/v claim/code gap
- [[iter-61y-iteration-discipline-tooling]] — cargo xtask + hook + plan template
- [[iteration-61z-discipline-skill-integration]] — deferred ralph-loop skill checks (load-bearing)
- [[stability-roadmap]] — original roadmap that produced 61m..61s
- [[ralph-loop-pattern]] — the orchestrator this ran in
- [[ff-rdp-architecture-review]]
- [[lessons-learned]]
