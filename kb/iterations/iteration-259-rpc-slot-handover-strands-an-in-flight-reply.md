---
title: "Iteration 259: an RPC-slot handover can deliver one client's in-flight Firefox reply to the next client"
type: iteration
date: 2026-09-07
status: done
branch: iter-259/rpc-slot-handover-strands-an-in-flight-reply
depends_on:
  - iteration-240-daemon-frame-desync-root-cause
first_call_sites: []
dogfood_path: |
  Historical slow-eval example below is retained as a delivery probe only: evaluateJSAsync resultID matching means it cannot alone prove that the successor accepted another client’s answer. Use the current-main execution clarification for the attributable ordinary-reply case. Existing60-hop and20-eval obligations remain unchanged.
  # The window, stated precisely. The daemon serialises Firefox-bound requests
  # through a single RPC slot because RDP replies carry no per-request
  # correlation id (crates/ff-rdp-cli/src/daemon/server.rs, `rpc_writer`).
  # Iteration 240 made `ClientCleanupGuard::drop` the slot's ONLY release
  # point, so it can no longer be reassigned while its owner still believes it
  # holds it. What it did not close: the owner's handler thread can return
  # while a request it forwarded is still unanswered.
  #
  # 1. Reproduce the abandonment. Two terminals against ONE daemon:
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  #    terminal A — start a slow Firefox-bound request and kill it mid-flight:
  ff-rdp eval 'new Promise(r => setTimeout(() => r(1), 8000))' &
  sleep 1; kill %1
  #    terminal B — immediately claim the slot and ask something with a
  #    distinctive answer:
  ff-rdp eval '"B-OWNS-THE-ANSWER"'
  #    OBSERVED (to be recorded on the branch): whether B's reply is its own or
  #    A's stranded one. `--log-level trace` on the daemon shows the forward.
  #
  # 2. The same window without a kill: any client that exits between sending a
  #    request and reading its reply. `click --ref … --with-page` on a page
  #    that navigates was the iteration-224 shape of this.
  #
  # 3. After the fix, the loop that must stay green (it is the iteration-240
  #    dogfood, and it must not regress):
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo run -p xtask -- check-dogfood-script \
    kb/iterations/iteration-240-daemon-frame-desync-root-cause.md
  #    expected: 60 hops, 0 reconnects, 0 abandoned clients, dispatcher healthy.
  #
  # 4. And the sequential common case must not get slower. Every `ff-rdp <cmd>`
  #    is a fresh connection that claims and releases the slot, so any
  #    quiesce/drain barrier pays its cost on EVERY invocation:
  time (for i in $(seq 1 20); do ff-rdp eval '1+1' >/dev/null; done)
  #    Record the before/after. A fix that adds latency here is worse than the
  #    defect it closes.
tags: [iteration, daemon, rpc-slot, correctness, carry-over, ff-rdp-cli]
---
# Iteration 259: an RPC-slot handover can deliver one client's in-flight Firefox reply to the next client

> Filed 2026-09-07 as carry-over from the code review of
> [[iteration-240-daemon-frame-desync-root-cause]] (PR #243, review finding 1). That review found
> the daemon *reassigning* the slot out from under a live owner; iteration 240 fixed that by making
> the owner's cleanup guard the slot's only release point. This plan is the residual the fix does
> **not** cover, and which predates iteration 240 on `main`.

## The defect

The RPC slot exists because Firefox RDP replies carry no per-request correlation id: at most one
client may have Firefox-bound requests in flight, or a reply cannot be attributed. The slot is
released when the owning client's handler thread returns.

Nothing ties "the handler returned" to "Firefox has answered everything this client asked". A
client that exits between sending a request and reading its reply — killed, timed out, or simply
finished early — releases the slot with a reply still on its way from Firefox. The next claimant
takes the slot and `forward_to_rpc_client` hands it that reply. It has no way to tell that the
answer is not its own.

This is a wrong-answer bug, not a dropped-frame bug, which is what makes it worth a plan of its
own rather than a footnote.

## Themes

- **A — Measure it before designing for it.** The window may be narrow enough in practice that no
  realistic client hits it, or wide enough that the `--with-page` resets iteration 224 chased were
  partly this. Establish which with a reproduction before touching the slot, and record the answer
  either way. A "cannot reproduce" outcome is a legitimate close for this plan.
- **B — What correlation is actually available.** RDP replies carry `from` (the actor) but no
  request id. Establish from the Firefox source whether *anything* on a reply can be matched to the
  request that produced it — per-actor request ordering is the most likely candidate, since an
  actor answers its requests in order. If per-actor FIFO holds, the daemon could track outstanding
  requests per actor for the slot owner and discard, rather than forward, replies from actors the
  new owner never wrote to.
- **C — Do not pay for it on the common path.** Every `ff-rdp <command>` is a fresh connection that
  claims and releases the slot, so the sequential case is the hot path. A fix that makes the next
  claimant wait for a quiesce period would tax every invocation to close a rare window. Whatever is
  chosen must be free when the departing client had nothing outstanding — which is the normal
  ending, and is knowable.
- **D — Say so when it cannot be closed.** If B establishes that no attribution is possible, the
  honest outcome is a documented limitation plus a *detectable* one: the daemon can at least count
  slot releases that happened with requests outstanding and report it in `daemon status`, so a
  confusing result is diagnosable after the fact instead of invisible.

## Tasks

### A. Establish the window [3/3]
- [x] Reproduce a stranded reply reaching a second client, or establish that it cannot be
      reproduced and say what was tried
- [x] Record how often a slot release happens with a request outstanding, over the iteration-240
      60-hop dogfood loop
- [x] Read the Firefox source for any reply-to-request correlation beyond `from` (Theme B)

### B. The fix or the documented limitation [2/2]
- [x] Either attribute (or discard) replies belonging to a departed owner, or record the
      limitation with the reasoning that closed the design space
- [x] Report slot releases-with-requests-outstanding in `daemon status` (Theme D) — this is worth
      doing whichever way B lands

### C. Proof [2/2]
- [x] A test that fails on the pre-fix daemon: a client that departs mid-request must not have its
      reply delivered to the next claimant
- [x] The iteration-240 dogfood loop still reports 60 hops / 0 reconnects / 0 abandoned clients,
      and 20 sequential `eval` invocations are no slower than before (Theme C)

## Acceptance Criteria [4/4]

- [x] The window is either closed or documented, with the measurement in the Outcome either way
- [x] No regression in `daemon status` answerability or in the sequential invocation path
- [x] `daemon status` names the condition when a slot is released with work outstanding
- [x] Every claim in this plan's Outcome is backed by a run recorded on the branch

## Out of scope

- The desync and wedge that iteration 240 fixed. Both are closed; this is the part it left open.
- Adding a correlation id to Firefox RDP. Not ours to add.

## References

- `crates/ff-rdp-cli/src/daemon/server.rs` — `rpc_writer`, `claim_rpc_slot_queued`,
  `forward_to_rpc_client`, `ClientCleanupGuard`
- `crates/ff-rdp-cli/src/daemon/client_writer.rs` — the one writer per client socket
- [[iteration-240-daemon-frame-desync-root-cause]] — the resumable framer, the single client
  writer, and the owner-only slot release
- [[iteration-137-daemon-rpc-queue]] — where contended clients started queueing for the slot
- [[iteration-224-say-goodbye-before-closing-a-client]] — the unattributable client resets that
  may share this cause

## Implementation preflight — 2026-09-19

The current-owner forwarding gap remains an investigation target, not a demonstrated wrong-answer reproduction. Slow eval is not a generic uncorrelated response: ConsoleActor captures resultID and matches evaluationResult. Distinguish delivery to another client from that client accepting the result; use an applicable deterministic reply scenario. Reconcile the plan's permitted unsuccessful-reproduction outcome with its original regression/instrumentation criteria honestly, leaving unmet criteria unticked. Resolve reply-attribution implications before iteration 266 changes shared-connection releases. The references iteration-137-daemon-rpc-queue and iteration-224-say-goodbye-before-closing-a-client are unresolved and need their actual targets established.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Restart plan — 2026-09-19

Follow [[ralph-loop-open-iterations-2026-09-19]]. The preserved checkpoint is
`cc44e873e977c979729372010ba65fcf088341b9` on this plan's branch and worktree
`/Users/james/.cache/ff-rdp/queue-20260919-259`. It contains documentation only:
execution blocker, corrected historical links and266's dependency. No attributable
wrong-answer reproduction or product repair exists; all original ACs remain unmet.

**Entry gate.** An automated content filter rejected the prior protocol
investigation as possible cybersecurity risk. A fresh session, different model,
renamed task or equivalent alternate tool is not evidence that this restriction
has cleared. Inspect the recorded blocker and current applicable access/policy
state without reissuing the rejected action as a capability probe. Proceed only
through an actually available permitted path; record why it is permitted. If no
such path is established, retain259blocked and skip266, continuing independent
262/268/271. Do not reinterpret this plan as approval to bypass a denial.

**Conditional implementation plan, only after the entry gate clears (Astra).**

1. Verify the current owner-only RPC-slot release and shared writer from240, plus
   the now-merged258 absolute deadlines/abandoned-ACK handling and267 auth fix.
   Determine which layer owns each obligation; do not overwrite those fixes.
   Update the historical slow-eval dogfood qualification before relying on it:
   ConsoleActor uses a resultID, so arrival at a new client does not itself prove
   that client accepted the wrong result.
2. Establish a bounded, owned two-client lifecycle experiment for a reply type
   whose actual correlation contract is verified in the running Firefox revision.
   Record request/actor identity, departure, slot ownership transition, reply
   arrival/forwarding and whether the recipient accepts/rejects the answer. Use
   only local owned fixtures/connections. A deterministic transport-level test
   may isolate ordering, but it cannot replace the required real-Firefox evidence.
   After verifying that reply contract and before execution, record both a finite
   attempt ceiling and wall-time budget, with a maximum of20 paired trials or
   30minutes for this first reproduction block, whichever comes first. Stop early
   on a conclusive attributed result. On exhaustion, preserve exact scenarios,
   counts and negative evidence with all unmet ACs; do not extend or restart the
   block unchanged. These discovery limits do not replace the separate required
   60-hop dogfood/20-eval measurements or clear the execution entry gate.
3. Measure outstanding work at release during the existing24060-hop dogfood,
   and record the original20-sequential-eval before/after comparison. Define
   matching build/environment and wall-time measurement before collecting data;
   avoid a new unrequested benchmark suite. A no-reproduction result must list
   the actual scenarios and counts; it does not automatically satisfy the status
   instrumentation or regression criteria.
4. After attribution, choose the narrow ownership/correlation fix, or an honest
   detectable limitation if no safe correlation exists. Do not impose a universal
   quiescence delay on the empty-outstanding common path. Define the observable
   outstanding-work counter and its lifecycle in `daemon status`. Distinguish
   ordinary replies, async result-ID traffic, events and one-way methods from the
   published Firefox contracts; do not assume all methods ACK or share FIFO rules.
5. Add the meaningful regression required by the original plan and validate
   status answerability/common-path latency,60hops/0reconnects/0abandoned clients,
   own dual-gate sweep and ordered gates. Independent Astra review must resolve
   concurrency and reply-attribution findings before publication.

The original plan permits a no-reproduction/documented-limitation outcome, while
still requiring observable status and evidence. Reconcile each original box
explicitly; do not mark the iteration complete just because no failure appeared.
Two repair batches remain the normal new-run ceiling; preserve prior attempts
and do not reset unresolved findings by changing sessions.

## Execution checkpoint — 2026-09-19

The selected investigation was stopped by an automated content filter that
reported possible cybersecurity risk before any attributable reproduction or
product implementation was completed. This is an execution blocker, not evidence
that the proposed reply-ownership diagnosis is correct. No workaround, rephrased
retry, or delegated repetition of the rejected action was attempted.

The clean product baseline was `010059c632da4ce1344b0516a05a7c911b4cfe15`.
Owned raw Firefox PID70596 and its wrapper PID70579 were stopped; port6000 was
verified free and desktop Firefox PID1112 was preserved. Exact evidence is in
`.git/ralph-loop/20260919-queue/iter259/execution-blocker.md` and the retained
source-fetch/build/cleanup artifacts. No acceptance criterion is fulfilled by
this attempt. Resume requires an appropriate permitted execution path; iteration266
remains blocked on this plan's unresolved reply attribution.

The batch-end inventory audit corrected two stale historical wikilink names only.
No product investigation was resumed by that bookkeeping change.

## Current-main adaptation — 2026-09-25
The current-owner forwarding gap remains present after 282/279. Establish a same-actor ordinary-reply case that records both delivery and the successor's accepted answer; a late asynchronous evaluation event alone is insufficient. Preserve the finite reproduction ceiling and collect the original 60-hop outstanding-work and 20-eval before/after measurements on the same current-main baseline.

Define connection-scoped ownership before sending, including ordinary replies, asynchronous ACK/result traffic, declared one-way operations and daemon-owned release replies. Preserve the startup-recovery reply sink and the no-wait empty-outstanding common path. Unknown attribution must have an explicit safe disposition and observable status. A negative reproduction does not establish that safe attribution is impossible or discharge instrumentation.

The all-open owner grant does not override the historical automated execution-filter denial. Preserve `.git/ralph-loop/20260924-all-open/259-historical-filter-boundary.json`; do not reissue, disguise or substitute equivalent rejected operations to probe it. The historical record is an unspecified agent-filter failure, not evidence of a blanket prohibition on local correctness work. A materially safer local deterministic unit-work path may be assessed separately on its actual content; this documentation adaptation neither executes it nor establishes clearance for a previously rejected operation. All original tasks and ACs remain unchanged.

## Historical outcome checkpoint — 2026-09-28 (acceptance then incomplete)

The current candidate closes wrong-owner reply delivery by recording the response
contract before a Firefox write and attributing each completion under the origin
ledger lock. Actual client departure quarantines unfinished actors without
forgiving ordinary replies, async acknowledgements or result-ID obligations.
A successor may use distinct actors. Reusing a quarantined actor, an ambiguous
error, an uncertain write or accounting-capacity exhaustion retires the origin
before unsafe delivery. Valid orphan completions are discarded. Typed events do
not cancel debt. Empty handover adds no wait. Status retains aggregate outstanding
counts and distinguishes active/orphan work and departures with outstanding work.
Deferred releases remain bound to their originating connection; this is not
iteration266's broader resource-lifetime implementation.

A controlled native baseline on main `d2a17038`, with Firefox156.0.1, demonstrated
B's production `RootActor` accepting A's held ordinary `listTabs` answer. Five
actual fixture workers returned and joined. That result establishes the controlled
window, not its natural frequency or the cause of any historical EOF/reset.
The baseline's1204 normal/1205 overlay source inputs, frozen artifacts and110
Firefox package files were reverified unchanged. The former candidate's universal
retirement result remains historical. The separately qualified process-descriptor
pair below supplies native evidence for the quarantine generation; it is not a
relabeling of this title/tab-based scenario.

The separate in-flight metadata proof established old `listFrames` before
navigation, a local interrupted return, useful replacement metadata and old debt
at actual client departure. The orphan-accounting generation passed the corresponding cross-layer
regression: all9 fixture workers returned/joined, the old completion was discarded,
and B parsed its own distinct-actor answer without an old packet in its event
side channel. The same test on the prior source failed exactly at the expected
quarantine assertion (exit101), with all7 acquired workers returned/joined and
no cleanup failures. Both occurrences conserved the450-profile inventory. No
candidate repetition was needed. All32 focused controls passed once; their
fixtures recorded31 actual supervised worker joins in total. Fresh independent
source review returned explicit ACCEPT with zero actionable findings, and strict
workspace Clippy passed for that generation. These focused results do not replace
validation of subsequent source changes or closing validation.

The earlier preannounced-navigation caller repair remains: metadata already known
to belong to the outgoing document is not requested as a fresh snapshot. Its
focused/mutation evidence is retained. It cannot prevent an unreceived navigation
event racing an already-sent request; quarantine addresses that departure boundary.
No deadline, lifecycle event, retry allowance or assertion was weakened.

Earlier original20-eval and60-hop measurements passed before the accounting repair:
4,343,623,500ns baseline versus4,269,403,459ns candidate, and60pairs/120daemon
responses with0reconnects/0failed hops. Those results remain historical.

The orphan-accounting generation's one newly scheduled20-eval pair completed
all20 correct daemon results in each arm, but FAILED the unchanged no-slower
criterion: baseline4,241,698,416ns, candidate4,270,449,958ns (+28,751,542ns).
The54 recorded commands all had actual exit0 waits and both owned sessions finished
successfully. This one comparison does not attribute the difference to accounting,
startup or scheduling; neither the first sample nor the failed result is discarded.
The separately scheduled original60-hop scenario PASSED60pairs/120daemon routes,
0reconnects,0failed hops and final cumulative departures/pending0 in its verified
fresh session. All126 internal commands had actual exit0 waits and the owned
session finished successfully. Both scenarios conserved450 preexisting profiles.

The new native candidate occurrence FAILED before its A/B ownership phase: the
first direct production listTabs returned a parsed tab count different from one.
The fixed error omitted the count, so zero versus multiple remains unknown. Only
the two acquired relay workers ran; both returned/joined and owned Firefox cleanup
succeeded. The logged receive EOF belongs to the local fixture relay during
teardown; it is not evidence of native Firefox death. The source audit found that
port-ready launch does not establish the fixture's one-tab precondition. The
proposed private correction explicitly configures six startup-page preferences
and records counts and cleanup ordering from existing operations, without an extra
request, warmup, wait, retry or lifecycle transition. Any changed setup must be
reviewed and independently proved; the historical baseline must not be relabeled
as a matched six-preference run.

A narrow product follow-up has passed ordered non-live gates: when total pending work is
zero, return immediately from active-work/quarantine bookkeeping instead of
scanning collections. It preserves hazard, terminal and orphan records. Source
inspection justifies avoiding these scans, but does not prove that they caused
the measured29ms difference or that the optimization will remove it. Ordered formatting, strict workspace Clippy and workspace tests passed on the
new source:2,738 parent tests plus1 nested test,0failures/426ignored. All32 earlier
focused names passed within that workspace run; they were not separately repeated.
The450 existing profiles were conserved and4 attributable workspace fixtures were
retained. Independent scoped review accepted this delta and the startup fixture
correction. The current generation then completed all20 correct daemon results
in both measurement arms, but again FAILED the unchanged no-slower requirement:
baseline4,161,411,583ns, candidate4,181,974,375ns (+20,562,792ns). All54 recorded
commands actually exited0. No cause or recovered performance is established.
The separately admitted current60-hop scenario PASSED60pairs,0reconnects and
0failed hops, with126 actual exit0 command waits. These are separate occurrences
from the historical generation above; each conserved454 existing profiles.

The six-preference native setup did produce exactly one tab, but its subsequent
A reply lacked the assigned title marker and failed before A departure or B
acquisition. The three acquired workers actually returned/joined; B never existed.
Cleanup succeeded, conserved454 profiles and left no owned process groups.
This is a separate failed precondition from the older non-one-tab occurrence;
it establishes no ownership regression. Its full raw reply, failure and cleanup
ordering remain preserved. A later diagnostic confirmed the DOM setter's marker
readback while the parent form still reported the old title; its cause remains
unknown, and that title experiment was stopped. The different process-identity
fixture below answered the ownership question. Common-path timing, validation of
subsequent product changes, the closing dual-gate sweep, final documentation/static
gates, exact-head CI and merge remain pending. Original criteria remain unchanged
and acceptance remains0/4.

The first closing sweep failed349/351, with two watcher159 assertions. Both
subsequent one-off diagnostic scenarios passed on their original bounds and were
stopped after answering their capture questions. Neither passing capture took
the controlled in-flight interruption window. The old sweep's two causes, and the
older untraced hop17 failure, remain unproved; no claim attributes them to262,
267 or the controlled mechanism. All failed results and previous ledgers remain
preserved. The historical unspecified filter-denial boundary and original
reproduction ceiling remain intact; no rejected operation was retried or relabeled.

### Qualified native process pair and accept-readiness follow-up

On source68, a fresh matched clean-main/current native pair used stable parent-owned
process metadata with real `getProcess` ordinary requests, rather than DOM titles.
One setup `listProcesses` snapshot selected parent0 and an actual distinct nonparent
ID on the same upstream connection. Actual A and B reply descriptors matched their
snapshot IDs/actors/isParent values before the baseline released unmodified A then B.
Production B `RootActor::get_process` accepted A's descriptor actor. In the candidate,
A actually returned/joined; pending ordinary1/orphan1/active0 remained quarantined
without immediate retirement. B's same-root request was refused before a Firefox
write and the origin retired with old debt retained. All ten fixture bodies returned
successfully and joined (five per arm); both occurrences conserved454 profiles.
This proves the controlled ordinary-reply mechanism, not natural frequency, process
liveness, a historical EOF cause or the cause of the failed closing tests. The
baseline's reviewed notification eligibility differs from the stricter candidate;
actual post-notification descriptor validation, not a liveness assumption, qualified
its replies. The experiment is answered and is not allocated another occurrence.

A separate scalar-only capture on the instrumented source68 overlay found the
measured main and snapshot TCP completions inside the daemon's existing100ms
accept-wait intervals. TCP-to-accept observations were90.660ms and44.150ms,
respectively. Complete observation-prefix checks and actual command/outer waits
qualified the capture;454 profiles were conserved. Clock reads and logging perturb
timing. This pre-existing wait does not establish the cause of the failed original20
+20,562,792ns comparison or isolate reply-accounting cost; that requirement remains
failed on source68.

The current source-only follow-up uses registered Mio listener readiness for accepts
and cancellation, retaining the100ms signal/idle housekeeping ceiling. Accepts use
that same Mio handle so Windows WouldBlock re-registration is not bypassed. Existing
handler ownership, idle activity rules and all reply-accounting/retirement behavior
remain unchanged. Focused controls assert actual listener/cancellation event tokens,
queued connections after WouldBlock, registration-race handling and empty-queue
housekeeping, without sub-100ms timing assertions. The same batch adds exact
case-insensitive `auth` key redaction and a synthetic64-byte/nested-key regression;
explicit raw mode remains intentional and unchanged. No real credential capture is
needed. Independent review accepted the formatted readiness/auth repair with zero
findings. Ordered formatter, strict workspace/all-target Clippy and workspace tests
passed:2,744 parent tests plus1 nested test,0failures/426ignored. All47 named focused
controls passed within that workspace run; they were not separately repeated.
The454 existing profiles were conserved and4 attributable workspace fixtures retained.
A fresh normal and measurement-tool producer qualified, with21 actual command waits
and actual outer exit0. The normal CLI SHA256 is
`39a47dc48389bbd25376b573cf48bb67ad7981646132d0f51494447d89d81136`;
its formatted1213-input manifest is
`73fdcae5ebdd584ad64154cbeb39e81222b672355c1698aa3daa59ff1d4b5e58`.
The earlier native fixture bypasses the accept loop: its unchanged direct-handler
ownership mechanism is relevant evidence, not full validation of the new accept loop
or new binaries. No native/scalar occurrence was repeated for this follow-up.

On this reviewed source, one fresh original20 comparison PASSED the unchanged raw
no-slower criterion: baseline4,177,893,042ns, candidate1,095,637,584ns,
candidate-minus-baseline−3,082,255,458ns. Both arms completed20 correct daemon
`eval 1+1` results with matched compiler/debug/package inputs, the same six startup
preferences and the predeclared single unmeasured setup eval. All54 internal commands
had actual successful waits. This is one declared comparison, without tolerance,
retry-until-winner or a statistical speedup claim; earlier failed comparisons remain.

The separately admitted original240 dogfood PASSED60navigate/click pairs,
120daemon routes,0reconnects/failed hops/abandoned clients and final pending work0.
All126 internal commands actually returned0. Initial private registry absence,
verified zero initialization, first post-navigation full status and unchanged session
identity establish final cumulative departures-with-outstanding0 for the fresh
exclusive run. The pre-start counter was unavailable; no observed full-run
before/after delta is claimed. Status remained answerable. Both measurement occurrences
conserved all458 profiles, protected/real state and verified owned cleanup, with
actual outer waits and no survivors. Their questions are answered; no repeat is implied.

The second required dual-gate closing sweep PASSED351/351 across all six exact
tiers (340+1+3+3+2+2), with0skipped/preexisting/vanished/launch-timeout/timed-out
cases,0profile leaks and0unattributed profiles. All343 launches were paired across
17ledgers. It conserved the458 baseline profiles and retained10 attributable
fixtures (468 total), with protected/real state unchanged and no owned survivors.
The sweep and its actual outer both exited0; the owned raw browser's actual wait
was−15 after owned TERM, which is cleanup evidence, not a worker-return claim.
Original20/60 and required closing results now support the final criterion map in
[[rdp-259-reply-ownership-validation-2026-09-28]]. The supervisor has approved original acceptance4/4 from the criterion map,
qualified native/regression/status evidence, original20/60, closing and static
results. The plan is marked done under the project convention; exact-head CI,
branch checkpoint/publication and GitHub merge remain separate and are not claimed. The passing sweep does not retrospectively explain closing1's two watcher159
failures or the untraced hop17 occurrence.

Current private verification records (recorded facts, not new admission):

- `iter259/accept-readiness-gates1/gates/root-verification.json`: ordered counts and47 exact controls.
- `iter259/review-accept-readiness1/report.md`: accepted readiness/auth repair.
- `iter259/review-readiness-measurements1/report.md`: accepted measurement tooling.
- `iter259/accept-readiness-root-admission1/eval20-verification.json`: current raw no-slower result and54 waits.
- `iter259/accept-readiness-root-admission1/dogfood60-verification.json`: original60 result,126 waits and cumulative counter semantics.
- `iter259/review-runtime-evidence4/report.md`: scoped native/scalar interpretation and limits.

- `iter259/native-process-root-admission2/pair-verification.json`: qualified native process pair and actual joins.
- `iter259/accept-wait-diagnostic-packet2/accept-wait-execution/capture-qualification.json`: complete scalar capture and its limits.
- `iter259/accept-wait-diagnostic-packet2/accept_wait-root/root-result.json`: actual waits/conservation/release.
- `iter259/accept-readiness-repair1/HANDOFF.md`: current source-only readiness/auth follow-up.
- `iter259/review-orphan-ownership1/report.md`: independent current-source review.
- `iter259/orphan-current-producer1/HANDOFF.md`: strict lint and current producers.
- `iter259/orphan-focused1/root-release.json`:32 passing controls and actual joins.
- `iter259/orphan-baseline-proof1/HANDOFF.md`: meaningful prior-source failure.
- `iter259/orphan-native-baseline-reuse1/assessment.json`: unchanged native baseline.
- `iter259/orphan-live-results1/verification.json`: failed native/20 and passing60.
- `iter259/orphan-native-setup-analysis1/HANDOFF.md`: setup and relay EOF boundaries.
- `iter259/eval-latency-source-audit1/report.md`: measured difference and unproved cause.
- `iter259/orphan-empty-fastpath1/gates/root-verification.json`: ordered gates and counts.
- `iter259/review-empty-native2/report.md`: scoped empty-fastpath/setup review.
- `iter259/orphan-native-setup4/producer/HANDOFF.md`: actual qualified producer.
- `iter259/fastpath-root-admission1/`: actual current native/20/60 receipts and cleanup.
- `iter259/closing1/root-release.json`: preserved failed closing sweep.
- `iter259/watcher159-results-audit1/report.md`: bounded capture dispositions.

These paths are relative to `.git/ralph-loop/20260924-all-open`. The previous
complete failure ledger remains in `iter259/closing-docs-draft2` and its linked
immutable receipts; this current summary does not erase it. Iteration266 remains
blocked until this iteration's reviewed safe-origin contract is actually merged.


## Closing evidence and carry-over — 2026-09-28

See [[rdp-259-reply-ownership-validation-2026-09-28]] for the self-contained final
mechanism, original measurements, full closing partition, criterion-by-criterion
map and retained-failure dispositions. Private logs are not claimed as public
artifacts; no GitHub merge or CI result is claimed. The supervisor approved all four
original criteria and the done status after reading the evidence and final static
results; the branch checkpoint and publication are still to be performed.

### Final static qualification and acceptance decision

The remaining static checks qualified with30actual child waits and verified468
full-run profile identities, unchanged protected/real state and zero owned survivors.
GLOBAL was released. Plan/all-plan validation, source invariants, layout, help/skill,
vendored JS, HYALO005 and diff checks passed. Dogfood explicitly skipped because
this plan has no `dogfood_script`; the separately qualified original60 supplies its
live evidence. Firefox-reference checking explicitly found no `firefox_refs` key;
no source-reference coverage is invented. The actor tool's committed base-to-HEAD
check passed, and all eight dirty actor-note mappings were inspected; root still
must run its exact `--since d2a17038945169d21ae9a9005cde967e3bab0769` check after commit.

Historical pending/failed checkpoints above describe their recorded generations,
not the final decision. The original four criterion wordings remain unchanged and
are now checked by root's explicit approval. Static qualification is not exact-head
CI or merge.266 remains blocked until reviewed259 delivery is actually on main
and the safe-origin contract is assessed for its release call sites.

### Pre-existing spec-drift carry-over

The September28 actor-spec review found zero introduced spec drift. A bounded
read-only follow-up checked the actual installed Firefox156.0.1 package and
confirmed two pre-existing issues, filed separately as
[[iteration-286-native-sources-thread-lifecycle]] and
[[iteration-287-legacy-object-property-names-api]].286 owns native sources'
paused-on-attach assumption and unsupported detach cleanup;287 owns the unused
public ownPropertyNames helper, whose old CLI consumer was repaired in18.
Neither is a259 acceptance claim or new observed runtime failure. Both remain
planned with all tasks/criteria pending;285's withdrawn proposal stays reserved.
No extra implementation, native run or sweep was performed to file them.
