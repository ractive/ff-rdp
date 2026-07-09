---
title: "Iteration 112: windows-latest CI hangs launching live Firefox in tests/live/live_61l.rs"
type: iteration
date: 2026-07-10
status: planned
branch: iter-112/windows-live-firefox-launch-hang
depends_on:
  - kb/iterations/iteration-108-windows-ci-preexisting-reds.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-99-ci-preexisting-reds.md
  - kb/iterations/iteration-108-windows-ci-preexisting-reds.md
first_call_sites: []
dogfood_path: |
  # After the fix, the windows-latest `test` job must finish `cargo test --workspace`
  # within its 10-minute step timeout, with live_61l tests either skipping cleanly
  # (Firefox unavailable) or completing (Firefox available) — never hanging:
  cargo test -p ff-rdp-cli --test live live_61l::live_eval_basic -- --nocapture
  # expected on Windows: returns within ~30s (skip or pass), never blocks past
  # LiveFirefox::launch()'s own 30s TCP + 10s tab-poll deadlines.
tags:
  - iteration
  - ci
  - windows
  - live-tests
  - preexisting-red
---

# Iteration 112: windows-latest live-Firefox launch hang

Discovered during the iter-108 (`windows-ci-preexisting-reds`, PR #147) merge
review, immediately after iter-108's own fixes landed and pushed the
windows-latest `test` job further than it had ever gotten before. Once the
prior 5 pre-existing failures (Theme A env-var bug, Theme B send-teardown
race) and a second latent mock-server race found during review were all
fixed, the `test (windows-latest)` job progressed past `crates/ff-rdp-cli`'s
unit + e2e suites into `tests/live/main.rs`, and then hung: `live_61l::live_eval_basic`,
`live_61l::live_eval_csp`, `live_61l::live_locale_pin_launch_sets_lang_env`,
and `live_61l::live_navigate_invalidates_console_actor` were all still
"running for over 60 seconds" when the job's 10-minute `Run tests` step
timeout fired and killed it (CI run 29052935360, job 86237770363, 2026-07-09).

This is the same category of bug as iter-99/iter-108: a pre-existing red that
was **masked** by an earlier, unrelated Windows failure aborting `cargo test`
before it ever reached this code path. It is not caused by iter-108's diff
(`install_skill.rs`, `nav_action.rs`, `tests/e2e/support/mock_server.rs`) —
none of those files are on the `live_61l` code path
(`crates/ff-rdp-cli/tests/live/live_61l.rs`, `crates/ff-rdp-cli/src/commands/launch.rs`).

## Why this test hangs (working hypothesis — verify on a Windows CI runner)

`live_61l` tests that are **not** gated by `FF_RDP_LIVE_NETWORK_TESTS` (e.g.
`live_eval_basic`) call `LiveFirefox::launch()` unconditionally. That in turn
calls `Command::new(ff_rdp_bin()).args(["launch", "--headless", ...]).output()`
— a **blocking** call that waits for the child process's stdout/stderr pipes
to close. `LiveFirefox::launch()`'s own internal deadlines (`wait_for_tcp`
30s, tab-poll 10s) only start *after* `cmd.output()` returns, so they cannot
bound a hang inside `ff-rdp launch` itself.

Two other windows-latest `test` jobs completed the *earlier* e2e/unit suites
within the timeout, and `live_navigate_cross_origin_url_match` /
`live_navigate_dnsfail` (both gated or fast-failing) returned near-instantly
in the same run — only the tests that actually try to spawn and wait on
Firefox hang. Leading theory: GitHub's `windows-latest` runner image ships a
real Firefox binary (unlike `ubuntu-latest`/`macos-latest`, where `ff-rdp
launch` fails fast with "Firefox not found" and the test skips cleanly), so
`find_firefox()` succeeds and `ff-rdp launch --headless` actually attempts a
real launch — and something in that path never returns on Windows. Classic
candidates:
1. A grandchild process (Firefox itself, or an updater/crash-reporter helper)
   inherits the launcher's stdout/stderr handles and keeps them open, so
   `Command::output()` blocks forever waiting for EOF on a pipe nothing will
   ever close (well-documented Windows `CreateProcess` handle-inheritance
   footgun — see `std::process::Stdio` docs).
2. `ff-rdp launch` itself blocks on something Windows-specific (e.g. waiting
   for a debugger-server confirmation that never arrives in this sandboxed
   environment).

## Tasks [0/2]

- [ ] Reproduce on a Windows runner/VM (or by inspecting `ff-rdp launch`'s
      process-spawn code in `crates/ff-rdp-cli/src/commands/launch.rs` for
      `Stdio::inherit()`/handle-inheritance issues) and confirm which of the
      two hypotheses (or another) is the actual cause.
- [ ] Fix at the layer the root cause lives:
      - If it's handle inheritance: ensure the Firefox child spawn uses
        `Stdio::null()`/`Stdio::piped()` consistently so no inherited handle
        can keep the launcher's own pipes open past process exit, and/or add
        an explicit bounded timeout around `LiveFirefox::launch()`'s
        `cmd.output()` call in the test harness itself (e.g. spawn +
        `wait_timeout`-style polling) as defense in depth for any future
        instance of this class of hang.

## Acceptance Criteria [0/2]

- [ ] live_eval_basic (or the specific reproducer test identified in task 1):
      completes (pass or clean skip) within 60s on windows-latest CI — verify
      via a CI run's job log showing no "has been running for over 60
      seconds" message and no step timeout.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
      cargo test --workspace -q` clean — run in the pre-PR quality gates.

## Out of scope

- Any other `live_61l`/`live_*` test content correctness — this iteration is
  scoped to the **hang**, not to auditing live-test assertions.
- Re-running the full live suite here; iter-110 (`post-batch-live-sweep`)
  covers that separately once this hang no longer eats the windows-latest
  `test` job's time budget.

## References

- [[iteration-99-ci-preexisting-reds]] — same category: pre-existing red
  masked by an earlier failure that aborted the test run first.
- [[iteration-108-windows-ci-preexisting-reds]] — the PR (#147) whose fixes
  first let `test (windows-latest)` progress far enough to expose this hang.
- CI run 29052935360, job 86237770363 (`test (windows-latest)`, iter-108,
  2026-07-09) — the run where this was discovered; step timed out at 10 min.

## Hotfix note (2026-07-10)

The immediate CI hang was hotfixed ahead of this iteration: PR `iter-112/gate-ungated-live61l-tests` adds the missing `#[ignore]` gates to 8 live_61l tests that launched Firefox unconditionally (hanging 10-min-timeout on the Firefox-less windows runner). Remaining scope for this iteration: root-cause why an ungated launch hangs forever instead of failing fast (launch needs a timeout), and extend `check-live-test-layout` so every `#[test]` under `tests/live/` must carry `#[ignore]` — making this class of miss impossible.
