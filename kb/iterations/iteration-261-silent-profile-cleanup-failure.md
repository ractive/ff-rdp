---
title: "Iteration 261: a failed profile cleanup on the launch failure path is silent to every caller"
type: iteration
date: 2026-09-07
status: planned
branch: iter-261/silent-profile-cleanup-failure
depends_on:
  - 246
first_call_sites:
  - primitive: (to be decided — likely a warning on the `launch` error envelope)
    site: crates/ff-rdp-cli/src/commands/launch.rs
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

### A. Surface the skip [0/2]
- [ ] A failed removal on the launch failure path reaches the caller's JSON envelope
- [ ] The message names the directory and the `ProfileCleanupSkip` reason

### B. Exit code [0/1]
- [ ] State, in the Outcome, what the exit code is and why it did not change

### C. Sibling paths [0/1]
- [ ] Audit the other `ManagedProfileGuard` drop sites for the same silence

## Acceptance Criteria [0/3]

- [ ] A launch whose profile removal is made to fail prints a warning naming the surviving
      directory, in JSON output, with no `RUST_LOG` set
- [ ] The existing `unit_175_*` and `live_175_*` tests still pass unchanged
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

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
