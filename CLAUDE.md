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

**`cargo test-live`'s `N passed; 0 failed` summary line does not mean N tests reached Firefox**
(iter-155). Every live test also checks its env gate at runtime and `return`s early when unset;
libtest counts that as `ok`, not `ignored`, so a test whose `FF_RDP_LIVE_NETWORK_TESTS` gate is
unset reports exactly the same as one that actually ran. Use `cargo run -p xtask -- live-sweep`
for a real sweep: it classifies each gated test from its own `#[ignore = "…"]` reason, runs only
the ones whose env var(s) are set (with `--include-ignored`), and runs the rest without
`--include-ignored` so libtest reports them `ignored`. It prints a machine-readable
`LIVE_SWEEP_SUMMARY executed=N skipped=M preexisting=K total=T` line — that `executed=N`, not a
`cargo test-live` pass count, is what a `[verified: <date>, …]` AC annotation should quote.
`preexisting=K` (iter-158 Theme F) counts env-qualified tests that need a Firefox somebody else
started on port 6000; the sweep probes that port once and, finding nothing, reports them
`ignored` rather than folding them into `executed`. Start one with
`firefox -no-remote --start-debugger-server 6000 --headless` to execute them.

**A `LIVE_SWEEP_SUMMARY` is meaningless without the env gates that produced it — always quote
both.** Two sweeps are comparable only when the same gates were set. Measured across the 158–161
batch (2026-08-14): iter-159 ran with both gates and got `executed=237 skipped=0`, 225 passed /
3 failed; iter-160 ran with `FF_RDP_LIVE_TESTS=1` alone and got `executed=209 skipped=32`, 209
passed / **0 failed**. The second looks like the better result and is a smaller one — the 32
`FF_RDP_LIVE_NETWORK_TESTS` tests did not run, and they include `live_block_url_pattern`, a real
product defect ([[iteration-164-two-failures-the-158-sweep-uncovered]]) that only fails when it
executes. `0 failed` over a shrunken corpus is the same false green this section exists to
warn about, one level up. Cite sweeps as `FF_RDP_LIVE_TESTS=1 [FF_RDP_LIVE_NETWORK_TESTS=1] →
LIVE_SWEEP_SUMMARY …`, and never compare a `skipped=0` run against a `skipped>0` one without
saying so.

**AC checkbox convention**: every AC checkbox in an iteration plan should name the test and the
asserted post-condition, e.g.:
```
- [ ] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR
```
This is **authoring guidance, not a gate**. An AC is the spec you implement against; write it so
the test to run is obvious. Recording a measured result when you tick a `live_*` AC is still worth
doing for the next reader:
```
- [x] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR [2026-08-12: 2400 px ≥ 1200 × 2]
```

**Nothing checks tick state.** iter-162b deleted `ac-fidelity-check.sh` and with it the
`[verified: …]` / `[deferred — new plan: …]` / `[allow-ac-wording: …]` machinery. A gate that reads
a plan and a diff cannot tell whether a test ran; over its life it produced 28 commits whose only
content was rewording an AC to silence it, against one catch a human had already found, and in the
158–161 batch it scored zero catches, seven false positives, and one induced falsification (five
iter-158 ACs merged both ticked *and* deferred). What replaces it is not another checker:

- **Tick honestly, or leave it unticked.** If an AC's premise turned out to be wrong, say so in the
  plan and leave the box empty. Never reword an AC so it matches what happened.
- **Carry-over is filed before the PR merges** — see the Iteration discipline section.
- **Product-source iterations paste a real live sweep into the PR body**, with the env gates that
  produced it (see the Live tests section). That reads execution; an AC annotation does not.

**Standing policy — every iteration touching product source runs a full sweep before its PR:**
```sh
FF_RDP_LIVE_TESTS=1 [FF_RDP_LIVE_NETWORK_TESTS=1] cargo run -p xtask -- live-sweep
```
Paste the real `LIVE_SWEEP_SUMMARY` line, the pass/fail counts and the gates you set. Do not
paraphrase and do not reuse an earlier run's numbers. This is not ceremony: on iter-160 the sweep
caught three failures the targeted tests missed, one of which would have broken every
cross-origin frame click. An isolated `cargo test live_foo` is not a substitute — iter-153 shipped
a broken feature certified by a truthful isolated run that passes in isolation and fails under
contention.

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
  This is a review rule, not a gate: iter-162a deleted `check-dead-primitives` after it
  induced a decoy (`DemuxReader::new()` constructed in `daemon/server.rs` purely to
  satisfy it — see the comment at `daemon/server.rs:750-756`) while 425 lines of dead
  public API shipped past it and survived every CI run until a human found them.
- Every `TODO`/`FIXME`/`XXX` must include a GitHub issue link, Jira ticket, or `// allow-todo: <reason>`.
  Also a review rule: `check-todo-annotations` and the pre-commit hook that duplicated it
  were deleted in iter-162a, having guarded an empty set (0 hits in
  `crates/ff-rdp-{core,cli}/src`) for their whole lifetime.
- Every spec method change must have a live Firefox test, not just a unit test.
- Carry-over work must be filed as a new iteration plan BEFORE the current PR merges.
- Carry-over is the discipline that replaced the AC gate — treat it as load-bearing.
  At the end of an iteration, enumerate every AC left unticked, every deferral, and every
  out-of-scope finding, and for each one either fold it into the next iteration's plan or
  file a new plan. List the dispositions in the PR body under `## Carry-over`. This is what
  actually caught 158→159 and 159→160; the gate that was supposed to caught nothing.
- Never reword an acceptance criterion to make it match what happened. If its premise turned
  out wrong, leave it unticked and say why in the plan. Nothing checks tick state any more
  (iter-162b), so an honest empty box is the only signal a later reader gets.
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
  "implements RdpError::Navigation") must be backed by the branch diff — a review rule now.
  `claims-vs-code.sh`, which emitted a `## Claims vs code` PR section, was deleted in
  iter-162b: advisory from day one, it never fired, and its promotion to a hard gate
  ([[iteration-61aa-claim-miss-hard-gate]]) was closed obsolete.
- Iteration plans must include `dogfood_path` and `first_call_sites` (if new pub items).
  Validate with: `cargo run -p xtask -- check-iteration-plan kb/iterations/iteration-NN-slug.md`
- **Before `/create-pr` on an iter-* branch, run the discipline gates.** There is no
  aggregator subcommand — iter-162a deleted `check-iteration-ready` because it hard-coded
  its own sub-check list, so every gate change cost a count bump and an assertion edit.
  Enumerate what xtask actually ships and run each gate:
  ```bash
  cargo run -q -p xtask -- --help          # list the check-* subcommands
  cargo run -p xtask -- check-<name> ...   # run each one
  ```
  Do not invent subcommand names that are not in the help output. Fix every reported failure
  before pushing. Most gates are local-only — do not assume CI will catch what you skip.
- The CI `discipline` job runs two: `check-live-test-layout` and
  `check-source-invariants` (the merged daemon-locks / error-envelope-paths /
  stderr-annotations scans).
  Two more are useful but local-only:
  - `check-firefox-refs <plan>` — validates `firefox_refs:` line ranges in an iteration plan
    against the local Firefox checkout (`FF_RDP_FIREFOX_PATH`). The only gate that checks a
    claim against ground truth *outside* the repository; both of its catches were false
    Firefox spec citations stopped before merge.
  - `check-actor-kb-sync --since origin/main` — fails if an actor `.rs` file was changed
    without a corresponding `kb/rdp/actors/*.md` update. A docs-sync reminder, not a defect gate.
- The ralph-loop skill scripts live in `~/.claude/skills/ralph-loop/scripts/`;
  a mirror is checked in at `tools/ralph-loop/scripts/` so changes are
  reviewable. Edit both — **by hand, with nothing checking you**. `check-discipline-regression`
  caught mirror drift until iter-162b deleted it along with the two scripts whose
  quadruplication was the only reason it existed. This is a real loss of safety, accepted
  because the gate guarded an obligation created by the machinery it guarded.
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
- See `CONTRIBUTING.md` for full details. The repo has no pre-commit hook: `.githooks/`
  held exactly one, a duplicate of `check-todo-annotations`, and both went in iter-162a.
