---
branch: iter-152/live-guard-coverage-sweep
date: 2026-08-12
depends_on:
  - kb/iterations/iteration-151-residual-live-firefox-leak.md
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 live_1
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 --skip live_1
  ff-rdp profiles list --jq '.results.count'
  # → after BOTH chunks exit, `count` must be 0, and every profile that does survive must
  #   name its spawning test via the .ff-rdp-owner-test marker (never "unknown test").
first_call_sites: []
status: planned
title: "Iteration 152: close the remaining live-suite guard-coverage gaps"
type: iteration
tags:
  - iteration
---

# Iteration 152: close the remaining live-suite guard-coverage gaps

Carry-over from [[iteration-151-residual-live-firefox-leak]]. Filed from the independent
code review of PR #188 (2026-08-12), which found five real gaps that were out of scope for
151's own fix but are the same bug family.

## Why this exists

151's PR review turned up **two high-severity leaks that 151 itself missed** — the
`launch --replace` class, where the CLI reaps the prior Firefox and starts a *new* one whose
PID no guard owns. Those two were fixed in 151's PR because they *were* the residual leak.

The five items below are the remainder of that review: real, confirmed, but each either
lower severity or a broader refactor than 151's scope allowed. They are filed here rather
than fixed inline so 151 stayed honest to its own title.

**The meta-lesson worth carrying:** 151's original audit searched for *discarded guards*
(`ManuallyDrop`, `mem::forget`). It did not search for *processes nothing ever owned*, which
is why it missed the `--replace` class entirely. Any audit here must enumerate launch sites,
not guard sites.

## Themes

### Theme A — guard the remaining unowned launches

`live_90_daemon_lifecycle.rs:265` (`pre_fix_repro_daemon_state_sharing_red_then_green`) still
uses the unguarded `launch_on_port`, which returns a bare `u32`. It runs four assertions
before any cleanup, and the relaunch falls back to `new_pid = 0` on a parse miss, which
silently skips its `kill_pid`. This is the same "no RAII guard across an assertion" shape
151 Theme B claims to have eliminated suite-wide — in a file 151 edited.

- Give `launch_on_port` the `common::FirefoxGuard` return shape.
- Remove the `unwrap_or(0)` cleanup gate — a PID that failed to parse must fail the test,
  not silently skip the kill.

### Theme B — close the spawn→guard window

`live_142_disk_growth.rs`'s `launch_headless` still has an unguarded window: after
`out.status.success()` confirms Firefox is running, the `?` / `.ok()?` on JSON parse and
`results.pid` can return `None` and drop the launched process with nothing to reap it. The
same applies to `live_142_throttle_json_gc`, where two `.expect()`s sit between spawn and
`FirefoxGuard` construction.

- Extract the PID first and construct the guard before any other parsing or assertion.
- On the error path, `kill_pid` before returning `None`.

### Theme C — owner-marker coverage across raw launch sites

151 Theme A's `FF_RDP_LIVE_TEST_NAME` instrumentation only covers `common::LiveFirefox`.
Four live files carry private `LiveFirefox` clones (`live_oneway.rs`, `live_target_destroyed.rs`,
`live_cross_actor.rs`, `live_61l.rs`) and several sites launch raw
(`live_90_daemon_lifecycle.rs`, `live_110_kill_scoping.rs`, `live_86_perf_field_fixes.rs`,
`live_123_daemon_autostart_and_registry.rs`). None set the env var — so a profile leaked by
any of them still reports "unknown test", which is precisely the traceability 151 was
supposed to deliver.

- Make `current_test_name()` public and add a `common::launch_command()` helper that
  pre-sets `SPAWNING_TEST_ENV`.
- Route every `"launch"` invocation under `tests/live/` through it.

### Theme D — guard Drop must not signal a known-dead PID

`live_90_daemon_lifecycle.rs:169`: keeping a guard live after `daemon stop` / `--replace` has
already reaped the PID means `Drop` unconditionally signals a PID the test knows is dead.
`kill_pid` does no liveness or ownership check, which reintroduces at test scope the
recycled-PID hazard iter-110 guarded against in production. Low probability — but 151
removed the `ManuallyDrop` that was incidentally preventing it.

- Have `kill_pid` (or the guard's `Drop`) skip when `!pid_alive(pid)`, or add an explicit
  `disarm()` for paths that have already asserted the process is gone.

### Theme E — de-duplicate the owner-marker helpers

`live_151_residual_leak.rs`'s `live_owned_profile_dirs` is copy-pasted from
`live_96_profile_cleanup.rs`, and the `.ff-rdp-owner-test` literal now appears in four places.
The justification recorded in 151 ("no `[lib]` target for an integration-test binary to import
from") is wrong: both files are modules of the *same* `tests/live` binary, and
`tests/common/mod.rs` exists for exactly this — it already hosts `kill_pid` / `pid_alive` /
`FirefoxGuard`.

- Move the helper and the marker-name constant into `common/mod.rs`.
- The `SPAWNING_TEST_ENV` duplication between `src/` and `tests/` is genuinely unavoidable
  (the product-side constant is private); leave that one and keep its explanatory comment.

## Acceptance Criteria [0/5]

- [ ] live_152_no_unowned_launch_sites: a test (or xtask check) enumerates every `"launch"`
      invocation under `crates/ff-rdp-cli/tests/live/` and asserts each one's PID is bound to
      an RAII guard before the next assertion — enumerating launch sites, not guard sites
- [ ] live_152_marker_names_test_from_raw_launch: a profile leaked by a raw-`Command` launch
      site (not `common::LiveFirefox`) still names its spawning test in `.ff-rdp-owner-test`
- [ ] live_152_guard_drop_skips_dead_pid: a guard whose PID was already reaped does not
      signal it on drop, proven by observing no kill against a recycled/dead PID
- [ ] live_152_chunk_a_leaves_no_orphans: a full chunk-A run leaves zero ff-rdp-spawned
      Firefox processes — the AC 151 could not tick because it never ran the chunk
- [ ] live_152_chunk_b_leaves_no_orphans: the complementary chunk-B run leaves zero
      ff-rdp-spawned Firefox processes, and `live_96_profile_cleanup`'s precondition passes
      without manual cleanup

## Notes

- Do not tick the two chunk ACs without actually running the chunks. 151 ticked its
  equivalents on "implemented and compiled" and they were unticked in review; the chunk runs
  are ~6 minutes each and are the only evidence that counts. Note that
  `check-iteration-ready`'s `ac-fidelity-check` will NOT catch this: it verifies a ticked AC
  *references* a test slug, not that the test ran.
- Environment quirks (long commands killed at ~9–10 min, the two-chunk split, the
  `pgrep -f "firefox.*ff-rdp-profile"` self-match over-report) are documented in
  [[iteration-151-residual-live-firefox-leak]]'s Run guidance section — read it first.
- Verify on the wire before fixing. Across 135–151 the stated root cause diverged from
  reality at least eight times, most recently in 151 itself.
</content>
