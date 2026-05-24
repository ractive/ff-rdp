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
Do *not* merge with "--squash".

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
An AC without a named test is not done.

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
- Commit-message claims (`adds Foo::Bar`, "subscribes to dom-interactive",
  "implements RdpError::Navigation") must be backed by the branch diff. The
  ralph-loop skill emits a `## Claims vs code` PR-description section via
  `claims-vs-code.sh`; unmatched claims become ❌ rows the reviewer sees. Add
  `// allow-claim-miss: <symbol>` near the relevant code if a claim is
  legitimately untestable.
- Iteration plans must include `dogfood_path` and `first_call_sites` (if new pub items).
  Validate with: `cargo run -p xtask -- check-iteration-plan kb/iterations/iteration-NN-slug.md`
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
- Skill-edit iterations (those that modify `~/.claude/skills/`) cannot run
  through ralph-loop itself — drive them by hand in a regular Claude session.
- See `CONTRIBUTING.md` for full details and install instructions for the pre-commit hook.
