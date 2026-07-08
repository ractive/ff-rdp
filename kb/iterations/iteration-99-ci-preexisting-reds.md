---
title: "Iteration 99: clear the two pre-existing CI reds — cookies --help stack overflow (Windows), daemon auto-start registration failure (live soak)"
type: iteration
date: 2026-07-08
status: planned
branch: iter-99/ci-preexisting-reds
depends_on: []
firefox_refs: []
kb_refs: [kb/iterations/iteration-98-media-query-truthfulness.md]
first_call_sites: []
dogfood_path: |
  # After the fix, both formerly-red CI lanes run green end-to-end:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test eval_object_leak_soak -- --include-ignored
  # exit 0; daemon status reports a pid within the poll window
tags: [iteration, ci, windows, daemon, cookies, stack-overflow]
---

# Iteration 99 — clear the two pre-existing CI reds

## Verified state (2026-07-09) — STILL OPEN, 0/4 ACs

Re-audited against code + live CI after the 2026-07 deep review. The plan is
**not done**; one symptom is masked, both root causes remain:

- **Theme B — Live Tests lane is now GREEN, but only deflaked, not fixed.**
  Commit `23c5994` changed *only the test's polling logic* (fixed 500 ms
  sleep → 10 s status-poll loop, `tests/eval_object_leak_soak.rs:124-153`).
  The daemon auto-start registration delay was never root-caused, and the
  `daemon_autostart_failed` warning the plan asks for does not exist anywhere
  (grep: zero hits). The soak test passing is why this iteration *looks*
  fixed — it is not. **Root cause is almost certainly in the daemon
  lifecycle findings from [[deep-review-2026-07-fable5]]:** the auto-spawn
  check→spawn→register sequence is not serialized (TOCTOU, iter-100 D1), and
  auto-start failure silently falls back to a direct connection with no
  signal. Theme B should be **absorbed into
  [[iteration-100-daemon-lifecycle-hardening]]** (spawn lock + surfaced
  failure) rather than solved standalone here.
- **Theme A — CI lane is STILL RED on Windows.** Latest CI run 28977385134:
  `cookies_help_no_fields_paragraph_leak` exits `3221225725` (0xC00000FD,
  STATUS_STACK_OVERFLOW) on `windows-latest`. Untouched — the cookies command
  path has not changed since iter-84. This is the real remaining work in this
  iteration. macOS/Linux pass because of their 8 MB main-thread stack; the
  1 MB Windows stack is the only trigger. **Keep this as iter-99's sole
  focus** once Theme B moves to iter-100.

Recommendation: re-scope iter-99 to Theme A only (cookies stack overflow),
move Theme B to iter-100, and re-tick nothing until the cookies fix lands
with the small-stack repro test.

---

PR #135 (fix/ci-navigate-budget-windows-libc) fixed the navigate events-budget
collapse (Ubuntu green for the first time since 2026-05-27) and the Windows
`libc::pid_t` compile break (Windows unit suite: 610 passed). Two failures
remain on its CI, and **both are pre-existing main bugs**, not products of
that branch:

1. **Windows: `cookies_help_no_fields_paragraph_leak`** — `ff-rdp cookies
   --help` exits `0xC00000FD` (STATUS_STACK_OVERFLOW) with "thread 'main' has
   overflowed its stack". Failing on Windows CI since ~2026-05-27 (predates
   the June compile break that masked it). Windows' 1 MB main-thread stack
   (vs 8 MB Linux) is why only Windows blows; the overflow itself (deep
   recursion or oversized stack allocation somewhere in the `cookies --help`
   render path) is latent on all platforms.
2. **live-tests: `live_eval_object_leak_soak`** — after the auto-start `tabs`
   call succeeds, `daemon status` reports `{"running": false, "pid": null}`
   indefinitely (10 s poll exhausted). Reproduced 2026-07-08 on ubuntu-latest
   CI **and locally on macOS against clean main** (fails in 2.55 s with
   main's pre-poll code) — yet the same workflow passed on 2026-07-03. The
   daemon fails to spawn/register while the CLI silently falls back to a
   direct connection; suspects: Firefox version bump (152 locally; runner
   image update), a stale/conflicting per-user daemon registry entry, or an
   auto-start regression that only manifests with current Firefox. A re-run
   of the 2026-07-03 green run on today's runners was triggered to separate
   environment from code (result to be recorded here).

## Themes

### A. `cookies --help` must not overflow the stack

Find the recursion / oversized stack frame in the `cookies --help` path
(clap command construction or help rendering), fix it structurally (no
"raise the stack limit" workarounds), and pin it with a unit test that walks
`--help` for every subcommand so no other command hides the same latent bug.

### B. Daemon auto-start must register (or fail loudly)

Diagnose why the auto-started daemon never appears in `daemon status`.
Fix the root cause AND the silent degradation: when auto-start fails, the
CLI currently proceeds in direct mode with no signal — surface a warning in
the envelope so tests and users can tell the difference.

## Pre-fix repro

- `pre_fix_repro_cookies_help_stack_depth` — a unit test that runs
  `cookies --help` rendering in a deliberately small-stack thread
  (`std::thread::Builder::stack_size(1 << 20)`, mirroring the Windows
  main-thread limit) and asserts it completes; pre-fix it overflows on all
  platforms, making the Windows-only CI failure reproducible everywhere.
- `live_eval_object_leak_soak` (existing) — currently red on main; post-fix
  green. Its 10 s status-poll hardening landed in PR #135.

## Tasks

### Theme A — cookies --help stack overflow [0/3]

- [ ] Land `pre_fix_repro_cookies_help_stack_depth` (small-stack thread
      harness; fails pre-fix on every platform).
- [ ] Root-cause and fix the overflow in the `cookies --help` path.
- [ ] `unit_all_subcommand_helps_render_in_small_stack`: iterate every
      subcommand's `--help` in the same small-stack harness.

### Theme B — daemon auto-start registration [0/3]

- [ ] Record the 2026-07-03-rerun experiment outcome here (environment vs
      code) and root-cause why the daemon fails to register.
- [ ] Fix the root cause; `live_eval_object_leak_soak` green locally and
      in the live-tests workflow.
- [ ] `unit_autostart_failure_surfaces_warning`: when the daemon cannot be
      started, the command envelope carries a `daemon_autostart_failed`
      warning instead of silently going direct.

## Acceptance Criteria [0/4]

- [ ] `pre_fix_repro_cookies_help_stack_depth`: post-fix, `cookies --help`
      renders inside a 1 MiB-stack thread without overflow.
- [ ] `unit_all_subcommand_helps_render_in_small_stack`: every subcommand's
      `--help` renders inside the same harness.
- [ ] `live_eval_object_leak_soak`: green on live-tests CI and locally
      (daemon pid reported within the poll window).
- [ ] `unit_autostart_failure_surfaces_warning`: forced auto-start failure
      yields a `daemon_autostart_failed` warning in the envelope.

## Out of scope

- The navigate budget / Windows compile fixes themselves — landed in PR #135.
- The `redact_*` global-state test race (separate small follow-up).

## References

- PR #135 CI runs 28976703298 (Windows: cookies help overflow) and
  28976703234 (live-tests: soak daemon registration), 2026-07-08.
- [[iteration-98-media-query-truthfulness]] — queued sibling plan; iter-97/98
  ordering unaffected by this plan.
