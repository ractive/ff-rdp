---
title: RDP foundation refactoring spike
date: 2026-09-20
status: completed
tags: [research, architecture, testing]
---

# RDP foundation spike

Owner-authorized standalone spike based on main
`07e736e9b8567c40f60f819ac4de78cbf106038b`. The planning branch and all stopped
iteration branches/evidence remain separate. This is not a resumption or completion
of iterations262/268/271/259/266, and no investigation allowance was reset.

## Product boundary

`ConnectedTab` owns target metadata and the transition after a successful target
refresh. Commands read it through `target()`; they can no longer assign it directly.
Eval's stale-actor retry and navigation predicate refreshes use the same operation,
which updates metadata and registrations together. Replacing a target invalidates
its old fronts; refreshing the same target preserves its live handles. A displaced
console is invalidated without invalidating an unchanged target. Failed refreshes
preserve the prior state. The registration helper is private.

The existing transport-only readiness probe remains separate, and the initial
getTarget/watcher protocol policy is unchanged. This is a concrete ownership slice,
not a complete document-generation model or a fix for every target-replacement race.
No daemon reply-handover, shutdown or consent semantics were redesigned.

## Test-session boundary

Extend existing Rust live-test helpers for exact ff-rdp binary selection, isolated
state/profile, explicit prelaunch preferences, supported daemon autostart, launch
receipts and observable owned cleanup. Use one existing local eval contract as the
first consumer. Firefox-free tests belong to the consolidated e2e target.

## Validation and review

Detailed command/environment/version/diff fingerprints, logs, review findings and
mutation evidence are retained in the main checkout's
`.git/ralph-loop/20260920-rdp-foundation-spike/`. Targeted development checks precede
one stable-batch ordered fmt, strict workspace Clippy and workspace test gate.
The initial spike checkpoint was local; no merge, full live-sweep or original
iteration acceptance completion is implied. Publication is recorded below.

## Follow-up boundary

Only after this slice is verified should more acquisition paths converge or a
shared navigation deadline/outcome model be introduced. Direct and daemon session
lifetimes must remain explicit. Existing restrictions, archived262 correction and
required original live proofs/sweeps remain intact.

## Verified spike outcome

- Target lifecycle checks:9passed; restored old unconditional invalidation in a
  temporary mutation and the same-target regression failed as intended. The
  mutation was restored before later checks.
- Existing command regressions:10navigation and24eval-related cases passed.
- Harness checks:7passed, including actual-launch nonzero/malformed-output cleanup,
  preservation on cleanup failure, exact binary resolution and bounded output.
- Local Firefox156.0: migrated isolated-session eval contract passed and reported
  `finish() == Ok(())`; existing local navigation-to-page-inspection contract passed.
  The first test captured binary/version/profile/PID/port/preferences during the
  run. The older navigation test has only its existing output plus command/input
  evidence; no missing receipt was reconstructed. Both ran before the final six
  mechanical Clippy corrections (borrowing, clone assignment and formatting),
  which did not alter command arguments or lifecycle behavior.
- Final ordered gates: `cargo fmt`; strict workspace/all-target Clippy; workspace
  tests. Aggregate2496passed,0failed,418ignored. The ignored tests are not live
  success evidence. Ruststable1.98.1 was checked via `rustup update stable`.
- Final source files match the recorded workspace-test input manifest. Independent
  Sol reviews covered product behavior and harness repairs. No blocking finding
  remains. One low coverage gap remains: runner timeout is tested directly, not
  injected through the full `IsolatedLiveFirefox::launch` caller. Its shared error
  branch was reviewed; a caller timeout regression is follow-up, not claimed done.
- Planning branch remained clean at`db6aabd1048a1731f3ff57536ea0771ac6ddc0f0`;
  remote main remained at the spike base. No PR/main merge or full live sweep.
- Weekly limit was monitored through cmux `/status`:55%remaining initially,
  53%near checkpoint, above the owner's25%stop threshold. No token/cost savings
  estimate is claimed. All implementation/review agents stopped before checkpoint.

This result supports continuing the RDP refactor incrementally. It does not close
any original blocked iteration. Remaining work includes the transport-only readiness
probe, complete document/frame lifetime handling, shared operation deadlines/outcomes,
and the separately constrained shutdown/reply-ownership/consent work.

## PR263 review repair

Published the initial checkpoint as [PR263](https://github.com/ractive/ff-rdp/pull/263).
Fresh independent Sol review returned three medium findings: outer launch, daemon
autostart and daemon-stop deadlines could expire before the corresponding product
budgets. Copilot review5260452061 on2a1e2602 returned one separate medium finding:
a subprocess polling error returned without killing/reaping the child. All four
were verified and consolidated into one harness repair batch.

The repair passes an explicit product launch timeout and adds outer cleanup
headroom, derives autostart time from the product override plus command overhead,
uses a shared40-second daemon-stop bound covering the complete escalation path,
and terminates/reaps the child on polling errors. Product protocol code is unchanged.

Ten focused harness checks passed. These include deterministic polling-error
injection and the actual isolated-launch caller timeout/cleanup path, closing the
previous caller-level coverage gap. The first run had9passes/1failure: the100ms
fake-launch deadline expired before the fixture wrote its receipt evidence. A
1-second deadline against the same10-second child preserves timeout, reaping,
explicit-product-bound and scoped-cleanup assertions; all10then passed.

The repaired isolated Firefox156.0 eval contract passed with`finish()==Ok(())`;
its recorded browser PID36191 exited and its private profile was removed. Final
ordered fmt, strict Clippy and workspace tests passed:2499passed,0failed,418ignored.
An earlier Clippy run rejected a test's duration units; the equivalent seconds
literal was applied before rerunning the complete ordered sequence. Source hashes
match the final test inputs. No full live sweep was run for this standalone spike.

Fresh delta review and exact-head CI are recorded in the PR and the same local
evidence directory. The initial head's10CIchecks passed; that result does not
certify the changed repair head. No original iteration was closed.
