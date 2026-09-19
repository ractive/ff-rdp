---
title: "Iteration 260: daemon stop's profile removal races the browser it just stopped, and the live verifications iteration 242 could not run"
type: iteration
date: 2026-09-07
status: obsolete
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
takeover_reconciliation_252: "2026-09-09, reconciliation after252: the out-of-scope consent/live_target_count reference to iteration251 is historical; the pending owner is now iteration262-daemon-live-target-never-promoted. PartB overlaps the explicitly authorized242 verification obligation and remains selected separately; PartsA/C are outside this takeover. The252 sweep passed both named PartB tests but does not substitute for the owed distinct242 closure/audit or tick its remaining ACs."
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

#### A. Measure [1/2]
- [x] `profile_skip_reason` is recorded across at least 20 `launch`/`daemon stop` cycles
- [ ] The value is `remove-failed` (shared mechanism) or is not (different defect — say which)

#### B. Decide [0/2]
- [ ] One of the three shapes is chosen, with the other two's rejection recorded
- [ ] If shape 3 wins, `live_daemon_stop_profile_path_matches_launch_json`'s assertion is
      corrected the way iteration 242 corrected the iteration-97 Theme C assertion — asserting
      what the command can actually guarantee, and never silent about the other outcome

### Acceptance Criteria [1/3]

- [ ] The mechanism behind `profile_removed: false` is named from a recorded
      `profile_skip_reason`, not inferred from its resemblance to Part A of iteration 242
- [ ] A test pins whichever behaviour is chosen
- [x] `live_daemon_stop_profile_path_matches_launch_json` passes twenty consecutive runs, or its
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

### Acceptance Criteria [2/2]

- [x] Both named live tests pass against a real Firefox
- [x] A dual-gate sweep runs, and iteration 242's live-dependent ACs are ticked or their failures
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

- [x] Run `live_160_selector_diagnostics_survive` (`tests/live/live_160_envelope_honesty.rs`) in
      isolation, under `FF_RDP_LIVE_TESTS=1`, and confirm with `ps` that no `firefox` process
      naming that test's `.ff-rdp-owner-test` marker survives the test's exit.
- [ ] If it does leak: find the missing guard — Part B of iteration 242 enumerates launch sites,
      not guard sites, and this file was in scope for that scan
      (`tests/iter_242_launch_site_ownership.rs`); if the scan already covers it, say why it did
      not catch this case.
- [x] If it does not leak in isolation: say so and close this item as "unreproduced, likely an
      unrelated interrupted sweep's orphan" rather than leaving it open indefinitely.

### Acceptance Criteria [1/1]

- [x] The leak question has a landed answer (reproduced-and-fixed, or unreproduced-and-closed),
      not a re-deferral

## Out of scope

- `live_137_consent_accept_via_daemon`'s `live_target_count: 0` — [[iteration-262-daemon-live-target-never-promoted]] owns it (251 was absorbed).
- The chunk-A/chunk-B `--test-threads=1` methodology from the old iteration 243. It predates
  `xtask live-sweep` and iteration 242 already recorded the premise as superseded.

## References

- [[iteration-242-profile-liveness-flake-in-prune-all]] — the measurement, and DEC-054
- [[iteration-224-with-page-daemon-connection-reset]] — where the `daemon stop` observation came
  from


## Owed242 verification reconciliation, 2026-09-14

The separately requested242 sweep at `19f4e236a70399c2984e6d46eb15bdac37a55181` executed343 exact names
across five tiers:331pass/12fail, zero skips/reclassifications/profile leaks.
Both named PartB live tests passed and every failed/unmet242 requirement has an
explicit disposition in242's new evidence section, so the two PartB ACs are now
satisfied. This does **not** close this plan: PartA's twenty-cycle mechanism and
behaviour proof and PartC's attributable orphan investigation were not executed.
HN's passing verdict also does not diagnose the historical empty-title failure;
retain242's unticked readiness-versus-site mechanism requirement here. On its next
failure, retain the READINESS/SITE diagnostic and actual document state before
naming a mechanism. The obsolete chunk split remains unrun, not silently replaced
by profile-zero counts. Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/sweep.log` and
`sweep-reconciliation.json`. No260 product implementation is claimed.

## Implementation preflight — 2026-09-19

Part B's live verification is already discharged by the September 14 evidence; do not repeat a sweep solely because the historical introduction says it is owed. Parts A/C remain investigations. Stop already exposes profile_skip_reason; a stop-path ENOTEMPTY race is not established by the source audit. The cited live_160 test already owns a LiveFirefox guard, so do not assume missing ownership. Inspect the remaining one-shot removal path and obtain attributable measurements before choosing a repair. Iteration 261 is not a prerequisite for existing stop diagnostics.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Isolation evidence and disposition — 2026-09-19

Investigation base: `3a398566281abfa48ce96f1f8d855e6c1116a302`, Firefox 156.0,
macOS, stable Rust 1.98.1. Product and test source remain unchanged. Evidence lives
in `.git/ralph-loop/20260919-queue/iter260/` in the primary checkout; the execution
checkout is `/Users/james/.cache/ff-rdp/queue-20260919-260`, with its own Cargo target.

Part A's twenty isolated `launch --headless --debug-port 39301..39320` / `daemon
stop` cycles all returned `stopped: true`, `profile_removed: true`, and an actual
`profile_skip_reason: null`. Each launch and stop envelope, stderr, timestamp and
before/after owner-PID observation is retained (`cycle-N.*`, `measure.sh`,
`measure.log`). There was no skipped removal to diagnose. Separately, twenty
consecutive invocations of the unchanged exact test
`live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json`
passed with `FF_RDP_LIVE_TESTS=1`, `--exact --include-ignored --nocapture
--test-threads=1` (`test96-1.log` through `test96-20.log`). These are twenty test
verdicts, not the plain cycles relabelled as test runs.

Disposition: **obsolete**, independently reviewed with zero findings: the planned race was not
reproduced in this bounded investigation, as Theme A explicitly permits. Neither
ENOTEMPTY nor a sweep-load cause is established. Leave the mechanism task/AC and
chosen-behaviour test AC unticked. No one of the three proposed repairs was
selected: a retry, a new reap wait, and an assertion change each need a reproduced
failure and its actual cause before implementation is justified. The conditional
shape-3 task therefore also remains unticked. The passing repetition AC is ticked
on its own evidence; it does not prove the historical failure impossible.

Part C's exact test
`live_160_envelope_honesty::live_160_selector_diagnostics_survive` passed in
isolation (4.19s libtest). During the run, `.ff-rdp-owner-test` named that exact
test and `.ff-rdp-owner-pid` named PID 40124; the launch log records port 51294.
`test160.ps-during` captured its command line naming the same profile. Immediately
after the test exited, `ps` showed no process for that PID and no Firefox command
naming the isolated home/profile; no owner marker survived. See `part-c.sh`,
`test160.markers-during`, `test160-launches.log`, `test160.ps-after`, and
`test160.owner-40124.ps-after`. The existing `LiveFirefox` guard reaches
`kill_pid_and_wait` on drop; no missing guard was found. Close the orphan question
as **unreproduced, likely an unrelated interrupted sweep's orphan**; the latter is
a plausible historical explanation, not a measured attribution. The conditional
leak-repair task remains unticked because no leak occurred. The landed-answer AC records this evidence-backed closure in the iteration checkpoint.

The first Part C run also passed (PID 39800, 4.14s), with no surviving owner PID,
but the observer initially searched `$FF_RDP_HOME/profiles` instead of the actual
`$FF_RDP_HOME/ff-rdp/profiles`. It therefore lacked during-run marker evidence.
That operator error prompted the one corrected repeat, not a test failure or a
product repair. Preserve `part-c-attempt1/` and its separate owner-PID observation.

Part B reuses the September 14 owed242 evidence above, including its actual
331pass/12fail, five-tier accounting and `leaked=0 unattributed=0`; no new full
sweep was required for this documentation-only disposition. Unchanged source gates
are reused from `iter263-security/commands.log`: ordered fmt, strict workspace
clippy and workspace tests passed on the final rustls-updated inputs. The measured
source/dependency tree matches those merged inputs. Before checkpoint, the branch
fast-forwarded to verified main `38add406f711434f520fa06494b2e02ab3f5e0b2`, incorporating
261's independently reviewed launch-warning change; that merge's unchanged ordered
gates (`iter261/gates-record.md`) cover the final source tree. The stop-cleanup and
selector-test sources measured here did not change. No source or test changes are
introduced by260. Documentation checks run on this plan are recorded in
`iter260/checks.log`; final status/heading checks were repeated after finalization.

## Carry-over

| Item | Disposition |
| --- | --- |
| Historical false removal / mechanism task and AC; choice and chosen-behaviour test AC; conditional shape-3 correction | **No plan, with a stated reason:** no skipped removal occurred in twenty cycles or twenty exact tests. Preserve unticked requirements and obsolete disposition. Reopen only on a recorded false removal, retaining the actual skip reason, OS error/log, profile/owner state and failed-occurrence conditions before choosing a repair. |
| Historical six dead-owner directories in iteration245's carry-over row 5 | **No plan, with a stated reason:** that historical observation does not establish a stop-path cause; this investigation found none. Dead-owner leftovers and live-owner leaks are distinct. A new reproducible failed stop removal with attribution reopens the investigation; no source repair is justified from that old count alone. |
| Historical selector-test orphan / conditional missing-guard repair | **Closed in this iteration's evidence:** exact isolation, marker/PID attribution and post-exit process check above; no leak reproduced. Landing remains supervisor-owned. |
| First selector observer missed the actual profile-root nesting | **Closed in this iteration's evidence:** operator path correction and one attributable repeat; first passing result preserved separately. |
| HN readiness-versus-site diagnosis inherited from242 | **No plan, with a stated reason:** the retained pass does not explain the old failure, and no new HN failure fired its trigger in this investigation. Preserve242's unticked mechanism AC. On the next failure, retain READINESS/SITE diagnostics and actual document state before naming a cause. |
| September14 owed242 sweep failures | **Fold:** retain each exact named failure and its existing owner in242's evidence table; this docs-only closure does not change those dispositions or turn that sweep green. |
