# Agents
Delegate the work to agents whenever possible to avoid automatic context compaction. For any
bounded code change use the `ff-rdp-implementer` agent (`.claude/agents/`): it carries the landing
protocol, so the prompt only needs the finish line, file ownership and the proofs required.

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

Run **once, after the last code change**, in this order, and fix all issues:
1. `cargo fmt`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace -q`

Never commit code that fails any of these. Do not re-run them in a later step (PR creation,
review) when the tree is unchanged since the last green run — `git rev-parse HEAD^{tree}` (or
`git write-tree` for uncommitted work) tells you. CI is the authority: read `gh pr checks <PR>`
rather than substituting a local run for it (CI's `stable` may be newer than yours —
`kb/decision-log.md` DEC-044).

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
- Every spec method change needs a test.
- Every new `pub` item needs at least one non-test consumer in the same PR (review rule).
- Every `TODO`/`FIXME`/`XXX` needs an issue link or `// allow-todo: <reason>` (review rule).

## Live tests
Env gates: `FF_RDP_LIVE_TESTS=1` (launches headless Firefox locally), `FF_RDP_LIVE_NETWORK_TESTS=1`
(also makes real network requests), `FF_RDP_LIVE_SITES_TESTS=1` (also drives third-party sites —
BBC, Guardian, HN, Wikipedia, MDN; weekly CI job only).

- **The full sweep is not a per-PR gate.** It runs nightly on `main` (`.github/workflows/live.yml`,
  `cargo run -p xtask -- live-sweep` at default parallelism), on demand, and at release.
- **Per PR**, run only the live tests for the modules you touched:
  `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live <module> -- --include-ignored`.
  Tests whose gate is unset return early and still print `ok` — set every gate the module's
  `#[ignore]` reasons name before reading the result as a pass.
- **Flakes:** a live test that fails and then passes on one re-run is quarantined with
  `#[ignore = "flaky: issue #N"]` plus a GitHub issue. Never an iteration plan, never an
  "attribution" investigation. A red nightly gets one GitHub issue.

## PR Discipline
- One iteration = one branch = one PR; branch `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself
- Plan ticks and `status: done` go in the **same commit** as the code, so the PR triggers one CI run.
- One review pass; no re-review for style-only fixes.
- Commit-message claims (`adds Foo::Bar`) must be backed by the branch diff (review rule).
- Merge **on GitHub** (`gh pr merge <n> --merge --delete-branch`), never a local `git merge` +
  `git push origin main`. The `main` ruleset requires the `ci` status check (since 2026-10-03),
  so a direct push is refused and a red PR cannot merge. A refused merge is the enforcement
  working: report the reason and stop. Never `--squash`.
- **Docs-only changes go through a PR too** — `main` rejects direct pushes. CI's `changes` gate
  skips the Rust matrix for `kb/**` and `*.md`, so `ci` is green in ~20 s:
  `gh pr create --fill && gh pr merge --merge --delete-branch --auto`.

## Autonomous merges — standing authorization from the repo owner

This is a solo repository. I, James (github: ractive), the repository owner,
explicitly and durably authorize Claude Code — including subagents and
workflow agents running iteration loops on my behalf — to merge iteration PRs
into `main` **without a per-PR human approval**, provided all of the following
hold:

1. All CI checks on the PR head are green.
2. A local review pass (e.g. `/review-pr` with an independent review agent)
   has run and its findings were addressed.
3. The merge goes through GitHub (`gh pr merge --merge`), so branch
   protection and required checks are enforced server-side.

Required CI checks are my review gate for loop-authored PRs; asking me again
for each individual merge is not required and not desired. Stated explicitly
on 2026-08-29 (restating the 2026-08-17 launch-time authorization used for
PRs #228/#229) so that iteration loops on this repo no longer need it restated
per run. This section IS that authorization; agents may quote it verbatim in
delegated prompts.

## Iteration discipline
- **Never reword an acceptance criterion to make it match what happened.** If its premise turned
  out wrong, leave it unticked and say why.
- Acceptance criteria describe user-visible behaviour.
- Iteration plans use a number no other plan claims. Validate:
  `cargo run -p xtask -- check-iteration-plan <plan>` (`dogfood_path` / `first_call_sites` are
  optional).
- Plan `status:` is `planned | in-progress | in-review | done | obsolete` — `done`, never
  `completed`.
- Follow-up work goes in `kb/backlog.md` (one line per item, with the PR it came from), ticked in the same commit as the fix; file a new plan only for product work that needs design. Not GitHub issues.

Why the process was cut back on 2026-10-02: `kb/research/step-back-2026-10-02.md` and
`kb/discipline-rationale.md`. Contributor details: `CONTRIBUTING.md`. The repo has no pre-commit
hook.
