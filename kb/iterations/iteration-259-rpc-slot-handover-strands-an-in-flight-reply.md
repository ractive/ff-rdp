---
title: "Iteration 259: an RPC-slot handover can deliver one client's in-flight Firefox reply to the next client"
type: iteration
date: 2026-09-07
status: planned
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

### A. Establish the window [0/3]
- [ ] Reproduce a stranded reply reaching a second client, or establish that it cannot be
      reproduced and say what was tried
- [ ] Record how often a slot release happens with a request outstanding, over the iteration-240
      60-hop dogfood loop
- [ ] Read the Firefox source for any reply-to-request correlation beyond `from` (Theme B)

### B. The fix or the documented limitation [0/2]
- [ ] Either attribute (or discard) replies belonging to a departed owner, or record the
      limitation with the reasoning that closed the design space
- [ ] Report slot releases-with-requests-outstanding in `daemon status` (Theme D) — this is worth
      doing whichever way B lands

### C. Proof [0/2]
- [ ] A test that fails on the pre-fix daemon: a client that departs mid-request must not have its
      reply delivered to the next claimant
- [ ] The iteration-240 dogfood loop still reports 60 hops / 0 reconnects / 0 abandoned clients,
      and 20 sequential `eval` invocations are no slower than before (Theme C)

## Acceptance Criteria [0/4]

- [ ] The window is either closed or documented, with the measurement in the Outcome either way
- [ ] No regression in `daemon status` answerability or in the sequential invocation path
- [ ] `daemon status` names the condition when a slot is released with work outstanding
- [ ] Every claim in this plan's Outcome is backed by a run recorded on the branch

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
