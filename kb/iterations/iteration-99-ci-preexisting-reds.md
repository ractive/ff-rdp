---
title: "Iteration 99: cookies --help stack overflow (Windows STATUS_STACK_OVERFLOW)"
type: iteration
date: 2026-07-08
status: planned
branch: iter-99/cookies-help-stack-overflow
depends_on: []
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-98-media-query-truthfulness.md
  - kb/iterations/iteration-100-daemon-lifecycle-hardening.md
first_call_sites: []
dogfood_path: |
  # After the fix, cookies --help renders inside a 1 MiB-stack thread on every
  # platform (mirroring the Windows main-thread limit that currently overflows):
  cargo test -p ff-rdp-cli --test cli_cookies_help
  # exit 0 on windows-latest CI (was 0xC00000FD STATUS_STACK_OVERFLOW)
tags: [iteration, ci, windows, cookies, stack-overflow]
---

# Iteration 99 — `cookies --help` stack overflow

## Scope note (2026-07-09)

Originally this plan bundled two pre-existing CI reds. After the 2026-07
deep review, **Theme B (daemon auto-start registration) moved to
[[iteration-100-daemon-lifecycle-hardening]]** — it is the same failure
surface as iter-100's spawn race and silent direct-fallback, and belongs with
the daemon lifecycle work. iter-99 now owns **only** the Windows
`cookies --help` stack overflow.

For the record on the moved theme: the `live_eval_object_leak_soak` red was
only *deflaked* in commit `23c5994` (status poll widened 500 ms → 10 s); the
Live Tests lane is green but the root cause — auto-start sometimes never
registers while the CLI silently goes direct — is unfixed, and the
`daemon_autostart_failed` warning was never added. iter-100 Theme E owns both.

## The remaining bug

**Windows: `cookies_help_no_fields_paragraph_leak`** — `ff-rdp cookies
--help` exits `3221225725` (`0xC00000FD`, STATUS_STACK_OVERFLOW) with
"thread 'main' has overflowed its stack". Still red on the CI workflow as of
run 28977385134 (2026-07-08, `test (windows-latest)`); the cookies command
path has not changed since iter-84. Windows' 1 MB main-thread stack (vs 8 MB
Linux/macOS) is why only Windows blows — the overflow itself (deep recursion
or an oversized stack frame somewhere in the `cookies --help` render path) is
latent on all platforms, which is why macOS/Linux CI stay green and mask it.

## Themes

### A. `cookies --help` must not overflow the stack

Find the recursion / oversized stack frame in the `cookies --help` path
(clap command construction or help rendering), fix it structurally (no
"raise the stack limit" workarounds), and pin it with a test that walks
`--help` for every subcommand inside a deliberately small stack so no other
command hides the same latent bug.

## Pre-fix repro

- `pre_fix_repro_cookies_help_stack_depth` — a unit test that runs
  `cookies --help` rendering in a deliberately small-stack thread
  (`std::thread::Builder::stack_size(1 << 20)`, mirroring the Windows
  main-thread limit) and asserts it completes; pre-fix it overflows on all
  platforms, making the Windows-only CI failure reproducible everywhere.

## Tasks

### Theme A — cookies --help stack overflow [0/3]

- [ ] Land `pre_fix_repro_cookies_help_stack_depth` (small-stack thread
      harness; fails pre-fix on every platform).
- [ ] Root-cause and fix the overflow in the `cookies --help` path.
- [ ] `unit_all_subcommand_helps_render_in_small_stack`: iterate every
      subcommand's `--help` in the same small-stack harness.

## Acceptance Criteria [0/3]

- [ ] `pre_fix_repro_cookies_help_stack_depth`: post-fix, `cookies --help`
      renders inside a 1 MiB-stack thread without overflow.
- [ ] `unit_all_subcommand_helps_render_in_small_stack`: every subcommand's
      `--help` renders inside the same harness.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean (and the existing
      `cookies_help_no_fields_paragraph_leak` passes on windows-latest CI).

## Out of scope

- Daemon auto-start registration — moved to
  [[iteration-100-daemon-lifecycle-hardening]] Theme E.
- The navigate budget / Windows compile fixes — landed in PR #135.
- The `redact_*` global-state test race (separate small follow-up; see the
  transport-limits note in [[deep-review-2026-07-fable5]]).

## References

- CI run 28977385134 (`test (windows-latest)`: cookies help overflow,
  `0xC00000FD`), 2026-07-08.
- `crates/ff-rdp-cli/tests/cli_cookies_help.rs` — the existing test that
  overflows on Windows.
- [[iteration-100-daemon-lifecycle-hardening]] — where the daemon soak theme
  went.
