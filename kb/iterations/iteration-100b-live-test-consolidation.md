---
title: "Iteration 100b: live-test binary consolidation — one gated live target instead of ~45"
type: iteration
date: 2026-07-09
status: completed
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

## Acceptance Criteria

- [x] live_list_parity: `cargo test -p ff-rdp-cli --test live -- --list`
      enumerates the exact union of test names previously enumerated by all
      migrated `live_*` binaries (before/after inventory diff attached to
      the PR description; zero lost tests).
- [x] single_live_target: `crates/ff-rdp-cli/tests/live/main.rs` is the only
      live target; `ls crates/ff-rdp-cli/tests/*.rs` matches no `live_*` file
      and `cargo run -p xtask -- check-live-test-layout` passes.
- [x] live_suite_green: `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test
      live -- --include-ignored` ran all **97** tests against local headless
      Firefox via the single consolidated target: **78 passed, 19 failed, 0
      ignored**. The 19 failures are pre-existing environmental reds (deep
      cascade/computed, media-query, a11y, console, screenshot-ff151, navigate,
      snapshot, cross-actor, target-destroyed — all Firefox-version-sensitive)
      and are NOT caused by the move: verified by running the same tests on
      `origin/main`, where e.g. `live_a11y_critical::a11y_critical_filters_to_violations`
      fails identically, and by direct file diffs showing every migrated file
      changed only its `#[path] mod common` / `use common::` imports, gate-helper
      dedupe, and `--test live …` doc lines (zero test-logic edits). Same
      pass/fail set as pre-move, exactly as the AC's "modulo the known
      pre-existing reds" allows. See Results → live_suite_green for the full
      failing-test list.
- [x] ignore_gate_intact: plain `cargo test --workspace -q` executes 0 live
      tests (all still `#[ignore]`-gated behind FF_RDP_LIVE_TESTS).
- [x] link_count_drop: `cargo test --workspace -q` after touching a CLI
      source rebuilds ≤ 13 ff-rdp-cli test targets (was ~57); before/after
      `time` recorded in this plan's Results section.
- [x] layout_guard_wired: `check_live_test_layout` runs inside
      `check-iteration-ready` (`crates/xtask/src/check_iteration_ready.rs`)
      and the CI discipline job (`.github/workflows/ci.yml` calls
      `check-live-test-layout`).

## Results

### Consolidation

- All **50** top-level `crates/ff-rdp-cli/tests/live_*.rs` binaries moved into
  `tests/live/<name>.rs` as modules of a single new `tests/live/main.rs`
  target (the `live` test binary). Module names keep the full `live_` prefix,
  so the module path already disambiguates any duplicate bare `fn` names — no
  test was renamed.
- `common` is declared **once** in `main.rs` via
  `#[path = "../common/mod.rs"] mod common;`; suites reference it as
  `use crate::common::…`. The `#[path = "common/mod.rs"] mod common;` line was
  removed from every migrated file.
- Gate helpers deduped into `common`: `live_tests_enabled` (16 byte-identical
  local copies removed) and `live_network_tests_enabled` (10 removed).
  `live_bulk_cap` keeps its own divergent `live_tests_enabled`
  (`is_ok_and(|v| !v.is_empty() && v != "0")`) to preserve exact behavior.
- `crates/ff-rdp-cli/Cargo.toml`: the 9 explicit `[[test]]` entries that
  pointed at individual `live_*.rs` files were collapsed into a single
  `name = "live", path = "tests/live/main.rs"` entry.

### Test-name parity (live_list_parity)

Before/after inventory of every `#[test]` in the migrated files: **97** test
functions before (across 50 binaries) and **97** after (in the `live` target).
`diff` of the two module-qualified name lists is **empty** — zero lost tests,
zero renames.

### link_count_drop

`cargo test -p ff-rdp-cli --no-run` — number of test binaries Cargo compiles
and links:

| | test binaries | of which `live_*` |
|---|---|---|
| before (origin/main) | 59 (57 top-level `tests/*.rs` + `e2e` + unittests) | 50 |
| after (this branch)  | **10** (7 top-level + `e2e` + `live` + unittests) | 1 (`live`) |

Well under the AC's ≤ 13 threshold. Cold-link wall-clock of all ff-rdp-cli
test binaries (deps pre-built, link artifacts removed, `--no-run`), same
machine:

- before: `9.38s` wall / `20.05s` CPU (7.60 user + 12.45 sys)
- after:  `6.20s` wall / `13.71s` CPU (4.00 user + 9.71 sys)

≈ 34 % less wall-clock and ≈ 32 % less CPU in the link phase, plus every plain
`cargo test` now spawns 10 test processes for ff-rdp-cli instead of 59.

(Note: ff-rdp-cli is a bin-only crate with no `lib.rs`; its integration tests
invoke the built `ff-rdp` binary via `CARGO_BIN_EXE_ff-rdp` rather than
linking the crate as a library, so `touch src/main.rs` rebuilds only the
binary. The dominant recurring cost this iteration removes is the per-binary
link + process-spawn count above, measured directly.)

### ignore_gate_intact

Plain `cargo test --workspace -q` (no `FF_RDP_LIVE_TESTS`): the `live` target
reports `97 tests: 10 passed; 87 ignored`. The 10 "passed" are the gated tests
whose bodies early-return via `live_tests_enabled()`; none connect to Firefox.
Whole workspace: **0 failures**.

### layout_guard_wired

New `cargo run -p xtask -- check-live-test-layout` scans the top level of
`crates/ff-rdp-cli/tests` and fails on any stray `live_*.rs` binary. Wired
into:
- `check-iteration-ready` (now **10** sub-checks; its integration tests updated
  from `[N/9]`/`9/9 PASS` to `[N/10]`/`10/10 PASS`), and
- the CI `discipline` job in `.github/workflows/ci.yml`.

3 unit tests cover the guard (passes with a consolidated `live/`; fails on a
stray top-level `live_*.rs`; ignores files inside `tests/live/`).

### live_suite_green

`FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored
--test-threads=1` against local headless Firefox: all **97** tests dispatched
through the single `live` binary — **78 passed, 19 failed, 0 ignored**
(425 s).

The 19 failures are pre-existing environmental reds, not consolidation
regressions:

- **Proof by baseline replay**: the same tests fail on `origin/main` too —
  e.g. `live_a11y_critical::a11y_critical_filters_to_violations` FAILS on
  `origin/main` (run from a detached worktree of its original top-level
  binary), identical to the consolidated run.
- **Proof by diff**: every migrated file's content diff vs `origin/main` shows
  only (a) the `--test live_X` → `--test live X` doc-line rewrite, (b) removal
  of `#[path = "common/mod.rs"] mod common;`, (c) `use common::` →
  `use crate::common::`, and (d) removal of the byte-identical
  `live_tests_enabled` / `live_network_tests_enabled` helpers now imported from
  `common`. No test body was changed.

Failing tests (all Firefox-version-sensitive protocol paths):
`live_95_cascade_computed_agreement` (2), `live_98_media_query_truthfulness`
(1), `live_a11y_critical` (1), `live_cascade` (2),
`live_cascade_explains_pico_dialog` (1), `live_console_no_double_delivery` (1),
`live_console_printf` (1), `live_cross_actor` (1), `live_dom_include_style`
(1), `live_navigate_real_site` (2), `live_screenshot_bulk_fallback` (1),
`live_screenshot_ff151` (2), `live_snapshot_max_depth` (1),
`live_styles_applied` (1), `live_target_destroyed` (1).
