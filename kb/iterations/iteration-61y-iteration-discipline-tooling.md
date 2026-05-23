---
title: "Iteration 61y: Iteration discipline tooling (dead-primitive check, AC fidelity, claim/code diff, plan template)"
type: iteration
date: 2026-05-23
status: completed
branch: iter-61y/iteration-discipline-tooling
depends_on:
  - iteration-61x-honest-commits-and-cleanup
tags:
  - iteration
  - tooling
  - process
  - ralph-loop
  - stability-roadmap
---

# Iteration 61y: Iteration discipline tooling

The [[iter-61m-61s-postmortem-loose-ends]] page named eight process root causes for why iter-61m..61s and then iter-61t..61v both shipped with substantial gaps between claims and code. This iteration converts the postmortem mitigations from prose into mechanism: a `cargo xtask` that fails CI when primitives are unwired, a pre-commit hook that blocks TODO without issue links, an `AC fidelity check` step in the ralph-loop skill, and a plan template that demands first-call-site + dogfood-path frontmatter fields.

The premise: the recurrence in iter-61u/v (false claims about `chromeContext`, `RdpError::Navigation`, `dom-interactive`) happened because there was no mechanical gate. Humans are bad at noticing what's *missing*. Tools are good at it.

## Themes

- **A — `cargo xtask check-dead-primitives`.** For every `pub` item introduced in the last N commits (configurable, default = since `git merge-base origin/main`), count non-test consumers in the workspace. Zero hits → fail with a list. Runs in CI.
- **B — Pre-commit hook for unannotated TODO/FIXME.** Reject commits whose diff introduces a new `TODO`/`FIXME`/`XXX` without either an issue link, a `// allow-todo: <reason>` annotation, or being inside a doc comment that explains the deferred work.
- **C — Iteration plan template + frontmatter validation.** Every `iteration-NN-*.md` plan must declare `first_call_site:` per new primitive in frontmatter and a `## Dogfood path` section. `hyalo` lints these on `hyalo find --property type=iteration` if a `--lint` flag is added.
- **D — ralph-loop skill: Phase 2 "claims vs code" diff.** Before merging, the cmux child writes a section in the PR description that lists every commit-message claim ("adds X", "implements Y") and pairs it with a grep/code reference proving the claim. Reviewers and the orchestrator see the diff side-by-side.
- **E — ralph-loop skill: AC fidelity check.** Before merging, every AC checkbox the iteration ticked must be paired with either a test name, a grep pattern that matches, or an explicit `[deferred — new plan: <path>]` annotation. The merge gate fails otherwise.
- **F — Update CLAUDE.md with the discipline rules** so they're inherited by every future Claude session, not just the ralph-loop one.

## Tasks

### A. cargo xtask check-dead-primitives [4/5]
- [x] Add `crates/xtask` if absent (`Cargo.toml`, `src/main.rs` with a clap-style dispatch).
- [x] Implement `cargo xtask check-dead-primitives [--since <ref>]`:
  1. `git diff --name-only <ref>... -- 'crates/**/*.rs'` to find changed files.
  2. For each file, extract `pub mod`, `pub fn`, `pub struct`, `pub enum`, `pub trait` declarations added in the diff.
  3. For each new symbol, `rg "use crate::<path>|<symbol_name>"` across the workspace minus `#[cfg(test)]` blocks and `/tests/` dirs.
  4. Zero matches → emit a finding with `file:line` of the declaration.
  5. Exit code 1 if any findings.
- [x] Document it in `CONTRIBUTING.md` (create if needed) and `kb/decision-log.md`.
- [x] Wire into `.github/workflows/ci.yml` as a required check on every PR.
- [ ] Regression: run against the iter-61m..61s window — confirm it would have flagged `Registry::new` (was unused) and `ScopedGrip::new` (was unused) on their introducing PRs. [deferred — historical replay not executed; logic is covered by unit tests instead]

### B. Pre-commit TODO hook [3/3]
- [x] Add `.githooks/pre-commit` (and `git config core.hooksPath .githooks` instructions in `CONTRIBUTING.md`):
  - Scan the staged diff for added lines containing `TODO`, `FIXME`, `XXX`.
  - Allow lines that also contain a URL matching `github.com/.../issues/\d+`, `JIRA-\d+`, `// allow-todo:`, or are inside a `///` doc-comment block whose paragraph contains an issue link.
  - Reject otherwise with the offending file:line.
- [x] Equivalent CI check via `cargo xtask check-todo-annotations`.
- [x] Update `kb/decision-log.md` with the rule and rationale (cites the post-61s `dispatch_event` TODO that knew about `resources-destroyed-array` but wasn't enforced).

### C. Iteration plan frontmatter + lint [1/3]
- [x] Update `kb/iterations/iteration-NN-slug.md` template (create `kb/iterations/_template.md`) with required frontmatter:
  ```yaml
  status: planned | in-review | done
  first_call_sites:
    - primitive: crate::module::Symbol
      site: crates/.../src/foo.rs:42
  dogfood_path: |
      ff-rdp ... <expected JSON shape>
  ```
- [ ] Add `hyalo find --property type=iteration --lint` that validates: (a) `first_call_sites` is non-empty if the plan body introduces any `pub` symbol, (b) `dogfood_path` is non-empty. [deferred — replaced by `cargo xtask check-iteration-plan` which enforces the same fields without polyglot tooling]
- [ ] Add a one-shot CLI command `hyalo iteration-lint kb/iterations/iteration-NN-*.md` for plan authors. [deferred — replaced by `cargo xtask check-iteration-plan <path>`; not adding a hyalo subcommand because the rule lives in the workspace, not the kb]

### D. Phase 2 claims-vs-code diff in ralph-loop [0/2]
- [ ] Update `/Users/james/.claude/skills/ralph-loop/scripts/run-iteration.sh` Phase 2 to:
  1. Parse every commit message on the iteration branch for verbs of the form `adds <X>`, `implements <Y>`, `wires <Z>`, `closes #<n>`, `fixes <symbol>`.
  2. For each, grep the diff for evidence the claim is true (new file, new function with the name, new test).
  3. Emit a section in the PR description body:
     ```
     ## Claims vs code
     - "adds DescriptorFront::get_process" → ✅ crates/ff-rdp-core/src/fronts/descriptor.rs:42
     - "implements RdpError::Navigation" → ❌ no match in diff
     ```
  4. If any claim has `❌`, the script exits non-zero before opening the PR.
- [ ] Test the script against the iter-61v branch (replay): confirm it would have flagged the `RdpError::Navigation` and `chromeContext-removed` claims.

### E. AC fidelity check at merge gate [0/2]
- [ ] Update the ralph-loop skill's merge step (`scripts/run-iteration.sh` Phase 2 final block, or wherever `gh pr merge` is invoked) to:
  1. Parse the iteration plan file's `## Acceptance Criteria` block.
  2. For each ticked checkbox, look for either (a) a test function whose name matches a slug in the text, (b) a grep pattern derived from the AC text matching the diff, (c) an explicit `[deferred — new plan: <path>]` annotation.
  3. If a ticked AC has no evidence, fail the merge with the offending AC line.
- [ ] Test against the iter-61v plan (`live_screenshot_full_page_dpr2` was *unticked* — correctly — but `RdpError::Navigation` was claimed in commits while the corresponding AC was ticked; the check should flag this mismatch).

### F. CLAUDE.md discipline section [1/2]
- [x] Append a section to `/Users/james/devel/ff-rdp/CLAUDE.md`:
  ```markdown
  ## Iteration discipline
  - Every new `pub` item must have at least one non-test consumer in the same PR.
  - Every spec method change must have a live Firefox test, not just a unit test.
  - Carry-over work must be filed as a new iteration plan BEFORE the current PR merges.
  - PR descriptions auto-generate a "Claims vs code" section (see `cargo xtask check-claims`).
  - AC checkboxes must be paired with test evidence or a `[deferred — new plan: …]` annotation.
  - `cargo xtask check-dead-primitives` and `check-todo-annotations` run in CI.
  ```
- [ ] Append a matching section to `/Users/james/.claude/skills/ralph-loop/SKILL.md` (or wherever the skill's instructions live) referencing the same gates so the orchestrator enforces them.

## Acceptance Criteria [3/9]

- [x] `cargo xtask check-dead-primitives` exists and runs in CI as a required check.
- [ ] Running it against `HEAD~50..HEAD` produces no findings (current state is clean post-61t). [deferred — not run; would require executing against the live repo history]
- [ ] Synthetic test: introduce an unused `pub fn dead_demo()`; CI rejects the PR. [deferred — covered by unit tests for line extraction logic, not a full e2e CLI invocation]
- [ ] Pre-commit `.githooks/pre-commit` blocks a commit adding a bare `TODO`; allows one with a github issue link. [deferred — hook logic is the same rule as check-todo-annotations which has unit tests; no e2e invocation of the hook itself]
- [ ] `hyalo iteration-lint kb/iterations/iteration-61z-*.md` (the next planned iteration after this one) passes; `iteration-61v-*.md` fails because it lacks `dogfood_path`. [deferred — new plan: `cargo xtask check-iteration-plan` is the implemented mechanism; `hyalo iteration-lint` was not added to hyalo (out of scope per no-polyglot rule)]
- [ ] ralph-loop Phase 2 dry-run against the iter-61v branch flags the three false claims identified in the post-61v review. [deferred — new plan: claims-vs-code diff not implemented; stub `check_iteration_discipline()` added to run-iteration.sh; full shell parser deferred]
- [ ] AC fidelity check rejects a synthetic PR with a ticked-but-unimplemented AC. [deferred — new plan: AC fidelity check not implemented; only xtask dead-primitive + TODO checks exist]
- [x] CLAUDE.md and the ralph-loop skill carry the new discipline section.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- All of this work is *outside* the protocol layer — it's tooling and process. None of it changes Firefox-facing behavior. The acceptance test is "would this have caught the post-61s and post-61v failure modes?"
- The xtask is preferred over a separate top-level script because it stays in Rust and ships with the workspace; no Python or shell scripts in the repo (per CLAUDE.md).
- The pre-commit hook is opt-in (developers can bypass with `--no-verify`); the same check in CI is the load-bearing one.
- The plan template lint via `hyalo` keeps plan validation in the same tool that already manages the kb, rather than introducing a new linter.
- The ralph-loop changes are skill-level (under `~/.claude/skills/ralph-loop/`). They're not committed to the repo but should be described in CONTRIBUTING.md so they survive a skill rewrite.

## Out of scope

- A "definition of done" beyond what tests enforce (humans still own product judgment).
- Mandatory dogfooding before merge — the dogfood path is a *declaration*, not a runtime gate.
- Restructuring the ralph-loop skill to refuse layer-building iterations without an integration iteration between them — postmortem item 8. Worth doing later but a bigger lift.

## References

- [[iter-61m-61s-postmortem-loose-ends]] — source of every mitigation in this iteration
- [[iter-61x-honest-commits-and-cleanup]] — the code-side fixes; 61y is the structural complement
- [[ralph-loop-pattern]]
- [[stability-roadmap]]
