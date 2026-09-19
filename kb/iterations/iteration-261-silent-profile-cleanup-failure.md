---
title: "Iteration 261: a failed profile cleanup on the launch failure path is silent to every caller"
type: iteration
date: 2026-09-07
status: done
branch: iter-261/silent-profile-cleanup-failure
depends_on:
  - 246
first_call_sites:
  - primitive: AppError::with_warning and ManagedProfileGuard::cleanup
    site: crates/ff-rdp-cli/src/commands/launch.rs::report_failed_profile_cleanup
dogfood_path: |
  # Reproduce the silence, not the race: make the removal fail on purpose.
  # (A directory the process cannot remove is the cheapest stand-in for the
  # load-triggered ENOTEMPTY that iteration 211's sweep hit.)
  firefox -no-remote --start-debugger-server 6000 --headless
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_175_failed_launch_leaves_no_profile_dir -- --include-ignored --test-threads=1
  # expected TODAY: green, and `ff-rdp launch --launch-timeout 0 …` prints only
  #   {"error":"Firefox (pid N) did not open debug port P within 0s — …"}
  # even when ManagedProfileGuard's Drop logged `could not remove …`.
  # expected AFTER: the envelope also carries a warning naming the directory
  # that survived, so a caller and a sweep can both see it without RUST_LOG.
tags: [iteration, launch, profile-cleanup, honesty, carry-over, error-envelope]
---

# Iteration 261: a failed profile cleanup on the launch failure path is silent to every caller

> Filed by [[iteration-246-sweep-load-misclassification]] Part A Theme D, which was told to
> "fix the ones that are waits; file the ones that are product bugs as their own plans".
> Iteration 246 fixed the *ordering* half (reaping the killed child before the guard's `Drop`
> walks its directory); this plan is the *reporting* half it deliberately did not widen into.

## Why

`ManagedProfileGuard::drop` (`crates/ff-rdp-cli/src/util/profile_dir.rs`) is the single place a
failed launch's profile directory is removed. When the removal does not happen it takes the
`ProfileCleanup::Skipped(reason)` arm and reports it with `tracing::warn!`.

`ff-rdp` is a JSON-on-stdout tool. A `tracing::warn!` is not in the envelope, so:

- the caller sees only `{"error":"Firefox (pid N) did not open debug port P within Ns — …"}`,
  with no hint that a directory was also left behind;
- `live-sweep` sees nothing either — its `LIVE_SWEEP_PROFILES leaked=…` line (iteration 245)
  counts survivors at the end of a tier, so it attributes the leak to the *tier*, not to the
  command that produced it;
- and the only test that can observe it, `live_175_failed_launch_leaves_no_profile_dir`, has to
  reconstruct the fact by diffing the profile root.

That is the same class iteration 179 spent a whole iteration on: the tool knows exactly what went
wrong and does not say so where anyone reads.

## What is already known

Iteration 211's second closing sweep failed `live_175_failed_launch_leaves_no_profile_dir` with

```
a launch that failed waiting for the debug port left 1 profile directory behind:
["ff-rdp-profile-E0IitROGtQDhVDZu"]
```

and passed in the sweep immediately before it, on a tree differing only in three test fixes.
Iteration 246 attributed the *most likely* mechanism from the code — `child.kill()` only sends the
signal, so `remove_dir_all` could walk a directory a still-living Firefox was writing into — and
fixed that ordering. What it could not establish, because the condition is load, is whether that
was the *only* mechanism. The reporting gap is what makes the next occurrence answerable.

## Themes

- **A — Surface the skip.** A `ProfileCleanup::Skipped` on the failure path belongs in the
  `launch` error envelope as a warning naming the directory and the reason, alongside the
  deadline error it is already returning.
- **B — Decide what a `Skipped` means for the exit code.** It must not turn a `User` error into
  something else; the launch already failed. This is about evidence, not severity.
- **C — Check the other `Drop` sites.** `ManagedProfileGuard` is dropped from every early return
  in `launch::run`, not only the port-deadline one.

## Tasks

### A. Surface the skip [2/2]
- [x] A failed removal on the launch failure path reaches the caller's JSON envelope
- [x] The message names the directory and the `ProfileCleanupSkip` reason

### B. Exit code [1/1]
- [x] State, in the Outcome, what the exit code is and why it did not change

### C. Sibling paths [1/1]
- [x] Audit the other `ManagedProfileGuard` drop sites for the same silence

## Acceptance Criteria [3/3]

- [x] A launch whose profile removal is made to fail prints a warning naming the surviving
      directory, in JSON output, with no `RUST_LOG` set
- [x] The existing `unit_175_*` and `live_175_*` tests still pass unchanged
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

## Out of scope

- The load-triggered race itself — iteration 246 reaped the child before the guard's `Drop`, and
  whether that was sufficient is a question this plan's reporting makes answerable rather than one
  it re-litigates.
- `live-sweep`'s `LIVE_SWEEP_PROFILES` accounting (iteration 245). It counts survivors correctly;
  the gap is that the command which produced one never said so.

## References

- [[iteration-246-sweep-load-misclassification]] — where this was found and triaged
- [[iteration-175-failed-launch-leaks-unmarked-profile-dir]] — the guard this plan reports on
- `crates/ff-rdp-cli/src/util/profile_dir.rs` — `ManagedProfileGuard::drop`
- `crates/ff-rdp-cli/src/commands/launch.rs` — the port-deadline failure path

## Implementation preflight — 2026-09-19

The source audit confirms a reporting gap: ManagedProfileGuard traces skipped cleanup without returning details into the launch error envelope. Include both the build_command guard and the outer launch guard. Existing child kill/wait ordering from 246 is already present; preserve it. Decide explicitly how secure-root resolution failure that disarms a guard is reported. Preserve User/error exit semantics and one JSON envelope; do not emit an additional document from Drop. Focus regressions on actual cleanup/error paths rather than an exhaustive hypothetical exception matrix.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Outcome — 2026-09-19

`ManagedProfileGuard::cleanup` now exposes a skipped cleanup to its owner before `Drop` runs.
Both owners use that result: `build_command` decorates failures after its managed directory is
created, and `launch::run_with_hooks` decorates spawn, immediate-exit, debug-port, and process-status
failures. The existing primary error remains authoritative and the single JSON document gains a
`warnings` array naming the surviving directory and stable `profile_cleanup_skip_reason`.

The warning wrapper delegates its error type and exit code to the original error. A failed launch
therefore remains a `User` error with exit code 1; failed cleanup adds evidence, not severity. If
the secure profile root cannot be resolved, the guard remains fail-closed, removes nothing, and
reports `no-profile-root` through the same warning path. `Drop` remains the fallback for callers
that do not explicitly collect cleanup, and emits no JSON of its own.

The regression replaces the freshly created managed directory with a file before returning a
simulated spawn error. That makes the real `remove_dir_all` operation fail deterministically and
proves, without `RUST_LOG`, that the original error JSON names the path and `remove-failed` reason.
The unchanged `unit_175_*` tests passed, and the closing dual-gate sweep passed both unchanged
`live_175_*` tests.

Validation used stable Rust 1.98.1. All nine current xtask `check-*` gates passed; the dogfood gate
reported its documented skip because this plan has no `dogfood_script`. The ordered `cargo fmt`,
strict workspace clippy, and workspace tests passed.

## Carry-over

- **No plan, setup corrected:** the first owned raw port-6000 browser was observed listening,
  then was absent before startup classification. Its exit cause is unknown. Keeping the
  replacement browser in an owned execution session restored all nine core tests; both
  attempts and cleanup observations remain in `iter261/gates-record.md`. The first summary
  was `executed=334 skipped=0 preexisting=9 vanished=0 launch_timeout=0 timed_out=1 total=344`,
  with zero profile leaks. A browser that disappears while its owning session is retained
  would require a separate investigation; no product cause is claimed here.
- **File:** the initial `live_styles_applied::live_styles_applied_returns_real_rules` failure
  fired the existing203 frontmatter trigger and is now owned by
  [[iteration-274-styles-applied-unattributed-recurrence]]. Only `FAILED` survived before
  watchdog termination, so its cause and required panic/URL/document/DOM/route diagnostics
  remain unknown. The corrected pass does not close it. Plan274 is outside this batch,
  and203 remains parked; the trigger is not reset to wait for another occurrence.
- **File:** the initial `live_158_launch_lifecycle::live_158_launch_survives_contended_bind`
  watchdog hang is owned by [[iteration-273-contended-launch-output-hang]]. Retained stacks
  show a worker blocked reading subprocess output and the test joining it; they do not
  identify the responsible pipe owner. The runner reaped four managed Firefox processes.
  Both snapshots are preserved under `.git/ralph-loop/20260919-queue/iter261/watchdog/`.
  The corrected pass remains a control, not evidence of repair. Plan 273 is not selected
  for this batch.
- The corrected full sweep reconciled all tiers and names:
  `executed=344 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=344`, with
  zero leaked or unattributed profiles. It had one ordinary failure,
  `live_137_consent_accept_via_daemon`, with `target_count=1`, `live_target_count=0`, a healthy
  dispatcher, and no RPC owner after the 15-second bound. This is the already selected
  [[iteration-262-daemon-live-target-never-promoted]] failure family; it is preserved there and was
  not rerun to chase green.
