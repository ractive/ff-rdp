---
title: "Iteration 91: check-pre-fix-repro — speed, scope, and recoverability"
type: iteration
date: 2026-05-31
status: planned
branch: iter-91/check-pre-fix-repro-perf-and-recoverability
depends_on:
  - iteration-87-gate-hardening-required-checks-and-dogfood-linter
  - iteration-89-screenshot-fifth-attempt-single-theme
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md
  - kb/iterations/iteration-89-screenshot-fifth-attempt-single-theme.md
first_call_sites:
  - primitive: "per-annotation crate hint parsed from `[pre_fix_repro_test: slug | crate=ff-rdp-core]`"
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: "`run_test` invokes `cargo test -p <crate>` instead of `--workspace`"
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: "per-cargo-invocation timeout (default 600s, env-configurable)"
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: "startup recovery hint printed before any stash/checkout"
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: "`run_test` passes `--include-ignored` so annotated tests gated by `#[ignore]` are executed"
    site: crates/xtask/src/check_pre_fix_repro.rs
dogfood_script: iteration-91-check-pre-fix-repro-perf-and-recoverability.dogfood.sh
tags:
  - iteration
  - tooling
  - xtask
  - perf
  - dx
---

# Iteration 91 — `check-pre-fix-repro` is too slow and too easy to wedge

iter-89 hit `check-pre-fix-repro` head-on: a single invocation against the
iter-89 plan ran for **24+ minutes** before being killed, and the kill left
the working copy on detached HEAD with all branch work in a dangling
stash. The bugs are not in the idea of "verify the test fails on main,
passes on branch" — they're in the execution.

## Pre-fix repro

`pre_fix_repro_check_pre_fix_repro_completes_under_300s`: a smoke test
that runs `xtask check-pre-fix-repro` against a fixture plan with one
annotated test and asserts wall-clock < 300s on a warm build. On
`origin/main` the test will time out; on branch HEAD it passes.

## Tasks

### Theme A — narrow the recompile scope [0/5] [pre_fix_repro_test: pre_fix_repro_check_pre_fix_repro_completes_under_300s]

- [ ] Extend the plan annotation grammar to accept an optional crate
      hint: `[pre_fix_repro_test: slug | crate=ff-rdp-core]`. Update
      `parse_pre_fix_repro_annotations` in
      `crates/xtask/src/check_pre_fix_repro.rs` to return the crate
      hint when present.
- [ ] In `try_list_tests` and `run_test`, when the annotation carries a
      crate hint, invoke `cargo test -p <crate>` instead of
      `--workspace`. Fall back to `--workspace` only when no hint is
      provided. This shrinks the per-checkout compile from "all 4
      crates" to "one crate + its deps".
- [ ] Backfill the existing iteration plans (`iter-87`, `iter-88`,
      `iter-89`) with the `crate=` hint so their pre-fix-repro tests
      use the narrower scope.
- [ ] Document the new annotation grammar in
      `kb/iterations/README.md` (or the closest existing index).
- [ ] Pass `--include-ignored` to every `cargo test` invocation issued by
      `run_test` / `try_list_tests`. Pre-fix-repro tests are routinely
      gated by `#[ignore]` plus an `FF_RDP_LIVE_TESTS` body guard, but the
      current runner uses `cargo test … --exact` without
      `--include-ignored`, so the test is silently filtered out and the
      "at least one passed" check fails on both revisions. Surfaced by
      iter-90 PR #127 review (`pre_fix_repro_daemon_state_sharing_red_then_green`);
      iter-90 worked around it by dropping `#[ignore]`, but the runner
      itself should not require that.

### Theme B — timeout guard around every `cargo test` invocation [0/2]

- [ ] Add a per-invocation timeout to `run_test` and `try_list_tests`
      using `wait-timeout = "0.2"`. Default 600s, override via env
      `FF_RDP_CHECK_PRE_FIX_REPRO_TIMEOUT_SECS`. On timeout, kill the
      child process group and return a clear error citing the slug and
      the elapsed seconds.
- [ ] Unit test `cargo_invocation_times_out_cleanly`: spawn a sleep
      shim via the timeout helper, assert the helper kills the child
      and surfaces a structured error within timeout + 5s.

### Theme C — recoverability when interrupted [0/3]

- [ ] On startup, before any `git stash` or `git checkout`, print a
      single-line recovery hint to stderr naming the current branch
      and the stash slot that will be created (e.g.
      `[check-pre-fix-repro] If interrupted, recover with: git checkout iter-89/… && git stash pop`).
- [ ] Install a `ctrlc` (or libc `signal`) handler that on SIGINT
      attempts the inverse: `git checkout <previous_ref> && git stash pop`.
      If the inverse fails (e.g. merge conflict), exit 130 leaving the
      printed hint as the user's recovery path.
- [ ] Unit test `recovery_hint_printed_before_any_git_mutation`:
      capture the tool's stderr; assert the hint appears before any
      `Command::new("git")` call by gating the git calls behind a
      printed-hint flag.

### Theme D — observability [0/1]

- [ ] When a sub-step's wall time exceeds 60s, print a progress line
      (`[check-pre-fix-repro] still compiling on origin/main … 90s elapsed`)
      every 30s. Implemented via a background thread that polls a
      shared `Instant`. Gives the user a signal that the tool is alive
      vs. wedged.

## Acceptance Criteria [0/6]

- [ ] `pre_fix_repro_check_pre_fix_repro_completes_under_300s`: warm
      build finishes inside 300s on a single-crate annotated plan.
- [ ] `cargo_invocation_times_out_cleanly`: timeout helper kills the
      child within the configured window.
- [ ] `recovery_hint_printed_before_any_git_mutation`: hint emitted
      before any `git stash`/`git checkout`.
- [ ] `annotation_grammar_accepts_crate_hint`: parser accepts the
      `| crate=<name>` suffix and rejects malformed variants without
      panicking.
- [ ] `runner_executes_ignored_pre_fix_repro_test`: a fixture plan
      annotates a `#[ignore]`-gated test; the runner passes
      `--include-ignored` so the test is actually executed (not silently
      filtered) and the red/green outcome matches the test body.
- [ ] `dogfood_script_full_run_iter_91`: sibling `.dogfood.sh` exits 0
      and writes `/tmp/ff-rdp-iter-91-dogfood-ok`. The script runs
      `xtask check-pre-fix-repro` against an annotated fixture plan
      and asserts (a) wall time < 300s, (b) tool exits 0, (c) no
      orphan stash entries remain, (d) the working branch is unchanged.

## Out of scope

- Replacing `cargo` with `cargo nextest`. The single-crate scope is
  the dominant win; nextest is a separate dependency-policy question.
- Adding `sccache`. Useful but workflow-wide; track separately.
- Restructuring `check-iteration-ready` to run sub-checks in parallel.
  Worth doing later, but parallelism amplifies the recoverability
  problem; fix that first.
- The detached-HEAD recovery on SIGKILL. `Drop` guards can't run on
  SIGKILL by definition; the printed hint covers this case.

## References

- [[iteration-89-screenshot-fifth-attempt-single-theme]] — the run
  that exposed the bug
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]] —
  introduced `check-pre-fix-repro`
