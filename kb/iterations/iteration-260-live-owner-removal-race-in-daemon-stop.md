---
title: "Iteration 260: daemon stop's profile removal races the browser it just stopped, and the live verifications iteration 242 could not run"
type: iteration
date: 2026-09-07
status: planned
branch: iter-260/live-owner-removal-race-in-daemon-stop
depends_on:
  - iteration-242-profile-liveness-flake-in-prune-all
first_call_sites: []
dogfood_path: |
  # Part A — is `daemon stop`'s `profile_removed: false` the same ENOTEMPTY
  # race iteration 242 measured in `profiles prune --all`?
  #
  # iteration 242 proved that `--all` against a live owner fails
  # `remove_dir_all` with "Directory not empty (os error 66)" roughly 4 runs in
  # 10, because the browser keeps writing into the directory during the walk.
  # `daemon stop` calls `cleanup_profile_dir` -> `remove_dir_all` on the same
  # kind of directory, moments after reporting the process gone. Same syscall,
  # same directory, a narrower but non-empty window: on macOS a content process
  # can outlive the parent's exit by tens of milliseconds.
  #
  # `profile_skip_reason` (iteration 242) is what makes this answerable at all —
  # before it, `profile_removed: false` carried nothing. Run the stop in a loop
  # and count how often it says `remove-failed`:
  export FF_RDP_HOME="$(mktemp -d)"
  cargo build --quiet -p ff-rdp-cli --bin ff-rdp
  B=./target/debug/ff-rdp
  for i in $(seq 1 20); do
    P=$((39300+i))
    "$B" launch --headless --debug-port $P >/dev/null || continue
    "$B" --port $P daemon stop | jq -c '.results | {profile_removed, profile_skip_reason}'
  done
  rm -rf "$FF_RDP_HOME"
  # → a `remove-failed` here confirms the shared mechanism. Anything else
  #   (`outside-profile-root`, `not-managed-basename`, `no-profile-root`) is a
  #   DIFFERENT defect and this plan's premise is wrong — say so.

  # Part B — the live verifications iteration 242 wrote but could not run.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_242_marker_names_test_from_direct_launch -- --include-ignored
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -p ff-rdp-cli --test live live_eval_on_hn -- --include-ignored
  # → and a dual-gate `cargo run -p xtask -- live-sweep`, which is the only
  #   evidence that closes iteration 242's remaining acceptance criteria.
tags: [iteration, profiles, daemon, race, live-tests, carry-over]
---

# Iteration 260: `daemon stop`'s profile removal races the browser it just stopped

Carry-over from [[iteration-242-profile-liveness-flake-in-prune-all]], filed before that
iteration's PR merged.

## Part A: is the `daemon stop` observation the same race?

Iteration 242 found what the `profiles prune --all` flake actually was: not a liveness flip, but
`remove_dir_all` failing with `ENOTEMPTY` because the owner Firefox writes into the profile
directory while the walk is in progress. Measured: 4 failures in 10 runs of the iteration-97
dogfood gate against a real headless Firefox, with `owner_liveness` reporting `"live"` on every
one of them.

Iteration 224's closing sweeps carried a second observation with the same "true once, false once,
one run apart" signature, which iteration 242 inherited and did not resolve:

```text
live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json
profile_removed must be true — got
  {"stopped":true,"pid":18379,"port":57624,"profile_removed":false,"profile_removed_path":null}
```

`stopped: true` means the escalation ladder reported the process gone and the port free, so
`cleanup_profile_dir` *was* called and returned nothing. It passed alone, `--test-threads=1`,
2.86 s, immediately after the sweep.

**The hypothesis this iteration must test, not assume:** it is the same `ENOTEMPTY` race.
`cleanup_profile_dir` calls the same `remove_dir_all` on the same kind of directory, and the
window — between the parent's exit being observed and every content process releasing the
profile — is narrower than `--all`'s but is not zero.

Iteration 242 made this answerable rather than answering it: `ProfileCleanup::Skipped` now
carries a `ProfileCleanupSkip`, and `daemon stop` reports it as `profile_skip_reason`. Three of
its four values are refusals that are correct by design for a `--profile` directory; only
`remove-failed` indicates a problem. **Get the value before proposing anything.** Iteration 242
spent its first hour on a hypothesis three iterations had inherited without measuring, and the
measurement took twenty minutes once the field existed.

### Themes

- **A — Measure.** Loop `launch` + `daemon stop` under an isolated `$FF_RDP_HOME` and record
  `profile_skip_reason`. If it never fires, say so: the iteration-224 observation may have been
  a sweep-load artefact, and a plan that cannot reproduce its subject should close as obsolete
  rather than fix something adjacent.
- **B — Decide what "removed" should mean when the owner is still letting go.** Three shapes,
  and iteration 242 deliberately rejected the first for `prune --all`:
  1. a bounded retry — rejected there because "how many retries" has no principled answer
     against an owner that keeps writing. It may be defensible *here*, where the owner is known
     to be exiting rather than running, and the window is bounded by process teardown;
  2. wait for the PID to be reaped before attempting removal at all;
  3. leave it, and rely on the next `launch`'s orphan sweep — which the iter-142 dead-owner rule
     already reclaims immediately, so the directory is not actually leaked, only reported wrong.
  Shape 3 makes `profile_removed: false` *correct* and the live_96 assertion wrong. Adjudicate
  that explicitly; do not pick a shape because it makes a test green.

### Tasks

#### A. Measure [0/2]
- [ ] `profile_skip_reason` is recorded across at least 20 `launch`/`daemon stop` cycles
- [ ] The value is `remove-failed` (shared mechanism) or is not (different defect — say which)

#### B. Decide [0/2]
- [ ] One of the three shapes is chosen, with the other two's rejection recorded
- [ ] If shape 3 wins, `live_daemon_stop_profile_path_matches_launch_json`'s assertion is
      corrected the way iteration 242 corrected the iteration-97 Theme C assertion — asserting
      what the command can actually guarantee, and never silent about the other outcome

### Acceptance Criteria [0/3]

- [ ] The mechanism behind `profile_removed: false` is named from a recorded
      `profile_skip_reason`, not inferred from its resemblance to Part A of iteration 242
- [ ] A test pins whichever behaviour is chosen
- [ ] `live_daemon_stop_profile_path_matches_launch_json` passes twenty consecutive runs, or its
      assertion is corrected and *that* passes twenty

## Part B: the live verifications iteration 242 could not run

Iteration 242 ran under an unattended loop agent with no live sweep. It verified Part A directly
(30 iteration-97 gate runs against a real Firefox) but left three live checks unrun. They are
written and committed; they need a sweep.

- `live_242_launch_ownership::live_242_marker_names_test_from_direct_launch` — a profile created
  by a direct `Command`-built launch names its spawning test in `.ff-rdp-owner-test`.
- `live_61r_eval::live_eval_on_hn` — exercises the new `common::await_document_ready()` readiness
  wait and its `READINESS:` / `SITE:` diagnosis.
- A dual-gate `cargo run -p xtask -- live-sweep`, which is the only evidence that closes
  iteration 242's Part B and Part C acceptance criteria.

### Acceptance Criteria [0/2]

- [ ] Both named live tests pass against a real Firefox
- [ ] A dual-gate sweep runs, and iteration 242's live-dependent ACs are ticked or their failures
      filed — **whichever the sweep actually shows.** Iteration 242 left six ACs unticked rather
      than tick them on reasoning; do not undo that by ticking them on a sweep that did not
      exercise them.

## Part C: does `live_160_selector_diagnostics_survive` leak its Firefox?

Carry-over from iteration 242's Part C task list (the "still open, and unrelated to the above"
item), which itself carried it from iteration 176's closing sweep. Iteration 176 (`done`, its own
diff touches only `eval`'s statement scanner) found an orphaned Firefox (pid 79010) attributed to
`live_160_selector_diagnostics_survive` five hours after that test should have exited, and closed
without determining whether the test itself leaked it or the process was an unrelated interrupted
sweep's orphan. Iteration 242 re-flagged it as open and out of its own scope. It has had no owner
since 176 closed.

- [ ] Run `live_160_selector_diagnostics_survive` (`tests/live/live_160_envelope_honesty.rs`) in
      isolation, under `FF_RDP_LIVE_TESTS=1`, and confirm with `ps` that no `firefox` process
      naming that test's `.ff-rdp-owner-test` marker survives the test's exit.
- [ ] If it does leak: find the missing guard — Part B of iteration 242 enumerates launch sites,
      not guard sites, and this file was in scope for that scan
      (`tests/iter_242_launch_site_ownership.rs`); if the scan already covers it, say why it did
      not catch this case.
- [ ] If it does not leak in isolation: say so and close this item as "unreproduced, likely an
      unrelated interrupted sweep's orphan" rather than leaving it open indefinitely.

### Acceptance Criteria [0/1]

- [ ] The leak question has a landed answer (reproduced-and-fixed, or unreproduced-and-closed),
      not a re-deferral

## Out of scope

- `live_137_consent_accept_via_daemon`'s `live_target_count: 0` — [[iteration-251]] owns it.
- The chunk-A/chunk-B `--test-threads=1` methodology from the old iteration 243. It predates
  `xtask live-sweep` and iteration 242 already recorded the premise as superseded.

## References

- [[iteration-242-profile-liveness-flake-in-prune-all]] — the measurement, and DEC-054
- [[iteration-224-with-page-daemon-connection-reset]] — where the `daemon stop` observation came
  from
