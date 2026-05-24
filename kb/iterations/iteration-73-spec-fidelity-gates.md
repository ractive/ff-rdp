---
title: "Iteration 73: Spec-fidelity gates — firefox_refs xtask, actor↔kb sync, rdp-spec-reviewer agent"
type: iteration
date: 2026-05-24
status: done
branch: iter-73/spec-fidelity-gates
depends_on:
  - iteration-72-transport-polish
first_call_sites:
  - primitive: xtask::check_firefox_refs::run
    site: tools/xtask/src/main.rs (new subcommand `check-firefox-refs` dispatch)
  - primitive: xtask::check_actor_kb_sync::run
    site: tools/xtask/src/main.rs (new subcommand `check-actor-kb-sync` dispatch)
  - primitive: xtask::firefox_refs::FirefoxRef
    site: tools/xtask/src/check_firefox_refs.rs (consumed by `run`)
dogfood_path: |
  # 1. Validate the iter-74 plan against the local firefox checkout.
  FF_RDP_FIREFOX_PATH=/Users/james/devel/firefox \
    cargo run -p xtask -- check-firefox-refs \
      kb/iterations/iteration-74-protocol-correctness-oneway-events-lifecycle.md
  # Expected: "OK: 8 firefox_refs verified" or per-ref miss report.
  
  # 2. Run actor↔kb sync gate against current branch.
  cargo run -p xtask -- check-actor-kb-sync --since origin/main
  # Expected: passes on a clean branch; on a branch that edited
  # crates/ff-rdp-core/src/actors/watcher.rs without touching
  # kb/rdp/actors/watcher.md, exits non-zero with the missing path.
  
  # 3. Invoke the new spec-reviewer agent on a synthetic diff fixture.
  claude --agent rdp-spec-reviewer \
    --input tools/agents/fixtures/synthetic-watcher-diff.patch
  # Expected: drift report flags the missing spec line ranges.
tags:
  - iteration
  - process
  - gates
---

Protocol fidelity is the codebase's recurring failure mode (W2, S1, S3,
S6 in the latest review). The discipline gates we already ship
(`check-dead-primitives`, `check-todo-annotations`,
`check-discipline-regression`, `ac-fidelity-check`, `claims-vs-code`)
catch *internal* drift but say nothing about whether the wire shape
still matches Firefox. This iter introduces the missing third leg:
every protocol/actor iter must cite real `devtools/` line ranges, the
ranges must exist, and any actor source edit must travel with its kb
note. A new specialist subagent (`rdp-spec-reviewer`) re-reads the
cited spec + server file on PR creation and produces a drift report
before Copilot/CodeRabbit ever look at the diff.

This is a pure-process iter — no `ff-rdp-core` or `ff-rdp-cli` Rust
changes. It is also the foundation for iter-74..77, which all carry
`firefox_refs:` and will be the first plans gated by these checks.

## Themes

- **A — `check-firefox-refs` xtask.** Parses iteration-plan frontmatter,
  resolves `$FF_RDP_FIREFOX_PATH` (default `/Users/james/devel/firefox`),
  asserts each cited path exists and each line range is non-empty.
- **B — `check-actor-kb-sync` xtask.** Diff-aware: any touched
  `crates/ff-rdp-core/src/actors/<X>.rs` must be paired with a touched
  `kb/rdp/actors/<X>.md` in the same `git diff origin/main...HEAD`.
- **C — `rdp-spec-reviewer` subagent + CLAUDE.md/CONTRIBUTING wire-up.**
  Agent definition lives under `~/.claude/agents/` with a mirror at
  `tools/agents/` (same convention as the ralph-loop scripts mirror).
  Hooked into `/create-pr` before the Copilot/CodeRabbit pass.
- **D — Actor kb backfill.** Stub `kb/rdp/actors/<X>.md` for every actor
  in `crates/ff-rdp-core/src/actors/` that has no current note. Stubs
  only — link to the spec + server file with line ranges, plus a single
  sentence describing what the actor is for. No deep content.

## Tasks

### A. `check-firefox-refs` xtask
- [x] Add `tools/xtask/src/check_firefox_refs.rs` with `pub fn run(plan_path: &Path) -> Result<()>` and a `FirefoxRef { path, lines, why }` struct.
- [x] Parse the plan's frontmatter (reuse the YAML parser the existing xtasks already pull in for `check-iteration-plan`); tolerate plans with no `firefox_refs:` (process iters like this one).
- [x] For each ref: resolve `$FF_RDP_FIREFOX_PATH` (default `/Users/james/devel/firefox`), assert `path` exists, parse `lines: "<start>-<end>"`, read the file, assert the range is in-bounds and non-empty.
- [x] Wire dispatch in `tools/xtask/src/main.rs` (`check-firefox-refs <plan>` subcommand). Exit code 1 with a per-ref error report on any miss.
- [x] Unit tests: in-range, out-of-range, missing-file, missing-env, malformed line spec.

### B. `check-actor-kb-sync` xtask
- [x] Add `tools/xtask/src/check_actor_kb_sync.rs` with `pub fn run(since: &str) -> Result<()>`.
- [x] Shell out to `git diff --name-only <since>...HEAD`. For each path matching `crates/ff-rdp-core/src/actors/(?P<x>[a-z_]+)\.rs`, assert `kb/rdp/actors/<x>.md` is also in the diff (or, allow-listed via the existing `// allow-claim-miss:` style comment near the top of the .rs file: `// allow-actor-kb-skip: <reason>`).
- [x] Map known rename pairs (`dom_walker.rs` → `walker.md`, `screenshot_content.rs` → `screenshot-content.md`) in a small constant table; fail loudly when an actor has no mapping AND no kb file.
- [x] Wire dispatch + unit tests with synthetic diff fixtures.
- [x] Add both xtasks to `.github/workflows/ci.yml` as required checks (alongside `check-discipline-regression`).

### C. `rdp-spec-reviewer` subagent
- [x] Author `tools/agents/rdp-spec-reviewer.md` (system prompt + tool allowlist). Mirror to `~/.claude/agents/rdp-spec-reviewer.md`; document the mirror in `CONTRIBUTING.md`.
- [x] Inputs: PR diff (stdin) + the touched plan path. Outputs: per-actor drift report (markdown). For each touched `actors/<X>.rs`, the agent reads the cited `devtools/shared/specs/<X>.js` + `devtools/server/actors/<X>.js` and lists: (1) methods present in the spec but not in the diff/source, (2) fields the source sends that the spec doesn't declare, (3) `oneway:`/`release:`/`BULK_RESPONSE` marker mismatches.
- [x] Hook into the `/create-pr` skill: run the agent BEFORE the Copilot/CodeRabbit pass; surface the drift report as a comment block in the PR body under a `## Spec drift` heading.
- [x] Add `tools/agents/fixtures/synthetic-watcher-diff.patch` + expected report under `tools/agents/fixtures/expected/`. An xtask `cargo run -p xtask -- run-agent-fixtures` diffs actual vs expected (snapshot test).

### D. CLAUDE.md + kb backfill
- [x] Update `CLAUDE.md` "Iteration discipline" section to require `firefox_refs:` on any protocol/actor iter; list `check-firefox-refs` + `check-actor-kb-sync` as required gates next to the existing `check-dead-primitives`/`check-todo-annotations`/`check-discipline-regression` bullets.
- [x] Mirror to `CONTRIBUTING.md` (install instructions, env-var note, how to add an allowlist line).
- [x] Backfill stub kb notes for every actor without one. Current gap (from `ls`): `device.md`, `dom-walker.md` (alias for `walker.md` — decide whether to alias or split), `inspector.md`, `object.md`, `responsive.md`, `storage.md`, `string.md`, `tab.md`, `target.md`, `thread.md`. Each stub: title, spec path + line range, server path + line range, one sentence purpose. No deep content.

## Acceptance Criteria [7/7]

- [x] `check_firefox_refs_validates_real_plan`: `tools/xtask/tests/check_firefox_refs.rs::check_firefox_refs_validates_real_plan` — given iter-74's plan and a valid `FF_RDP_FIREFOX_PATH`, exits 0; mutating one `lines:` value out of range exits 1.
- [x] `check_firefox_refs_missing_env_reports_clearly`: same test file, asserts unset env + missing default path produces a single actionable error (not a panic).
- [x] `check_actor_kb_sync_pairs_required`: `tools/xtask/tests/check_actor_kb_sync.rs::check_actor_kb_sync_pairs_required` — a synthetic diff touching `actors/watcher.rs` without `kb/rdp/actors/watcher.md` exits 1; the same diff with both files exits 0.
- [x] `check_actor_kb_sync_allow_list_works`: same test file — a diff with `// allow-actor-kb-skip: refactor-only` at the top of the touched .rs is accepted.
- [x] `rdp_spec_reviewer_fixture_snapshot`: `tools/xtask/tests/run_agent_fixtures.rs::rdp_spec_reviewer_fixture_snapshot` — running the agent on `synthetic-watcher-diff.patch` produces output that snapshot-matches `expected/synthetic-watcher-diff.report.md`.
- [x] `claude_md_lists_new_gates`: doctest in `tools/xtask/src/lib.rs::claude_md_lists_new_gates` greps `CLAUDE.md` for `check-firefox-refs` and `check-actor-kb-sync` under "Iteration discipline".
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The xtasks are deliberately Rust (per the "all code stays in Rust" rule
in `CLAUDE.md`). The agent definition is markdown because that's the
subagent contract; the mirror at `tools/agents/` is the reviewable copy
and `check-discipline-regression` will be extended in a follow-up to
catch drift between the two locations (matches the ralph-loop scripts
mirror pattern).

`check-actor-kb-sync` is intentionally conservative: it only fires on
edits under `crates/ff-rdp-core/src/actors/`, not `fronts/`, because
fronts are client-side glue with no Firefox counterpart. Adding fronts
later is fine.

The synthetic-PR fixture for the agent is small but real — it deletes
one `oneway: true` marker and renames one field — so the snapshot test
exercises both classes of drift the reviewer is supposed to catch.

## Out of scope

- Replacing Copilot/CodeRabbit. The new agent runs *before* them and
  feeds drift into the same PR description.
- Auto-fixing drift. The reviewer reports; the iter owner fixes.
- Live tests — this iter ships no protocol changes.

## References

- [[iteration-72-transport-polish]]
- Protocol review report (2026-05-24) §5 ("process gaps")
- `CLAUDE.md` — Iteration discipline
- `tools/ralph-loop/scripts/` — mirror convention precedent
