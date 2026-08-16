# Agents
Delegate the work to agents whenever possible to avoid automatic context compaction.

# Documentation
Docs live in `./kb` as `*.md` with YAML frontmatter — see `.claude/CLAUDE.md` for the layout and
the `hyalo` CLI rules.

# Rust

## Language Server
Use the rust-analyzer-lsp language server plugin for code intelligence: analyzing code, finding
references, go-to-definition, checking clippy warnings. Run `cargo check` before using it to
refresh its indexes, after changing `*.rs` files.

## Code Quality Gates
Make the code unit testable. Add tests if feasible. Add e2e tests for all commands/subcommands.
It must be compatible with Windows, Linux and macOS.

Before committing or creating a PR, run **in this order** and fix all issues:
1. `cargo fmt`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace -q`

Never skip a step. Never commit code that fails any of these.

## Code Patterns
- No `.unwrap()` / `.expect()` outside of tests — use `anyhow::Context` with `?`
- No `clone()` unless the borrow checker demands it — try references first
- No unnecessary `pub` on struct fields
- All code stays in Rust — no polyglot tooling (no Bun, Node, Python scripts)
- New crates go in `crates/` with naming convention `ff-rdp-<domain>`
- `thiserror` in core library, `anyhow` in CLI
- JSON-only output with `--jq` filter support
- Spec drift — a field or method ff-rdp sends that the published Firefox spec dict does not
  declare — needs `// allow-spec-drift: bug NNNN` on the call site, naming a Bugzilla issue.
  `bug TBD (<rationale>)` only for a newly-discovered drift's first landing.

## Live tests
Two env gates: `FF_RDP_LIVE_TESTS=1` (launches headless Firefox locally) and
`FF_RDP_LIVE_NETWORK_TESTS=1` (also makes real network requests).

**Never quote a `cargo test-live` pass count as evidence — it does not mean the tests reached
Firefox.** Use `cargo run -p xtask -- live-sweep`; the `iteration-close` skill explains how to run
and read it.

## PR Discipline
- One iteration = one branch = one PR; branch `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself
- Merge **on GitHub** (`gh pr merge <n> --merge --delete-branch`), never a local `git merge` +
  `git push origin main`, which bypasses branch protection and leaves it untestable. A refused
  merge is the enforcement working: report the reason and stop. Never `--squash`.

## Iteration discipline

**Before `/create-pr` on an `iter-*` branch, invoke the `iteration-close` skill.** It carries the
three closing steps — the live sweep, the xtask gate enumeration, and the carry-over sweep — none
of which is automated.

Always-on rules:
- **Never reword an acceptance criterion to make it match what happened.** If its premise turned
  out wrong, leave it unticked and say why. Nothing checks tick state, so an honest empty box is
  the only signal a later reader gets.
- Carry-over work is filed as a new iteration plan **before** the current PR merges.
- Every new `pub` item needs at least one non-test consumer in the same PR (review rule).
- Every `TODO`/`FIXME`/`XXX` needs an issue link or `// allow-todo: <reason>` (review rule).
- Every spec method change needs a live Firefox test, not just a unit test.
- Commit-message claims (`adds Foo::Bar`) must be backed by the branch diff (review rule).
- Iteration plans include `dogfood_path`, and `first_call_sites` if they add pub items. Validate:
  `cargo run -p xtask -- check-iteration-plan <plan>`
- Plan `status:` is `planned | in-progress | in-review | done | obsolete` — `done`, never
  `completed`.
- The ralph-loop and new-ralph-loop skill scripts are mirrored from `~/.claude/skills/*/scripts/`
  to `tools/*/scripts/`. **Edit both by hand — nothing checks you.** Verify with
  `diff -r ~/.claude/skills/<skill>/scripts/ tools/<skill>/scripts/`.
- Skill-edit iterations (those touching `~/.claude/skills/`) cannot run through ralph-loop — drive
  them by hand in a regular Claude session.

Why these rules exist, and why the gates that used to enforce them were deleted:
`kb/discipline-rationale.md`. Full contributor details: `CONTRIBUTING.md`. The repo has no
pre-commit hook.
