---
title: "Iteration 100b: live-test binary consolidation — one gated live target instead of ~45"
type: iteration
date: 2026-07-09
status: planned
branch: iter-100b/live-test-consolidation
depends_on:
  - kb/iterations/iteration-100-daemon-lifecycle-hardening.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-46-e2e-test-consolidation.md
first_call_sites: []
dogfood_path: |
  # Enumerate the consolidated live suite (no Firefox needed):
  cargo test -p ff-rdp-cli --test live -- --list
  # Run one migrated module for real:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_96 -- --include-ignored --nocapture
  # The win: a workspace test pass no longer links ~45 live binaries.
  touch crates/ff-rdp-cli/src/main.rs && time cargo test --workspace -q
tags:
  - iteration
  - tests
  - build-performance
---

# Iteration 100b — live-test binary consolidation

## Motivation

[[iteration-46-e2e-test-consolidation]] merged 29 e2e binaries into one
`tests/e2e/main.rs` target, but the same disease regrew in a second
population: every iteration since has added its live Firefox tests as a NEW
top-level `crates/ff-rdp-cli/tests/live_*.rs` file. There are now **~45
`live_*` integration-test binaries** (57 top-level test targets total in
ff-rdp-cli). Every `cargo test` compiles, links, and executes all of them
even when every live test is `#[ignore]`-skipped — linking dominates, and
both the implement and review phases of every iteration pay it repeatedly.
This is the single biggest wall-clock drag on the iteration loop.

## Theme A — consolidate `live_*.rs` into one `tests/live/` target

Mirror the iter-46 e2e structure exactly:

- Create `crates/ff-rdp-cli/tests/live/main.rs` declaring one `mod` per
  migrated suite (module name = old file name minus `live_` prefix where
  unambiguous; keep iteration-numbered names as-is, e.g. `mod
  live_96_profile_cleanup;` is fine).
- Move each `tests/live_*.rs` to `tests/live/<name>.rs` verbatim, preserving
  the `//!` doc headers (update the `Run with:` lines to the new
  `--test live <filter>` form).
- `common`: today each file does `#[path = "common/mod.rs"] mod common;`.
  In the consolidated target declare it ONCE in `main.rs` via
  `#[path = "../common/mod.rs"] mod common;`; modules switch to
  `use crate::common::…`.
- Dedupe the ~45 copies of `live_tests_enabled()` / env-gate helpers into
  `common` (or a `live_support` module) — one definition, used by all.
- Test names must stay unique enough for `--list` parity; module paths
  disambiguate duplicates (e.g. several `smoke` fns) — rename only where the
  harness reports a genuine conflict, and record any rename in the PR
  description.

## Theme B — pin the convention so it doesn't regrow

- CONTRIBUTING.md (test layout section): new live tests go in
  `crates/ff-rdp-cli/tests/live/<slug>.rs` + a `mod` line in
  `tests/live/main.rs`. A new top-level `tests/live_*.rs` file is a review
  defect.
- Add an xtask guard: `check-live-test-layout` — fails if any
  `crates/ff-rdp-cli/tests/live_*.rs` exists. Wire it into
  `check-iteration-ready` and CI's discipline job so ralph-loop agents can't
  regress it silently. (This is the one new pub-ish surface; it is its own
  first consumer via check-iteration-ready wiring — first_call_sites stays
  empty as it's xtask-internal.)

## Non-goals

- The `e2e` target, `cli_*.rs`, `error_shapes.rs`, `eval_object_leak_soak.rs`,
  `playbook_evals.rs`, `dom_help_mentions_styles.rs` binaries stay untouched
  (candidates for a later sweep; `cli_cookies_help.rs` intentionally stays a
  separate binary — its small-stack repro relies on process-level layout).
- No test-logic changes. Moves + import fixes + gate-helper dedupe only.
- ff-rdp-core's 9 and xtask's 11 test targets are out of scope.

## Acceptance criteria

- [ ] live_list_parity: `cargo test -p ff-rdp-cli --test live -- --list`
      enumerates the exact union of test names previously enumerated by all
      migrated `live_*` binaries (before/after inventory diff attached to
      the PR description; zero lost tests).
- [ ] single_live_target: `ls crates/ff-rdp-cli/tests/*.rs` matches no
      `live_*` file; `tests/live/main.rs` is the only live target and
      `cargo run -p xtask -- check-live-test-layout` passes.
- [ ] live_suite_green: `FF_RDP_LIVE_TESTS=1 cargo test-live` passes locally
      against headless Firefox for the consolidated target (same pass/skip
      counts as pre-move, modulo the known pre-existing reds).
- [ ] ignore_gate_intact: plain `cargo test --workspace -q` executes 0 live
      tests (all still `#[ignore]`-gated behind FF_RDP_LIVE_TESTS).
- [ ] link_count_drop: `cargo test --workspace -q` after touching a CLI
      source rebuilds ≤ 13 ff-rdp-cli test targets (was ~57); before/after
      `time` recorded in this plan's Results section.
- [ ] layout_guard_wired: check-live-test-layout runs inside
      `check-iteration-ready` and the CI discipline job (visible in the PR
      diff of .github/workflows + xtask).

## Results

(to be filled by the implementing iteration)
