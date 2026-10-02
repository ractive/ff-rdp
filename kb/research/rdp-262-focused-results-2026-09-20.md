---
title: "Focused 262 execution results — 2026-09-20"
date: 2026-09-20
type: research
---

# Focused 262 — reviewed caller repair, blocked closing validation

The owner explicitly launched [[rdp-262-focused-next-session-2026-09-20]] with a
separate 90-minute preparation/implementation/review grant and at most two
independently reviewed implementation/repair batches. Both batches were used.
This note supersedes that proposal's unadopted starting state and the 262 starting
state in [[rdp-foundation-another-spin-2026-09-20]]. Other iterations were not executed.

Run evidence is private under
`/Users/james/devel/ff-rdp/.git/ralph-loop/20260920-focused262/`.
Read `state.json`, `sweep-1/reconciliation.json`, `blocked-checkpoint/restore-recipe.md`
and the final verification/lock-release receipts before any resume. No further
implementation or sweep is authorized by a numerical time remainder alone.

## What was repaired and reviewed

One fresh implementer retained context through both batches. Batch 1 made the
actual submission caller wait for positive document handover before `--settle`,
`--wait-for` or `--with-page`, retaining one submission deadline and private target
installation. It also retired interrupted console acknowledgments so a late old
result cannot become the next evaluation's result. Both defects had failing-before
and passing-after offline regressions. Review 1 rejected an outgoing-target race
during the subsequent `listFrames` metadata request.

Batch 2 guarded the sampled target during metadata acquisition, retried only typed
lifecycle loss within the same deadline, and retired an abandoned metadata reply.
Endpoint/authentication and other protocol errors remain errors. Its before tests
reproduced both the `noSuchActor` abort and a wrong late URL; after tests cover
21 actual-caller scenarios, same-document URL changes, deadline exhaustion,
interrupted actions and replies, and a non-lifecycle error that must not retry.
Fresh review 2 resolved R1 and returned zero new actionable findings.

Final ordered stable/fmt/strict workspace Clippy/default-parallel workspace checks
passed: **2,519 passed, 0 failed, 419 ignored**. Earlier failures and the first
review remain preserved. Historical proof 4/5 is reusable only for the unchanged
prevention contract: no eligible shared legacy `getTarget`. Function-byte reuse
for the modified metadata acquisition is explicitly invalid; neither review
claims that old proof measured the final binary's new retry behavior.

## New live evidence and blocker

The original live253 suite passed **8/8** with unchanged assertions. Committed
submission measured **1,083 ms daemon / 738 ms direct**, against the original
less-than-two-second bound. The historical 4.248194792-second failure remains
retained; its cause was not retroactively established. The mandatory 137 consent,
both 145 cases and watcher-source tests then each passed targeted validation.

One complete dual-gate closing sweep ran on the frozen candidate. All six tiers
and 347 unique names reconcile: **346 passed / 1 failed**.

```text
LIVE_SWEEP_SUMMARY executed=347 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=347
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

The sole failure was
`live_262_target_snapshot::live_262_watched_target_prevention_contract`, at its
supported `eval 1` daemon-autostart setup, before navigation or the prevention
assertion. The product exited 124 with `Timeout: waiting for watched tab target`;
the outer command watchdog did not time out. The retained daemon trace contains
one top-level `about:blank` availability (inner-window 21), its destruction, and
no replacement before failure. There were **zero shared `getTarget` sends and
zero `listFrames` sends**. This observation does not establish the cause or prove
the historical legacy-suppression mechanism recurred.

Required 137, both 145 and watcher-source rows passed inside this sweep, but it
does not supply a clean closing sweep. No second sweep, isolation retry, new
capture or third repair was started. The three-consecutive-sweep requirement
remains unmet. Original acceptance text and checkbox state remain AC 2/5; the
passing candidate gates and live253 result are recorded without claiming delivery.
No code commit, PR or merge occurred.

## Recovery and accounting

The exact 569 candidate source inputs, four actor documents, full patch and
CLI/live-test/xtask binaries are archived and hashed under `batch2/` and
`blocked-checkpoint/`. Run-owned source was restored to the clean foundation
checkpoint and rebuilt; restoration is not a third repair batch. The pushed,
clean documentation checkpoint is `043cb8e3f8932e53a14259459b14f582c54c34f5`;
`checkpoint.json` records remote verification. Main remains
`8666a5324905793538c59532e909e0808beee7f3`.

Preparation through review approval was conservatively charged **2,209 seconds**
of the new 5,400-second grant, without deducting compilation or idle time.
The numerical remainder is 3,191 seconds, with **no implementation batches left**.
Final acceptance validation and preservation are recorded separately. Reviews
measured 169.5 and 242.2 seconds. Actual token totals and a comparable supervision
baseline are unavailable; no token-saving or overhead-halving claim is made.
Weekly usage was 32% at launch and 29% at the last validation reading; the final
receipt records the last fresh check. The 25% threshold did not cause this stop.

Historical preparation ledgers remain unchanged, including the old 85-second
remainder. Discovery remains **282 seconds and one unused slot, five of six used**.
No session, sweep or restoration reset that allowance. Future eligible work must
explain the new setup loss without assuming contention or raising deadlines,
preserve the reviewed caller correction, and respect the exhausted repair limit.

All other plans retain their entry conditions: 259's execution restriction and
266's dependency are binding; 147/203 are parked; 268/271 and 272–278 were not
executed. The pending union remains 14 plans. All preexisting primary handoff and
feedback bytes were preserved, with the new AGENTS checkpoint prepended to its
existing body. Runtime cleanup, source recovery, remote refs and lock release
must be verified from the final receipts, not inferred from this note alone.
