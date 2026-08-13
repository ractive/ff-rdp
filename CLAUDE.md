# Agents
Delegate the work to agents whenever possible to avoid automatic context compaction.

# Documentation

Keep all documentation in `./kb` as `*.md` markdown files with YAML frontmatter (text, numbers, checkboxes, dates, lists). Use it as your second brain:
- Research outcomes → `research/`
- Design decisions → `decision-log.md`
- Iteration plans → `iterations/iteration-NN-slug.md` (one file per iteration, markdown task lists for steps/tasks/ACs)

Organize in subfolders. Use `[[wikilinks]]` for cross-references. Keep Obsidian-compatible.

Use `hyalo` CLI (not Read/Grep/Glob) for all markdown knowledgebase operations.
Examples: `hyalo find --property status=planned --format text`, `hyalo find "search text"`, `hyalo find --property 'title~=pattern'`.
Run `hyalo --help` for usage. Use `--format text` for compact LLM-friendly output.

# Rust

## Language Server
Use the rust-analyzer-lsp language server plugin for code intelligence: analyzing code, finding references, go-to-definition, checking clippy warnings.
Run "cargo check" before using it to update its indexes, after changing *.rs files.

## Code Quality Gates
Make the code unit testable. Add tests if feasible. Add e2e tests for all commands/subcommands.

It must be compatible with Windows, Linux and macOS.

Before committing or creating a PR, run **in this order** and fix all issues:
1. `cargo fmt`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace -q`

Never skip a step. Never commit code that fails any of these.

Merge PRs **on GitHub** (`gh pr merge <n> --merge --delete-branch`), not with a local
`git merge` + `git push origin main`. A local merge bypasses the PR flow entirely, so branch
protection and required checks are never consulted — which leaves the protection config
untestable. If a GitHub merge is refused, that is the enforcement working: report the reason
and stop; never fall back to a local merge. Do *not* merge with `--squash`.

Iteration plan `status:` vocabulary is `planned | in-progress | in-review | done | obsolete`,
enforced by `cargo run -p xtask -- check-iteration-plan`. Use `done`, never `completed` — the
latter was a synonym that the merge workflow wrote, and the 142 plans carrying it were
normalized on 2026-08-12 so each state has exactly one word. (`kb/research/` documents still
use `completed`; the validator does not govern them.)

### Live tests
Some tests require a running Firefox instance and are gated by an env var:
- `FF_RDP_LIVE_TESTS=1` — enables tests that launch headless Firefox locally.
- `FF_RDP_LIVE_NETWORK_TESTS=1` — enables tests that also make real network requests.

Run them with: `FF_RDP_LIVE_TESTS=1 cargo test-live`

The `cargo test-live` alias (defined in `.cargo/config.toml`) expands to `cargo test --workspace -- --include-ignored`, which includes all `#[ignore]`-gated live tests.

**AC checkbox convention**: every AC checkbox in an iteration plan MUST name the live test and the asserted post-condition, e.g.:
```
- [ ] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR
```
An AC without a named test is not done. **Ticking** a `live_*` AC additionally requires a
`[verified: <YYYY-MM-DD>, <measured result>]` annotation recording the run — prose such as
"verified live, PASS" is not accepted, the bracket form is:
```
- [x] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR [verified: 2026-08-12, 2400 px ≥ 1200 × 2]
```
`ac-fidelity-check.sh` enforces this (iter-154); see the Iteration discipline section for why.

## Code Patterns
- No `.unwrap()` / `.expect()` outside of tests — use `anyhow::Context` with `?`
- No `clone()` unless the borrow checker demands it — try references first
- No unnecessary `pub` on struct fields
- All code stays in Rust — no polyglot tooling (no Bun, Node, Python scripts)
- New crates go in `crates/` with naming convention `ff-rdp-<domain>`
- `thiserror` in core library, `anyhow` in CLI
- JSON-only output with `--jq` filter support

## PR Discipline
- One iteration = one branch = one PR
- Branch naming: `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself

## Iteration discipline
- Every new `pub` item must have at least one non-test consumer in the same PR.
  Run `cargo run -p xtask -- check-dead-primitives --since origin/main` to verify.
- Every `TODO`/`FIXME`/`XXX` must include a GitHub issue link, Jira ticket, or `// allow-todo: <reason>`.
  Run `cargo run -p xtask -- check-todo-annotations --since origin/main` to verify.
- Every spec method change must have a live Firefox test, not just a unit test.
- Carry-over work must be filed as a new iteration plan BEFORE the current PR merges.
- AC checkboxes must be paired with test evidence or a `[deferred — new plan: …]` annotation.
  An AC without a named test is not done — do not tick it.
  The ralph-loop skill enforces this at merge time via `ac-fidelity-check.sh`:
  every ticked AC must reference a test slug, a code symbol that appears in the
  diff, or the `[deferred — new plan: <path>]` form. See iter-61z.
  **The gate reads a plan and a diff — it cannot tell you a test ran.** A green
  `ac-fidelity-check` means the ticked ACs *reference* evidence that resolves,
  nothing more; running the tests is still on you. iter-154 narrowed the gap at
  two points: a ticked AC whose text admits non-execution ("not exercised", "not
  run", "never run", "not executed", "implemented and compiled", "not verified" —
  matched as whole words) fails outright, and a ticked
  AC naming a `live_*` test must carry `[verified: <YYYY-MM-DD>, <measured result>]`
  — live tests are `#[ignore]`-gated and never run in CI, so nothing downstream
  will ever execute them. Both read the AC's full wrapped text, not just its first
  line. Untick or defer rather than routing around the wording — the deferral
  annotation must be the **last** thing on the AC. If an AC legitimately *describes*
  behaviour using those words ("`--dry-run` does not run the command"), annotate it
  `[allow-ac-wording: <reason ≥10 chars>]`; that escape hatch exists so the remedy
  for a false positive is never "reword until the grep stops firing".
- Spec drift: when ff-rdp must send a field or call a method that is NOT
  declared in the published Firefox spec dict (but the server *reads* it
  anyway), annotate the call site with `// allow-spec-drift: bug NNNN`,
  where `bug NNNN` is a filed Mozilla Bugzilla issue tracking the gap.
  The annotation makes the drift reviewable for the `rdp-spec-reviewer`
  agent and pairs every drift with an upstream-fix tracker (see iter-77).
  Use `// allow-spec-drift: bug TBD (<short rationale>)` ONLY for the
  initial landing of a newly-discovered drift; replace `TBD` with the
  actual Bugzilla number in a follow-up iteration before the next release
  cut. The `rdp-spec-reviewer` agent flags any `TBD` annotation it sees.
- Commit-message claims (`adds Foo::Bar`, "subscribes to dom-interactive",
  "implements RdpError::Navigation") must be backed by the branch diff. The
  ralph-loop skill emits a `## Claims vs code` PR-description section via
  `claims-vs-code.sh`; unmatched claims become ❌ rows the reviewer sees. Add
  `// allow-claim-miss: <symbol>` near the relevant code if a claim is
  legitimately untestable.
- Iteration plans must include `dogfood_path` and `first_call_sites` (if new pub items).
  Validate with: `cargo run -p xtask -- check-iteration-plan kb/iterations/iteration-NN-slug.md`
- **Before `/create-pr` on an iter-* branch, run the one-shot pre-PR gate:**
  ```bash
  cargo run -p xtask -- check-iteration-ready --plan <plan-path> --base origin/main
  ```
  This aggregates all discipline sub-checks in one command:
  1. `check-dead-primitives --since <base>` — no unwired new pub items
  2. `check-todo-annotations --since <base>` — no bare TODO/FIXME/XXX <!-- allow-todo: documents the check itself -->
  3. `check-actor-kb-sync --since <base>` — actor `.rs` changes paired with kb updates
  4. `check-firefox-refs <plan>` — `firefox_refs:` line ranges valid
  5. `check-discipline-regression` — mirror sync + replay baselines
  6. `ac-fidelity-check.sh --plan <plan> --base <base>` — ticked ACs *reference*
     resolvable evidence, declare no non-execution, and carry `[verified: …]` where
     they name a `live_*` test (it cannot verify a test ran)
  Fix every reported failure before pushing. CI still runs each gate individually as required checks.
- `cargo xtask check-dead-primitives`, `check-todo-annotations`,
  `check-discipline-regression`, `check-firefox-refs`, and `check-actor-kb-sync`
  run in CI as required checks. The latter two were added in iter-73 (spec-fidelity-gates):
  - `check-firefox-refs <plan>` — validates `firefox_refs:` line ranges in an iteration plan
    against the local Firefox checkout (`FF_RDP_FIREFOX_PATH`).
  - `check-actor-kb-sync --since origin/main` — fails if an actor `.rs` file was changed
    without a corresponding `kb/rdp/actors/*.md` update.
  `check-discipline-regression` pins the iter-61v (FAIL) and iter-61t (PASS) replay baselines
  so the heuristics in `claims-vs-code.sh` / `ac-fidelity-check.sh` don't silently regress.
- The ralph-loop skill scripts live in `~/.claude/skills/ralph-loop/scripts/`;
  a mirror is checked in at `tools/ralph-loop/scripts/` so changes are
  reviewable. Edit both. `check-discipline-regression` catches drift.
  The same applies to the **new-ralph-loop** skill:
  `~/.claude/skills/new-ralph-loop/scripts/` mirrors to
  `tools/new-ralph-loop/scripts/` (both `.sh` files *and* `ralph.workflow.js` /
  `smoke.workflow.js` — the workflow script carries the orchestration logic).
  This mirror was added after the 138–142 batch: without it, a fix to
  `ac-fidelity-check.sh` landed in the mirrored ralph-loop copy and was
  silently missed in the unmirrored new-ralph-loop one, and an iteration plan
  got reworded to route around the resulting false failure.
- Skill-edit iterations (those that modify `~/.claude/skills/`) cannot run
  through ralph-loop itself — drive them by hand in a regular Claude session.
- See `CONTRIBUTING.md` for full details and install instructions for the pre-commit hook.
