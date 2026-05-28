---
title: "Iteration 87: gate hardening — required CI checks, fail-by-default dogfood gate, dogfood-script linter, pre-fix-repro convention"
type: iteration
date: 2026-05-29
status: planned
branch: iter-87/gate-hardening-required-checks-and-dogfood-linter
depends_on:
  - iteration-86-perf-field-report-fixes
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-58.md
  - kb/dogfooding/dogfooding-session-57.md
  - kb/iterations/iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.md
  - kb/iterations/iteration-86-perf-field-report-fixes.md
first_call_sites:
  - primitive: "tools/branch-protection.sh: asserts live-tests is a required check on iter-* branches"
    site: tools/branch-protection.sh
  - primitive: "check-dogfood-script: FAIL-by-default on iter-* branches when FF_RDP_LIVE_TESTS unset"
    site: crates/xtask/src/check_dogfood_script.rs
  - primitive: "tools/lint-dogfood-script.sh: shellcheck-plus-ff-rdp-specific linter"
    site: tools/lint-dogfood-script.sh
  - primitive: "xtask check-pre-fix-repro: verifies carry-over themes have red-on-main, green-on-branch tests"
    site: crates/xtask/src/check_pre_fix_repro.rs
  - primitive: iteration plan schema accepts per-theme `pre_fix_repro_test:` annotation
    site: crates/xtask/src/iteration_plan.rs
  - primitive: "check-iteration-ready: invokes lint-dogfood-script and check-pre-fix-repro"
    site: crates/xtask/src/check_iteration_ready.rs
dogfood_script: iteration-87-gate-hardening-required-checks-and-dogfood-linter.dogfood.sh
tags:
  - iteration
  - gate-hardening
  - meta-gate
  - testing-discipline
---

# Iteration 87 — make the gate inescapable

[[dogfooding-session-58]] is blunt: iter-85 and iter-86 both shipped with a
red `live-tests` CI job and both merged anyway. The mechanism iter-85 built
(`xtask check-dogfood-script`, sibling `.dogfood.sh`) is sound — both jobs
correctly failed with exit 1 and never wrote the sentinel. What's missing is
the human-process scaffolding around it:

1. The `live-tests` job is not a **required** GitHub branch-protection
   check, so a red job blocks nothing.
2. `check-dogfood-script` SKIPs silently when `FF_RDP_LIVE_TESTS` is unset.
   Anyone running `check-iteration-ready` locally without the env var sees
   green even when every theme is broken.
3. The dogfood scripts authored in iter-86 contain assertion bugs that no
   linter caught (`grep -qi 'headless'` matches "regardless of headless
   mode"; `--jq-strict` is a boolean flag but is invoked with a positional
   path).
4. There is no convention enforcing that "carry-over" iterations actually
   reproduce the bug they claim to fix before claiming the fix.

iter-87 is meta — it ships no user-visible CLI fix. Its product is gate
hardening so iter-88/89/90 (the actual carry-overs) cannot ship paper-only
fixes the way iter-85 did.

## Hard rule

Same convention as iter-85/86: do not tick an AC checkbox until
`iteration-87-….dogfood.sh` exits 0 against a live FF 151 and writes
`/tmp/ff-rdp-iter-87-dogfood-ok`. `check-iteration-ready` greps for the
sentinel. iter-87's dogfood script exercises the linter and the
fail-by-default gate against fixture scripts — it does not need to fix a
user-visible CLI bug to justify a live run, because the dogfood-script
gate itself is what's under test.

## Pre-fix repro convention (introduced here; referenced by iter-88/89/90)

Every theme in a carry-over iteration (an iteration that re-attempts a
previously-broken theme) MUST start with a test that FAILS on `origin/main`
BEFORE the fix lands and PASSES on the branch HEAD AFTER. The test is
named in the plan's theme heading via a `pre_fix_repro_test:` annotation:

```
### Theme A — cascade … [pre_fix_repro_test: pre_fix_repro_cascade_fixture_red_then_green]
```

`xtask check-pre-fix-repro --plan <plan>` parses these annotations and:

1. Asserts the test exists (by `cargo test --list` grep for the slug).
2. Stashes pending changes, checks out `origin/main`, runs the test, asserts FAIL.
3. Restores branch HEAD, runs the test, asserts PASS.
4. Restores stash.

Iterations with no carry-over themes skip the check (no annotations → no-op).
The check is wired into `check-iteration-ready` after the dead-primitives
gate and before the dogfood-script gate.

## Tasks

### Theme A — `live-tests` becomes a required branch-protection check [0/3]

- [ ] `tools/branch-protection.sh`: gh-cli script that queries the current
      branch-protection rule for `main` and asserts `live-tests` is in
      `required_status_checks.contexts`. Exits non-zero with a printable
      remediation command on missing.
- [ ] Documented in `CONTRIBUTING.md`: the exact `gh api` call to apply
      the protection rule, scoped to `iter-*` branches via the
      `enforce_admins` + `required_status_checks` payload.
- [ ] Integration test `tools_branch_protection_asserts_required_live_tests`
      mocks `gh api repos/.../branches/main/protection` JSON and asserts
      the script exits 0 on a payload containing `live-tests`, non-zero
      otherwise.

### Theme B — `check-dogfood-script` FAILs by default on iter-* branches [0/4] [pre_fix_repro_test: live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch]

- [ ] Detect current branch via `git rev-parse --abbrev-ref HEAD`. If it
      matches `iter-*` (or the `BRANCH_NAME` env var set by CI does),
      treat the dogfood gate as required.
- [ ] When required AND `FF_RDP_LIVE_TESTS` is unset, exit non-zero with
      a clear diagnostic: `check-dogfood-script: FAIL — iter-* branch
      requires FF_RDP_LIVE_TESTS=1 to verify dogfood script. Re-run with
      FF_RDP_LIVE_TESTS=1`.
- [ ] On non-`iter-*` branches, skip behavior preserved (exit 0 with the
      existing SKIP message). Test
      `live_check_dogfood_script_skips_on_main_without_ff_rdp_live_tests`
      asserts the main-branch skip path.
- [ ] CI: confirm `.github/workflows/live.yml` always sets
      `FF_RDP_LIVE_TESTS=1` on iter-* PRs so the new fail-by-default
      doesn't break green CI on branches that pass the live run.

### Theme C — `tools/lint-dogfood-script.sh` [0/6]

A shellcheck-plus-ff-rdp-specific linter for `.dogfood.sh` files. Catches
the exact author errors iter-86 shipped.

- [ ] Rule 1 (`unanchored-grep`): flags `grep -qi '<token>'` where
      `<token>` appears in a configured deny-list of common false-positive
      tokens (initial list: `headless`, `error`, `warning`, `firefox`).
      Suggests anchored or word-boundary variants. Unit test
      `unit_lint_dogfood_script_flags_unanchored_grep` covers the iter-86
      Theme B case verbatim.
- [ ] Rule 2 (`bool-flag-positional`): flags `ff-rdp <subcmd> --<bool-flag>
      '<value>'` where `<bool-flag>` is in a known-boolean list
      (`--jq-strict`, `--headless`, `--replace`, `--force`, `--debug-raw`,
      `--debug-trace`). Unit test
      `unit_lint_dogfood_script_flags_boolean_flag_with_positional` covers
      the iter-86 Theme D case verbatim.
- [ ] Rule 3 (`missing-set-euo-pipefail`): file must start (after
      shebang/comments) with `set -euo pipefail`. Unit test
      `unit_lint_dogfood_script_requires_set_euo_pipefail`.
- [ ] Rule 4 (`missing-sentinel-pattern`): script must contain
      `SENTINEL=/tmp/ff-rdp-iter-<N>-dogfood-ok`, `rm -f "$SENTINEL"` near
      the top, and a final `date -u … > "$SENTINEL"` line. Unit test
      `unit_lint_dogfood_script_requires_sentinel_pattern`.
- [ ] Rule 5 (`shellcheck-clean`): runs `shellcheck` if available and
      surfaces any SC2086/SC2046/SC2155 errors as ff-rdp lint failures.
      Skips with a warning if `shellcheck` is not installed.
- [ ] Wired into `check-iteration-ready` as a sub-check before
      `check-dogfood-script`. Integration test
      `xtask_check_iteration_ready_calls_lint_dogfood_script` asserts the
      sub-check name appears in output.

### Theme D — `xtask check-pre-fix-repro` [0/4]

- [ ] Plan-schema extension: per-theme `pre_fix_repro_test: <slug>`
      annotation parsed by `iteration_plan.rs`. Backward-compatible
      (themes without the annotation are ignored).
- [ ] `check_pre_fix_repro.rs`: implements the four-step verification
      described in "Pre-fix repro convention" above. Uses `git stash
      --include-untracked` before checkout to preserve working-tree changes;
      restores stash unconditionally via a `Drop` guard.
- [ ] Integration test `xtask_check_pre_fix_repro_asserts_test_red_then_green`
      runs against a checked-in fixture repo (under
      `crates/xtask/tests/fixtures/pre_fix_repro/`) with a known
      red-on-main / green-on-branch test.
- [ ] Wired into `check-iteration-ready` between `check-dead-primitives`
      and `check-dogfood-script`. Skips silently when the plan has no
      `pre_fix_repro_test:` annotations.

### Theme E — fix iter-86's buggy dogfood-script assertions [0/3] [pre_fix_repro_test: lint_flags_iter86_assertions_before_fix]

- [ ] Theme B (lcp_note): replace `grep -qi 'headless'` with anchored
      form, e.g. `grep -qiE '(^|[^a-z])headless Firefox' || …` — the
      intent is "the note must NOT claim headless Firefox when launched
      non-headless". Re-test against the actual session-58 note text:
      `"…regardless of headless mode…"` must NOT trip the check.
- [ ] Theme D (`--jq-strict`): invoke as `ff-rdp perf audit --jq-strict
      --jq '.results.does_not_exist_xyz'` (the boolean flag accompanies
      a real `--jq` expr). The expected stderr substring `not found`
      stays.
- [ ] Re-run `tools/lint-dogfood-script.sh
      kb/iterations/iteration-86-*.dogfood.sh` post-fix; must exit 0. Pre-fix
      run on `origin/main`'s copy of the script must exit non-zero (this is
      the `pre_fix_repro_test` for Theme E).

## Acceptance Criteria [0/10]

- [ ] tools_branch_protection_asserts_required_live_tests: script exits 0
      when `live-tests` is in `required_status_checks.contexts`; non-zero
      otherwise; mock payload fixture in `tools/tests/`.
- [ ] live_check_dogfood_script_fails_without_ff_rdp_live_tests_on_iter_branch:
      with HEAD on a fake `iter-99/test` branch and `FF_RDP_LIVE_TESTS`
      unset, `cargo run -p xtask -- check-dogfood-script <plan>` exits
      non-zero with the new diagnostic.
- [ ] live_check_dogfood_script_skips_on_main_without_ff_rdp_live_tests:
      with HEAD on `main` and `FF_RDP_LIVE_TESTS` unset, exits 0 with the
      existing SKIP message.
- [ ] unit_lint_dogfood_script_flags_unanchored_grep: fixture with the
      iter-86 Theme B grep returns lint error mentioning "anchored" or
      "false-positive".
- [ ] unit_lint_dogfood_script_flags_boolean_flag_with_positional: fixture
      with `--jq-strict 'path'` returns lint error mentioning the boolean
      flag and missing `--jq`.
- [ ] unit_lint_dogfood_script_requires_set_euo_pipefail: fixture without
      the line returns lint error.
- [ ] unit_lint_dogfood_script_requires_sentinel_pattern: fixture without
      the sentinel pattern returns lint error.
- [ ] xtask_check_pre_fix_repro_asserts_test_red_then_green: fixture-repo
      integration test, full red→fix→green round-trip.
- [ ] live_iter_86_dogfood_script_assertions_fixed: the iter-86 dogfood
      script (post-cleanup) lints clean AND executes cleanly when run
      against a live FF 151 that already has iter-86's CLI fixes.
- [ ] dogfood_script_full_run_iter_87: the sibling `.dogfood.sh` exits 0
      and writes `/tmp/ff-rdp-iter-87-dogfood-ok`. Exercises (1) linter
      against a known-bad fixture (must exit non-zero), (2) linter against
      iter-87's own script (must exit 0), (3) `check-dogfood-script` with
      `FF_RDP_LIVE_TESTS` unset on a fake iter-* branch context (must
      exit non-zero).

## Out of scope

- Migrating historical dogfood scripts (pre-iter-85) to the new linter.
- A general-purpose shell static analyzer; we're shipping ff-rdp-specific
  rules layered on shellcheck, not a replacement for it.
- Branch-protection enforcement for non-`iter-*` branches.
- Automatic remediation of lint findings; the linter reports, the author
  fixes.

## References

- [[dogfooding-session-58]] — the failure that made this iteration urgent
- [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]] — the
  gate this iteration hardens
- [[iteration-86-perf-field-report-fixes]] — source of the buggy
  assertions Theme C and Theme E address
