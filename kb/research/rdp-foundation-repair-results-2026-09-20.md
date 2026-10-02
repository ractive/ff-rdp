---
title: Foundation repair execution results and stopped checkpoint
date: 2026-09-20
status: blocked
tags: [research, handoff, rdp]
---

# Foundation repair execution — stopped 2026-09-20

This is the final execution checkpoint for the owner's adopted
[repair launch](rdp-foundation-repair-launch-2026-09-20.md), following the completed
[assessment](rdp-foundation-assessment-2026-09-20.md). It supersedes earlier starting
states without erasing any historical evidence or consumed allowances. The run
implemented and reviewed eligible work; it did not stop at another assessment.
All five selected iterations remain blocked. No selected iteration was completed
or merged. Only the conditionally authorized foundation PR263 merged.

Run evidence is private and local:
`/Users/james/devel/ff-rdp/.git/ralph-loop/20260920-foundation-repair/`.
Read `state.json`, `allowance-ledger.json`, `inventory-final.json`,
`final-process-check.json`, `final-process-verification.json` and
`final-lock-release.json` before any resume. Earlier archives remain intact.
Raw daemon logs contain ephemeral authentication material; retain them privately.
There is no background continuation. All execution workers have stopped.

## Verified foundation merge and recoverable refs

[PR263](https://github.com/ractive/ff-rdp/pull/263) merged through GitHub using
`gh pr merge --merge --match-head-commit` on exact head
`5421b3e40f27ffd076ac1e0a0f5bc7eca8b60e54`, after ten green checks, independent
review with findings addressed, and matching ordered local gates. Verified merge:
`8666a5324905793538c59532e909e0808beee7f3`, at 2026-09-20T12:27:30Z.
No full sweep for the standalone foundation spike exists or is claimed.
See `pr263-premerge.json`, `pr263-merged.json`, `pr263-source-reuse.json` and the
preserved sibling `20260920-rdp-foundation-spike` run directory.

| Iteration | Clean, pushed checkpoint | Execution checkout |
| --- | --- | --- |
| 262 | `8f72ef38e4b09407c029d87b87b37eb5824ac59d` | `/Users/james/.cache/ff-rdp/remaining-20260919-262` |
| 268 | `27604336485c5fd458b8f4d88ed40cbc56190ce4` | `/Users/james/.cache/ff-rdp/remaining-20260919-268` |
| 271 | `10312d2fecb5f905f0a23df795d69dd31a0a30af` | `/Users/james/.cache/ff-rdp/queue-20260919-271` |
| 259 | `cc44e873e977c979729372010ba65fcf088341b9` (unchanged) | `/Users/james/.cache/ff-rdp/queue-20260919-259` |

Each changed branch preserves its previous checkpoint and the foundation merge
as ancestors. 271 additionally preserves partial repair `f044e6791683126b9d45e4444982c918d6648e88`.
262 and268 product source was restored to the foundation;271 source was restored
to its combined foundation plus preserved partial271 baseline. No failing candidate
source was committed. Correction and diagnostics bytes, manifests, diffs, failed
validation and review reports remain in each iteration's run archive.

Primary planning remains on `planning/remaining-iterations-20260919` at
`db6aabd1048a1731f3ff57536ea0771ac6ddc0f0`; preserved historical planning ancestors
include `c99068e6`. The three preexisting assessment/next-session/repair-launch
files remain byte-identical and uncommitted. Existing Hyalo/Jev feedback is
preserved with append-only execution observations. This results file and the new
AGENTS checkpoint are deliberately uncommitted; no unrelated planning was pushed.

## 262 — reviewed correction proved, required validation failed

Two new repair batches adapted the archived correction to the private
`ConnectedTab` foundation. Managed daemon acquisition uses a disposable
authenticated primary-watcher snapshot and central installation, without issuing
shared legacy `getTarget` on Pending/error. Direct/unmanaged fallback remains.
The final batch addressed independent review's settlement-budget regression with
one-snapshot best-effort refresh and six actual-caller regressions. Fresh final
review approved that delta and the frozen matched prevention proof.

Final freeze: `iter262/repair2/`, manifest SHA256
`3ff44c33f297c4817f7591fd6b6a8dabedaec633fe990b3947042f048568e072`.
Supported `eval 1` autostart replaced the historical invalid `daemon start` setup.
Both attempts used actual Firefox156.0, the same proof binary and fresh managed
profiles, with exact source/binary pins and desktop identity guards.

- Attempt4/pre,15:14:14–15:14:34 CEST: expected proof exit101, four legacy
  `getTarget` requests after setup, watcher readiness false and live form null.
  Navigation/eval succeeded; all54 command receipts had exit0. Setup had one
  additional legacy request. Inner and outer cleanup both passed.
- Attempt5/post,15:18:13–15:18:18 CEST: proof exit0, zero legacy requests in setup
  and later phases, ready target with initial/replacement forms and evaluation on
  the exact replacement console actor with browsing-context/inner-window IDs.
  All eight command receipts and both cleanup checks passed.

This count spans navigation/readiness/evaluation; do not attribute all four
requests solely to navigation or claim the raw count alone proves suppression
inside Firefox. Retained historical reviewed causal capture supplies that
attribution. See `proof-result.md`, `attempt4-verdict.json`, both command receipts
and `correction-proof-4/`, `correction-proof-5/` under repair2.

The required full253 regression block ran with both live gates: seven passed,
one failed. Daemon committed submission returned the correct ready destination,
but **4.248194792 seconds exceeds the original <2-second assertion** in
`live_253_outgoing_page.rs:357`. Cause remains unestablished. No retry or timing
criterion change occurred. Both new repairs were exhausted, so no required
closing sweep started; `iter262/run-sweep.py` was prepared but never executed.
All three consecutive qualifying dual-gate sweeps, all137/both145 results,
watcher-source disposition and remaining acceptance work are still owed.

The candidate's default-parallel workspace gate separately failed twice in
`harness_session::isolated_launch_timeout_still_runs_scoped_cleanup_and_passes_product_bound`
with an absent launch-timeout receipt (ENOENT). Exact isolation passed; that does
not establish a cause. Ordered serial workspace validation was2516passed/0failed/
419ignored, and is not default-parallel green. New plan278 owns this independent
missing-receipt investigation; it is outside execution scope. It cannot absorb
the required253 regression or waive262 completion.

Original262 criteria were not rewritten: Locate2/2, Fix1/2, Test1/1, AC2/5.
Only mechanism and matched live proof criteria advanced. The fix remains archived,
not landed. Full1182-file restoration archive
`iter262/blocked-checkpoint/worktree-files.tar.gz` has SHA256
`cc63251da568539023c4b2f91da46a91815a990b38bb90efa7c0a021c270e7c3`.
All569 frozen source inputs were retained before restoring567 foundation inputs.
Restored CLI/live/e2e/unit rebuild passed in15.720s. Matching unchanged foundation
fmt, strict Clippy and workspace gates2499/0/418 support the restored documentation
checkpoint only. See its source-restored, foundation-gates-reuse and commit receipts.

## 268 — two instrumentation repairs, no eligible capture

The first new batch fixed the three previously identified tooling defects and
adapted authentication/shutdown instrumentation to the foundation. Independent
review rejected six completeness/deadline/ordering classes. The final batch
improved route chains, interruption handling, lifecycle and worker-qualified
ordering, and explicitly labeled deferred writer timestamps. Fifteen offline
methods plus build and strict local Clippy passed; these are not live evidence
or a full workspace gate for the diagnostics.

Complete attribution remains unavailable: the daemon drops worker JoinHandles
and can exit while a worker sleeps in a one-second drainer interval. External
process death proves interruption, not worker return or a complete shutdown
contract. A partial mock control cannot unlock the mandated capture prerequisite.
Final independent review approved only the honest blocked checkpoint/restoration,
retaining two source-verified tooling defects:

1. Worker outcome can satisfy completeness without mandatory subsequent shutdown
   swap records; omitted swaps or a process-death gap may pass with another initiator.
2. The unqualified generic watchdog can block inside synchronous observation and
   grants separate cleanup intervals per PID instead of one aggregate deadline.

Current archived capture wrappers refuse with exit78 before launch; the compiled
mock control is ignored and guarded false before creating children. The generic
watchdog is unqualified and unwired. No268 Firefox capture or actual mock control
ran:0/6 capture slots and302 discovery seconds remain untouched. Original tasks
and AC remain0/4. No third new repair is authorized.

See `iter268/review1.md`, `review2.md`, their phase reports, both repair archives
and final `freeze.json`. The final568 source/76 artifact/four binary pins and
repair1 artifacts were verified. Restoration used the actual freeze manifest;
a stale delegated `restore-files.json` reference did not name an existing file.
Restored567 foundation inputs matched ordered foundation gates2499/0/418; rebuilt
artifacts passed in7.370s. These results validate the restored checkpoint, not the
withdrawn diagnostic executor or missing causal proof.

## 271 — final pair rejected, zero new BBC captures

Both new preparation repairs preserved native JavaScript, strict BBC/native
accepted/absence-or-zero-size assertions, direct managed launch and one consent
per arm. Proposed passive samples at0/250/1000/3000/10000ms do not supply missing
decision-time selector/geometry/timeOrigin or establish historical causality.
Sourcepoint/no-CMP cannot count as native dismissal. Original tasks and AC0/4
remain unchanged; six historical passing controls were not repeated.

Review1 rejected unbounded synchronous hashing/reads/sealing, deletion before a
fatal desktop check, delayed exceptions recorded as success, and loss of completed
navigation/sample observations on timeout. Final repair2 added earlier evidence
persistence, alarms, bounded reads, restricted sealing and all-home retention.
Six compiled offline checks, actual extracted JavaScript, five shared controls
and eleven new runner controls passed. Build and strict Clippy passed.

Final independent review rejected frozen manifest
`36c788677928fc2e7cf24ba471176103cd29558730e2593ae58491e177ec5334`
after verifying695 pins including110 Firefox package files. Three blockers remain:

1. Cleanup catches a one-shot timeout and continues; a subsequent file open can
   block without another alarm because it opens before checking the deadline.
2. Failure writing `command.json` skips browser termination/listener checks.
   Killing the test process group does not kill Firefox's separate process group.
3. Timeout after emitting success JSON can leave `pair-end.json` exit0 while the
   actual process exits70; an expired fallback deadline prevents correction.

Both new repair batches are consumed. No `review2-approval.json`, capture directory
or pair-command receipt exists. The supervisor's `iter271/run-reviewed-pair.py`
was never run and requires explicit approval for the exact freeze. Do not execute
the archived inner runner: final review is REJECT. Zero of the narrowly reopened
maximum two captures were used, and historical0/6 slots/370seconds are unchanged.

All567 combined-baseline source hashes were restored from repair1 baseline bytes,
limited to the three diagnostic source files. Rebuild passed in0.352s. Recorded
stable-update/fmt/strict-Clippy/workspace gates2500/0/418 match this combined
foundation/partial271 baseline and support its restored documentation checkpoint;
they do not validate the rejected pair or complete271.

## Allowances, untouched blockers and full inventory

No new session or remaining numerical time resets a consumed repair cap.
Historical and new grants remain separate in `allowance-ledger.json`.

| Iteration | New preparation charged /3600s | New repairs | Retained discovery now | Total historical capture slots used |
| --- | --- | --- | --- | --- |
| 262 | 2888s;712s numerical remainder | 2/2 | 282s after270s charged | 5/6 (two new) |
| 268 | 3058s;542s numerical remainder | 2/2 | 302s untouched | 0/6 |
| 271 | 2589s;1011s numerical remainder | 2/2 | 370s untouched | 0/6; reopened pair0/2 |

The262 discovery charge conservatively includes continuous15:14:14–15:18:44 CEST
wall time and interpretation. Supervisor preparation charges are conservative
allocations, not precise attention measurements. Final restoration/validation and
handoff administration are recorded separately; mixed worker compilation time
was not subtracted. All older two-repair allowances remain consumed.
Implementation/review timing and repeated check receipts remain in each archive.
Actual token totals and a comparable supervisor-only baseline are unavailable;
no measured token saving or halving of supervision overhead is claimed.

259's recorded execution rejection remains binding; no permitted path was
established and no retry, rephrasing, alternate-tool probe or execution agent was
launched. Its blocker is retained under sibling run
`20260919-queue/iter259/execution-blocker.md`. 266 still requires adequate,
independently reviewed259 reply-ownership semantics on main, which remain absent.
No266 implementation was started. Further eligible work requires the specific
entry conditions and any additional repair authority; unused capture time alone
is insufficient and the restricted path must not be probed.

The final Hyalo inventory union over planning and the three execution branches
contains14 pending plans:147,203,259,262,266,268,271,272,273,274,275,276,277,278.
Branch-local plan copies differ; use each iteration's checkpoint as authoritative
for its latest status. 275 is preserved on262 and277 on271. 278 was newly filed
and reviewed on262.147/203 stay parked;272–278 remain unexecuted. No broader queue
was launched and no original acceptance criterion was weakened.

## Cleanup and next-session requirements

Both262 proof runs recorded successful inner/outer cleanup and zero leaked
profiles; the253 block also completed and no run-owned runtime remained at final
inspection. Known ports62873,62883,63022,63032 and6000 were not listening. There
were no268/271 capture processes. Desktop Firefox57827 was separately verified
with start time Sun Sep20 14:58:37 2026. Earlier desktop1112 disappeared before
the proofs; cause is unknown and no claim is made that it survived that interval.
Do not kill or modify the desktop browser. No Firefox installation was changed.

The first final process audit had a parser/assertion mismatch for the desktop
runtime list; its raw receipt is retained. The corrected verification explicitly
checks process executable names, ownership paths, listeners and desktop identity.
Final lock release is recorded only after all writers are stopped and cleanup is
verified; consult the actual release receipt before resuming.

Codex weekly usage was34% remaining at15:10:30UTC, above the owner's25% stop
threshold; final state records the last fresh check. This stop is due to required
gates, prerequisites and exhausted repair counts. Preserve all archives, pushed
refs and uncommitted handoffs. Resume from verified proof/review dispositions;
do not replay captures, treat restored source as the candidate correction, or
start more work automatically. Hyalo/Jev execution observations are appended to
the existing feedback notes; no Jev calls or paid benchmark occurred.
