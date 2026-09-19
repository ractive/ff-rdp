---
title: "Iteration 263: two live sweeps ran 319 of 320 qualified tests and said nothing about the one they dropped"
type: iteration
date: 2026-09-07
status: done
branch: iter-263/live-sweep-plan-dropped-one-test
depends_on:
  - iteration-252-content-process-resources-on-the-direct-route
first_call_sites: []
dogfood_path: |
  # Reproduce by running a sweep *while* `cargo test --workspace` is running,
  # which is the only condition under which this was observed:
  #
  #   cargo test --workspace -q &                  # hold the build lock
  #   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
  #     cargo run -q -p xtask -- live-sweep | head -1
  #
  # Compare that header's `N qualified` against the uncontended plan:
  #
  #   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
  #     cargo run -q -p xtask -- live-sweep --dry-run | head -1
  #
  # On 2026-09-07 the contended runs printed 319 and the dry-run printed 320.
tags: [iteration, live-sweep, discipline, false-green, carry-over]
iteration_252_recheck: "2026-09-09 continuation: both full sweeps included all 320 compiled ignored CLI names and all nine core names. Each had 329 actual verdicts with zero expected/observed name discrepancies. The first is diagnostic only (briefly paused and predates a test-diagnostic repair); the second is the final implementation sweep. No live-sweep parser or guard changed, so this negative observation does not establish that the historical omission was fixed. Keep this plans original acceptance criteria pending."
iteration_252_review_repair: "Another dual-gate repair sweep reconciled all329 compiled ignored names and observed verdicts across five tiers with no missing names or reclassifications;320 passed and9 failed. Scanner and omission guard are unchanged so the historical omission is not claimed fixed and original ACs remain pending. Evidence: iter252/review-repair-1/expected-live-names.tsv and verdicts.tsv and empty name-discrepancies.txt."
---

# Iteration 263: a sweep that runs 319 of 320 and reports it as a full run

Carry-over from [[iteration-252-content-process-resources-on-the-direct-route]], filed before
that PR merges per CLAUDE.md's carry-over rule.

## What was observed

Iteration 252 added one live test,
`live_252_console_follow_content_resources::live_252_console_follow_sees_content_process_messages_both_routes`,
with the standard gate `#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]`.

Two consecutive `live-sweep` runs on 2026-09-07 printed:

```text
live-sweep: -p ff-rdp-cli --test live: 319 qualified …
test result: FAILED. 304 passed; 15 failed; … 12 filtered out
test result: FAILED. 307 passed; 12 failed; … 12 filtered out
LIVE_SWEEP_SUMMARY executed=328 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=328
```

`grep live_252` over both logs found **nothing** — no `ok`, no `FAILED`, no mention. The test
existed in the tree (mtime 14:30, both sweeps started at 14:39 and 14:44), was registered in
`tests/live/main.rs`, and was present in the compiled binary
(`strings target/debug/deps/live-* | grep live_252` → 7 hits;
`--list` names it). Immediately afterwards, and repeatedly since, the plan is **320**:

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -q -p xtask -- live-sweep --dry-run
live-sweep: -p ff-rdp-cli --test live: 320 qualified …
LIVE_SWEEP_SUMMARY executed=329 … total=329
```

The only difference between the runs that said 319 and the runs that say 320: the two 319 runs
were launched while `cargo test --workspace` was running in the same working tree.

## Why this matters more than one test

`executed=N` is the number `live-sweep` exists to make trustworthy. iteration 155 built the
partition so a skipped test says `ignored` in libtest's own vocabulary; iteration 158 made a
failed Firefox launch panic rather than return early; iteration 197 gave a stalled phase a
`timed_out` number so silence could not read as success. This is the same failure one level up:
a test vanished from the *plan itself*, so no counter had anything to report and `total=328`
looked internally consistent. `executed + skipped + preexisting == total` held — over the wrong
corpus. Nothing in the summary line can distinguish "we ran everything" from "we ran everything
we happened to notice."

That is exactly the shape `kb/discipline-rationale.md` warns about, and it is why iteration 252's
own live test came within one `grep` of being reported green without ever having run.

## Scope

- [ ] reproduce the 319/320 split deterministically, or establish that it cannot be reproduced
      and say what evidence would change that
- [ ] find where a qualified name is dropped between `scan_modules_dir` and the `--exact` list
      the phase is handed — candidates worth checking first: the directory read racing a
      concurrent `cargo` writing into the tree, and any cross-check of the partition against a
      binary listing that a concurrent build can leave stale
- [x] make the sweep **fail rather than under-report** when the names it was handed do not
      account for the whole gated corpus — the counter iteration 197 added for stalled phases,
      applied to the plan instead of the run
- [x] a regression test that a dropped name is reported, not silently absent

## Acceptance Criteria [3/3]

- [x] the mechanism is identified and named in the plan, or the plan is closed with the
      measurements and a written reason the mechanism could not be found
- [x] a sweep whose `--exact` list does not cover every gated test it discovered exits non-zero
      and names the missing tests
- [x] `cargo run -p xtask -- live-sweep --dry-run` and a real sweep of the same tree, with the
      same gates, report the same `qualified` count — asserted by a test, not by hand

## Notes

- Do **not** "fix" this by making the sweep re-scan more often; the failure to guard against is a
  plan that is silently smaller than the corpus, whatever produced it.
- Related: [[iteration-203-live-sweep-watch-conditions-third-holder]] (the sweep's other standing
  watch conditions).


## Owed242 exact-name negative, 2026-09-14

At `19f4e236a70399c2984e6d46eb15bdac37a55181`, compiled ignored-name lists and actual verdicts match
for all343 names in five tiers (CLI334/core9):331pass+12fail, no missing,
duplicate or extra test; no reclassifications. This additional uncontended
negative does not explain or fix the original319/320 omission. No scanner or
coverage guard changed; original tasks and ACs remain pending.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/expected-names.tsv`, `observed-names.tsv`,
all five binary enumeration files and `sweep-reconciliation.json`.

## Iteration257 inventory reconciliation, 2026-09-14

Adding the direct drawSnapshot guard increases this branch's ignored CLI corpus
from334 to335. The closing dry-run and real sweep both qualified344=335CLI+9core.
Every compiled ignored name has exactly one verdict:342pass+2fail, no missing,
extra, duplicate, skipped or reclassified name across all five tiers. The new
guard and all seven original screenshot regressions passed. This is another
uncontended accounting negative, not a scanner fix or an explanation for the
historical319/320 discrepancy; original tasks/ACs remain unchanged.
Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/`,
`enumeration-*.txt`, `expected-names.tsv`, `observed-verdicts.tsv`,
`name-discrepancies.txt`, `duplicate-names.txt` and `sweep-dry-run.log`.

## Implementation preflight — 2026-09-19

The source-based inventory still lacks independent reconciliation with compiled test enumeration. Later exact-name reconciliations are negative observations, not an installed guard. Directory-entry errors discarded via filter_map(|e| e.ok()) are a possible omission path, not proof of the historical319/320 cause; speculative concurrent source-directory writes are also unproved. Bound historical investigation and preserve the original permitted unsuccessful-investigation disposition. Implement and test detection/naming of uncovered gated tests and dry-run/real qualified-count parity without repeatedly running whole live suites during development.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Implementation and closing evidence — 2026-09-19

The historical 319/320 mechanism could not be established. The filing commit changed only this
plan, later uncontended reconciliations repeatedly found every compiled name, and the original
logs retain no directory-entry error or compiled enumeration from the failing instant. Concurrent
workspace compilation is the only recorded correlation, not a demonstrated cause. The old
`filter_map(|entry| entry.ok())` directory scans were a credible silent-omission path, so they now
propagate entry errors, but there is no evidence that such an error occurred in either 319 run.
Evidence that would change this conclusion is a preserved failing run where compiled
`--ignored --list` contains a name absent from the source plan, or a reproducible directory-entry
or source mutation during classification that produces that exact set difference.

Before either dry-run or real execution reports a count, `live-sweep` now enumerates every
target's compiled ignored tests through libtest. Every compiled name absent from the
source-derived gated plan is a named hard failure. Source-only names are accepted as host
`#[cfg]` exclusions and filtered out before partitioning, counting, or constructing real
`--exact` commands. Three focused regressions cover a compiled name dropped from the plan, a
cfg-excluded source name being filtered, and the shared verified partition used for dry-run
reporting and real `--exact` commands. The existing empty-corpus diagnostic remains ahead of
compiled enumeration; all 74 sweep-runner tests pass after that integration correction.

Pre-review same-tree dual-gate evidence agreed exactly: dry-run and real sweep both report 344
qualified tests (335 CLI + 9 core). The replacement closing sweep passed all 344, with
`skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0`, and every target's profile
scan plus the final summary reported `leaked=0 unattributed=0`. The first closing attempt remains
preserved: 343 passed and `live_137_consent_accept_via_daemon` reached live targets in 21 ms but
returned the already-recorded Sourcepoint `detected_not_actioned` result; it is assigned to
[[iteration-262-daemon-live-target-never-promoted]] as that plan's distinct ready-target consent
action observation. It passed in the required replacement sweep. No rerun was used merely to
chase that red; the runner changed afterward to restore the empty-corpus diagnostic, requiring a
new final sweep.

All nine current xtask gates passed on that pre-review tree. The dogfood-script gate honestly skips because
this plan has `dogfood_path` but no `dogfood_script`; the live dry-run and real sweep are its
executed dogfood. Stable remained Rust 1.98.1, then `cargo fmt --all -- --check`, strict workspace
clippy, and workspace tests passed in the required order. The first workspace-test attempt found
the empty-corpus ordering regression above and is retained; the corrected final suite passed.

Evidence: `.git/ralph-loop/20260919-queue/iter263/`, especially
`logs/final-dry-run.log`, `logs/final-live-sweep.log`,
`logs/final-live-sweep-replacement.log`, `logs/xtask-gates-final.log`,
`logs/clippy-final.log`, and `logs/workspace-tests-final.log`.

## Independent review repairs — 2026-09-19

Repair 1 corrected host-cfg handling: source-only names are filtered against the host's
compiled corpus. Its final sweep accounted for all 344 tests: 343 passed and
`live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` failed with the known
Sourcepoint result; `LIVE_SWEEP_PROFILES leaked=0 unattributed=0`. The initial repair dry-run
had no port-6000 browser (335 qualified, 9 preexisting); the supervisor's matching-precondition
dry-run qualified all 344. These records supersede the earlier all-green run as repair-1
source evidence without erasing it. Evidence: `iter263-repair1/` and its
`supervisor/matched-dry-run.log` under `.git/ralph-loop/20260919-queue/`.

Repair 2 addresses the review's two test-coverage findings. `filter_compiled_target` is the
production operation called by `verify_compiled_corpora`; its regression now passes a target
through that operation and checks exact retained names. `plan_target_phase` builds the
partition, summary and actual phase command consumed by the real driver before its dry-run
branch. The parity regression compares the dry-run report with the command's actual `--exact`
arguments, with a host-excluded name and an unset network gate in the fixture. It also checks
that a compiled name without source metadata fails by name. These are simulated host fixtures
on macOS, not an actual Windows execution.

Removing the production retain operation caused the cfg regression to fail; omitting the first
name in the real command builder caused the parity regression to fail. Both mutations exited
101 on assertion failures, the original source was restored byte-for-byte, and all 74 runner
tests passed afterward. Evidence: `iter263-repair2/commands.json`, `mutation-filter.log`,
`mutation-selection.log`, `restored-focused.log`, and `source-frozen.rs` in the same run store.
The final-source dual-gate sweep and matching-precondition dry-run both qualified 344 names
(335 CLI + 9 core). Exact-name reconciliation found zero missing, extra or duplicate names:
342 passed and two failed. CLI passed 333/335; all nine core tests passed. The summary is
`executed=344 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=344`, with
`LIVE_SWEEP_PROFILES leaked=0 unattributed=0`; all five per-tier profile checks were clean.
The owned raw Firefox PID 15091 was terminated and reaped, and port 6000 was free afterward.

The failures are distinct: `live_137_consent_accept_via_daemon` failed before consent handling,
waiting 15,257 ms / 47 polls for live targets (debug port 51281, daemon proxy 51370, daemon PID
17257), assigned to iteration 262's target-promotion investigation. This is not the earlier
ready-target Sourcepoint action failure. `live_240_sustained_hops_never_desynchronise` failed
hop 20/40 with zero reconnects and `daemon auth failed: recv failed: Connection reset by peer
(os error 54)` (proxy 58186), assigned to iteration 268's existing pre-auth-reset investigation.
No common cause is inferred, no application behavior was changed here, and no green-chasing
rerun was performed. Final logs and all-tier reconciliation are in `iter263-repair2/live-sweep.log`
and `reconciliation.json`.

All nine xtask gates passed after the repair-2 sweep (dogfood-script explicitly skipped: no
`dogfood_script` field). Then actual `cargo fmt`, strict workspace clippy and workspace tests
passed in order. Stable 1.98.1 was verified earlier the same day and reused. Source remained
byte-identical to the mutation-restored freeze throughout. Exact commands, timestamps and exit
statuses are in `iter263-repair2/gate-commands.json`.

## Carry-over

| Observation | Disposition |
|---|---|
| Historical 319/320 cause remains unproved; the first two Scope investigation tasks remain unticked | **No plan, with a stated reason.** The shipped compiled/source set guard now turns any recurrence into a named hard failure. Re-open causal investigation only if that diagnostic captures a concrete set difference or a controlled source/directory mutation reproduces it. |
| First closing sweep: ready-target Sourcepoint `detected_not_actioned` in `live_137_consent_accept_via_daemon`; replacement sweep passed | **Fold — [[iteration-262-daemon-live-target-never-promoted]]** already owns this exact, distinct ready-target consent-action observation and requires it to remain separate from target-promotion failures. |
| First workspace suite: empty-workspace test saw Cargo-manifest enumeration error instead of the established zero-gated diagnostic | **Closed in this iteration.** The zero-gated guard now runs before compiled enumeration; all 74 runner tests and the final workspace suite pass. |

| Repair-2 closing sweep: target-promotion wait in `live_137_consent_accept_via_daemon` before consent handling | **Fold — [[iteration-262-daemon-live-target-never-promoted]]**, distinct from the preserved Sourcepoint action observations. |
| Repair-2 closing sweep: hop 20/40 pre-auth reset in `live_240_sustained_hops_never_desynchronise`, zero reconnects | **Fold — [[iteration-268-daemon-pre-auth-connection-loss]]**, already owns this exact test and wire signature; attributable failed-auth timing remains unavailable. |

## CI prerequisite and final dependency evidence — 2026-09-19

PR254's first head6326620 passed nine CI checks but failed supply-chain for
existing rustls0.23.40, RUSTSEC-2026-0285. Following CONTRIBUTING's patch-upgrade
procedure, Cargo.lock now selects rustls0.23.45 and required webpki0.103.15;
no provider/features or advisory policy changed. Independent security review
returned explicit zero findings, separate from the two completed implementation
repair batches. Local cargo audit and cargo deny pass.

The final dependency-state dual-gate sweep qualifies344 in dry-run and real
execution:341pass+3fail, all five tiers/exact names, zero skips/reclassifications
or profile leaks. Failures:137 target readiness15,194ms/47polls→262;
165 eval greeting-wait Timeout at proxy52636→267;240 hop3/40 auth EOF at
proxy55319,0reconnects→268. This does not establish causes or authentication
success. The prior342/2 repair record and every earlier attempt remain retained.
Source runner hash is unchanged from the final independent review. Evidence:
`.git/ralph-loop/20260919-queue/iter263-security/` and `iter263-security-review/`.
The separate ordered gate ledger applies to the updated lockfile. Owned raw
Firefox39206 stopped/waited; port6000free. No sweep was repeated to chase green.
