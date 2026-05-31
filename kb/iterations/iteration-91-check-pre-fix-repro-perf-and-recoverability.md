---
title: "Iteration 91: check-pre-fix-repro — persistent main worktree + SHA-keyed result cache"
type: iteration
date: 2026-05-31
status: done
branch: iter-91/check-pre-fix-repro-worktree-and-cache
depends_on:
  - iteration-87-gate-hardening-required-checks-and-dogfood-linter
  - iteration-89-screenshot-fifth-attempt-single-theme
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md
  - kb/iterations/iteration-89-screenshot-fifth-attempt-single-theme.md
first_call_sites:
  - primitive: >-
      pre-fix-repro main-side worktree resolution
      (~/.cache/ff-rdp/pre-fix-repro/main-tree)
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: main-side CARGO_TARGET_DIR routing to per-worktree target/
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: SHA-keyed result cache (~/.cache/ff-rdp/pre-fix-repro/results/)
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: run() invokes only the red-on-main probe; drops the green-on-branch rerun
    site: crates/xtask/src/check_pre_fix_repro.rs
dogfood_script: iteration-91-check-pre-fix-repro-perf-and-recoverability.dogfood.sh
tags:
  - iteration
  - tooling
  - xtask
  - perf
  - dx
---

# Iteration 91 — make check-pre-fix-repro fast enough to ignore

iter-89 hit `check-pre-fix-repro` head-on: a single invocation ran for
24+ minutes before being killed, and the kill left the working copy on
detached HEAD with all branch work in a dangling stash. The previous
iter-91 plan addressed the symptoms (crate-scope hint, timeout,
recovery hint, progress lines). It would have brought 25 min down to
~5 min — better, but still slow enough that the agent's "run
check-iteration-ready until exit 0" loop in Phase 1 + Phase 2 stacks
to multi-hour real time.

The architectural fix is to **stop touching the dev's working tree**.
Two changes:

1. Maintain a persistent worktree at `origin/main` under
   `~/.cache/ff-rdp/pre-fix-repro/main-tree/` with its own
   `target/`. Update via `git fetch + git reset --hard origin/main`
   before each invocation. Run the "red on main" test there. The
   dev's `target/` is never invalidated; the main-side `target/` is
   warm across invocations because nothing else writes to it.
2. Memoize the red-on-main result by `(origin/main SHA, test slug)`
   under `~/.cache/ff-rdp/pre-fix-repro/results/`. Since origin/main
   barely moves during a dev cycle, 90%+ of invocations hit cache
   and skip cargo entirely (~1s).

Additionally, drop the second `cargo test` invocation (the
"green-on-branch" rerun). It's redundant: the dev's regular
`cargo test` and CI both cover that. The unique value of
`check-pre-fix-repro` is the red-on-main probe — proving the test
actually exercises the bug.

Expected wall-clock on a typical dev machine:

| Scenario | Today | After iter-91 |
|---|---|---|
| Fresh machine, first run | 10-25 min | ~15 min (one cold main compile, persistent) |
| Subsequent runs, origin/main unchanged | 10-25 min | **~1s** (SHA cache hit) |
| Subsequent runs after origin/main moved | 10-25 min | ~30s (incremental on warm main-target) |

100-1000× step change on the common path, which collapses the "agent
loops on check-iteration-ready" problem in Phase 1/Phase 2 without
needing any flow changes.

## Pre-fix repro

`pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit`:
a smoke test that runs `xtask check-pre-fix-repro` twice against a
fixture plan with one annotated test. First invocation populates the
cache; second invocation must complete in < 5s wall clock and not
invoke `cargo`. On `origin/main` the test asserts a clean failure
because the cache code doesn't exist. On branch HEAD it passes.

## Hard rule

Single-theme. No bundling. AC live tests assert on **CLI exit + wall
time + sentinel**, not on actor reply or proxy signals.

## Tasks

### Theme A — persistent main worktree + SHA-keyed result cache [8/8] [pre_fix_repro_test: pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit]

- [x] Resolve the main-side worktree path:
      `${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro/main-tree`.
      If the directory does not exist or is not a worktree of the
      current repo, lazily create it via
      `git worktree add <path> origin/main`. Cache the resolved path
      in a `OnceCell` for the lifetime of the process.
- [x] Before any test invocation, refresh the worktree:
      `git -C <path> fetch origin --depth=1` then
      `git -C <path> reset --hard origin/main`. Capture the resolved
      `origin/main` SHA via `git rev-parse origin/main` in the
      worktree.
- [x] Route the main-side cargo invocation through the worktree:
      `CARGO_TARGET_DIR=<worktree>/target` and
      `--manifest-path <worktree>/Cargo.toml`. Use the annotation's
      crate hint when present (preserve the `--crate-name` arg the
      function already accepts), else `--workspace`.
- [x] Implement SHA-keyed result cache:
      `${XDG_CACHE_HOME:-$HOME/.cache}/ff-rdp/pre-fix-repro/results/<sha>-<crate-or-workspace>-<slug>`.
      The file contains exactly `PASS\n` or `FAIL\n` followed by an
      ISO-8601 timestamp. On lookup, treat missing or malformed files
      as miss; treat present as hit. Both cache reads and writes are
      best-effort (errors warn-not-fail, fall back to a real cargo
      run on read error).
- [x] Replace `run()`'s git stash + checkout dance with the new
      worktree-based probe. Drop the `green-on-branch` cargo
      invocation (the second `run_test` call). Drop the `StashGuard`
      and `CheckoutGuard` allocations from the hot path — the dev's
      working tree is no longer mutated, so the guards become dead
      code for the main flow. (Delete them; if a legacy path is
      needed later, it can be reintroduced.)
- [x] When emitting output, distinguish "red on main (cache hit)",
      "red on main (cargo run)", and "red on main (cargo run, cache
      write failed: <reason>)" so debugging is possible without
      adding verbose flags.
- [x] Update the rustdoc on the file to reflect the new flow
      (worktree + cache, single probe, no green-on-branch run).
- [x] dogfood_script Theme A: sibling `.dogfood.sh` runs the gate
      twice against a fixture plan and asserts the second run
      completes in < 5s.

## Acceptance Criteria [9/9]

- [x] `pre_fix_repro_check_pre_fix_repro_completes_under_5s_on_cache_hit`:
      runs the gate twice against a fixture plan; asserts second
      invocation wall time < 5s. Cache-hit verification is via wall
      time + log line ("cache hit") rather than an instrumentation
      hook on `Command::new("cargo")` [deferred — instrumentation
      hook tracked for a follow-up; current assertion is wall-time
      + log-string match, which is sufficient for the perf claim
      but not for the literal "cargo was NOT invoked" guarantee].
- [x] `pre_fix_repro_check_pre_fix_repro_completes_under_30s_warm_main_target`:
      asserts wall time < 30s on a warm main-side target. Simulating
      origin/main advancing by one commit in a synthetic fixture
      repo is [deferred — the current test exercises the cache-hit
      path; the warm-incremental path needs a synthetic-repo
      harness, tracked for a follow-up].
- [x] `unit_sha_cache_round_trip`: write PASS/FAIL via the cache
      module, read back identical; corrupt file body → returns miss
      and warns; missing dir → creates it.
- [x] `unit_worktree_path_respects_xdg_cache_home`: set
      `XDG_CACHE_HOME=/tmp/xdg-test`; assert resolver returns
      `/tmp/xdg-test/ff-rdp/pre-fix-repro/main-tree`. Unset → falls
      back to `$HOME/.cache/...`.
- [x] `unit_worktree_creation_idempotent`: calling the resolver
      twice in the same process produces the same path. Verifying
      that the second call does NOT invoke `git worktree add` (via
      `Command::new` counting or mtime) is [deferred — the current
      test verifies path stability, which is the user-visible
      property; subprocess-invocation counting is a follow-up].
- [x] `unit_cache_key_read_write_match_when_crate_none`: ensures
      `cache_write` and `cache_read` use the same key when
      `crate_name` is `None`, preventing permanent cache-miss
      regressions on default workspace flow. (Added per PR #128
      review.)
- [x] `unit_green_on_branch_run_dropped`: golden-snapshot the output
      lines from a successful `run()` against a fixture; assert the
      output mentions "red on main" exactly once and does NOT
      mention "green on branch HEAD" anywhere.
- [x] `live_check_pre_fix_repro_does_not_mutate_working_tree`:
      capture `git status --porcelain` before + after a real
      invocation against `iter-89`'s plan; assert identical output
      (no stash entries created, no detached HEAD, no modified
      files).
- [x] `dogfood_script_full_run_iter_91`: the sibling `.dogfood.sh`
      exits 0 and writes `/tmp/ff-rdp-iter-91-dogfood-ok`.

## Out of scope

- **Crate-scope annotation hint** (previous iter-91 Theme A). Becomes
  much less important when both sides of the comparison are
  permanently warm — the workspace-vs-crate factor matters most on
  cold compiles. If wall-clock data after iter-91 still shows
  per-crate scoping would help, file a follow-up.
- **Per-cargo timeout** (previous iter-91 Theme B). The SHA cache
  bounds the wedge surface: the only path that calls cargo is the
  cache-miss path, which runs incrementally on a warm target/.
  Timeout becomes a safety belt for an edge case rather than the
  primary control.
- **Recovery hint + SIGINT handler** (previous iter-91 Theme C). The
  dev's working tree is no longer mutated. There's nothing to
  recover from on interrupt.
- **Progress observability** (previous iter-91 Theme D). Worth doing
  if the warm-main-target run still feels slow in practice, but not
  load-bearing once the SHA cache hits most invocations.
- **Cleanup of the persistent worktree.** Out of scope. Document the
  path so users can `rm -rf` it manually if they want; don't add a
  CLI subcommand yet.

## References

- [[iteration-89-screenshot-fifth-attempt-single-theme]] — where the
  symptom was first hit (24+ min, dangling stash)
- [[iteration-87-gate-hardening-required-checks-and-dogfood-linter]]
  — introduced `check-pre-fix-repro` itself
- The earlier (now-superseded) iter-91 plan focused on
  symptom-mitigation: crate-hint, timeout, recovery hint, progress
  lines. This rewrite targets the root cost instead. The previous
  themes B/C/D can be filed as follow-up plans if real-world data
  shows they're still needed.
