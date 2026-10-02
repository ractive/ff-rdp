---
title: Another foundation repair spin — stopped checkpoint
date: 2026-09-20
status: blocked
tags: [research, handoff, rdp]
---

# Another spin — 2026-09-20

After the previous stopped report, the owner requested “Give is another spin.”
The supervisor explicitly interpreted this as one additional repair batch each
for262/268/271 within the previously retained preparation and capture time. No
new time allowance or capture reset was inferred. All three additional batches
are now consumed. This note supersedes starting-state instructions in
[[rdp-foundation-repair-results-2026-09-20]] while retaining all prior evidence.
All five selected iterations remain blocked; no new PR or merge occurred.

Evidence and exact state:
`/Users/james/devel/ff-rdp/.git/ralph-loop/20260920-another-spin/`.
Read `state.json`, `inventory-reconciliation.json`, `final-verification.json`
and `final-lock-release.json` before any resume. Previous archives are immutable.
The final lock receipt, not the presence of this note alone, proves release.
There is no background continuation. Raw private evidence stays private.

## 262 — offline defect fixed, new caller defect rejected

A fresh worker selectively restored the reviewed569-input candidate and changed
only `commands/type_text.rs` relative to that candidate. Offline regression
proved that an unguarded evaluation against a destroyed console could ignore
its lifecycle event and wait for the full three-second submission grace period.
The new scoped target guard made the failing regression pass; all10 typing and
four253 unit tests passed. This was a plausible explanation for the previous
4.248194792-second live failure, not established historical attribution.

Independent review **rejected** the delta: the guard also reacts to navigation
start, which precedes replacement-target readiness. A single best-effort snapshot
can still return the outgoing target or leave it installed on Pending. Subsequent
`--settle` and `--wait-for` then evaluate that console without waiting/retrying for
handover; later `--with-page` recovery cannot repair an earlier failure.
Tests stop at `navigated_away` and omit this actual caller sequence. Review also
noted that late-ack safety was not demonstrated; console acknowledgment abandonment
still handles only Timeout. See `iter262/review.md` and `review.phase.json`.

Normal ordered stable-update, fmt, strict workspace/all-target Clippy and default
parallel workspace tests passed:2516passed/0failed/419ignored. This is new valid
candidate evidence, but does not erase the prior two parallel missing-receipt
failures or establish plan278's cause. Plan278 remains outside execution scope.
No live253 rerun, causal capture or sweep was allowed after rejection. Original
262AC2/5 and all required remaining validation stay unmet. Prior proof4/5 remains
preserved; no new claim extends it to this rejected caller change.

`iter262/source-files.json` SHA256
`e27f362845a1adf001383c8f8c95570b2baa86eb5bb8f9b51e556972ca670afc`
and `post-source/` retain all569 candidate inputs. Before-test compile failure
from stale Cargo mtimes, genuine pre-fix regression failure, passing after-tests,
ordered gates and restoration receipts are all retained. Guarded source restore
returned the branch to clean pushed `8f72ef38e4b09407c029d87b87b37eb5824ac59d`;
the restored product was rebuilt. No rejected product source was committed.

## 268 — both diagnostic-tooling defects repaired and reviewed

This batch changed only private diagnostic artifacts. The checker now requires
the mandatory shutdown swap pair after worker outcomes, matching operation,
thread and outcome reason. A preserved77-record synthetic deletion passes the
old checker and is rejected by the new checker. It is not a real failure capture.
The watchdog isolates observer callbacks and uses one aggregate cleanup deadline;
pending observations and unverified identities fail closed.

Independent review approved this **artifact-only blocked checkpoint**, with zero
new actionable findings, verifying26 artifact and10 inherited-input hashes.
Eighteen offline methods pass. The intermediate PermissionError/cleanup race and
subsequent correction remain in the archive. Review did not execute tests.
See `iter268/review-packet.md`, `review.md`, `artifact-hashes.json`,
`first-bad-control.json`, raw trace and control receipts.

The complete worker-return/shutdown attribution prerequisite is still missing:
product worker JoinHandles are dropped and the daemon can exit during the
one-second drainer sleep. External death establishes interruption, not worker
return. All three capture wrappers retain exit78; the compiled control remains
ignored with a fixed-false pre-child guard. No real mock control or Firefox
capture ran. Original tasks/AC0/4 remain. The product checkout stayed untouched
and clean at pushed `27604336485c5fd458b8f4d88ed40cbc56190ce4`.

## 271 — two runner defects repaired, terminal publication still blocked

The additional batch restored only the three unchanged diagnostic Rust files
and changed private runner helpers. Pre-open deadline checks and repeating alarms
address the consecutive-blocking-I/O path. A double-FIFO control returned fatal70
in0.152536seconds against a nominal0.15-second budget: scoped evidence, not a
hard real-time guarantee. Command-receipt write failure now enters cleanup through
`finally`; a Python mock in a separate process group exited-15, listeners were
checked and its private home retained. Its later identity-change refusal is fatal
evidence, not a claim of completely clean cleanup.

Staged writing handles recorded pre-publication failures, but a new precise
counterexample remains: **alarm after rename side effect produces actual process
exit70 while published `pair-end.json` says exit0**. The counterexample test's own
exit0 means it reproduced the defect. Independent review confirmed the mismatch
at `run-pair.py:355–358`, approved the blocked checkpoint/restoration, and rejected
capture. No distinct additional findings were returned. See
`iter271/publication-window.506cxz1s/raw.log` and `review.md`.

Freeze `647ed5e0b3db6709a2920cccd9809f817528f918f6aaf20dff4b2462416998a5`
pins700 inputs. The review verified current HEAD, executable copies, package
inputs and equality of the three diagnostic source files with prior repair2.
Strict Clippy/build, six compiled Firefox-free checks, actual BBC JavaScript,
five shared controls,11 prior runner controls and six new controls passed.
The first build wrapper's reserved zsh `status` error and first Python launcher
identity-transition control failure are retained; corrected receipts are separate.
There was no full workspace test or CI claim for these diagnostics.

Strict native-BBC accepted/actual native absence-or-zero-size criteria and the
limits of passive observations remain unchanged. No old six passing controls
were repeated as captures. No prospective pair ran:0/2 captures,370seconds intact.
The archived `run-capture.sh pair` command is **not approved; do not execute it**.
Original tasks/AC0/4 remain unmet. After review, the guarded restoration validated
all567 baseline inputs and restored only three files, then the product rebuild
passed. Source is clean at pushed `10312d2fecb5f905f0a23df795d69dd31a0a30af`,
preserving partial271 and foundation ancestry. No homes or historical data were
deleted as part of source restoration.

## Allowance ledger and remaining entry conditions

| Iteration | This spin charged preparation | Prior3600s grant total used | Numerical remainder | Additional batches this spin |
| --- | --- | --- | --- | --- |
| 262 | 627s:382worker+113review+132supervisor | 3515s | 85s | 1/1 |
| 268 | 476s:320worker+56review+100supervisor | 3534s | 66s | 1/1 |
| 271 | 914s:553worker+150review+211supervisor | 3503s | 97s | 1/1 |

Supervisor debits and271's full allocated150-second review charge are conservative,
not measured attention time. The271 reviewer omitted a clock interval; actual
review duration remains unavailable.268's worker took319.365seconds, exceeding
its assigned300-second sub-bound by19.365seconds; that overrun is retained and
rounded up to320. The review allocation was reduced to keep total268 below542
retained seconds. No compile time was subtracted from worker charges. Ordered
acceptance gates, source restoration/rebuild and final handoff administration
are recorded separately under the existing grant's validation separation.
Actual token totals and a comparable supervision baseline remain unavailable.
No claimed token saving or halving result is supported.

Discovery/capture allowances did not change:262 retains282seconds and one slot
(total5/6 used);268 retains302seconds and six slots;271 retains370seconds and six
historical slots, with at most the unopened two-arm pair. Numerical remaining
time does not grant another repair batch. Further changes require further repair
authority and satisfaction of the same specific prerequisites; nothing resets
on a new session.

259 remains at `cc44e873e977c979729372010ba65fcf088341b9` with its recorded execution
restriction; no retry, rephrasing, alternative-tool probe or execution agent was
launched.266 still depends on adequate independently reviewed259 reply-ownership
semantics on main. Remote main remains the prior verified PR263 merge
`8666a5324905793538c59532e909e0808beee7f3`; no new merge occurred this spin.

The full pending inventory is unchanged:147,203,259,262,266,268,271,
272,273,274,275,276,277,278.147/203 are parked,272–278 unexecuted. Branch-local
plans, original criteria and their previous pushed checkpoints were preserved.
This spin's new evidence is a local archive plus this uncommitted handoff, not a
new pushed code checkpoint. The seven preexisting dirty/untracked primary files
were copied byte-for-byte before work; previous handoffs remain unchanged,
feedback additions are append-only and the older AGENTS body is preserved.

All workers are stopped. Final cleanup must confirm no owned runtime or port
listener, retained desktop Firefox57827 identity, clean execution checkouts,
matching remote refs and release of only this run's lock. Those receipts are
written after this handoff. Weekly usage last read32% at18:47:42UTC; final state
contains the last fresh reading. The stop reason is unresolved required findings
and consumed repair batches, not the25% weekly threshold. Preserve all evidence
and do not continue in the background.
