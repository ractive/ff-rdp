---
type: iteration
title: "Iteration 265: typed WatcherFront protocol fidelity"
date: 2026-09-09
status: done
branch: iter-265/typed-watcher-protocol-fidelity
tags:
  - iteration
  - carry-over
  - protocol
depends_on: []
first_call_sites:
  - primitive: "spec::request::GetParentBrowsingContextId"
    site: "WatcherFront::get_parent_browsing_context_id"
  - primitive: "spec::response::{BlackboxingActorRef,BreakpointListActorRef,ThreadConfigurationActorRef}"
    site: "WatcherFront named actor accessors"
firefox_refs:
  - path: devtools/shared/specs/watcher.js
    lines: "29-36"
    why: "getParentBrowsingContextID requires browsingContextID and returns nullable browsingContextID."
  - path: devtools/shared/specs/watcher.js
    lines: "47-52"
    why: "unwatchResources is declared oneway."
  - path: devtools/shared/specs/watcher.js
    lines: "67-98"
    why: "Watcher actor accessors return network, blackboxing, breakpointList, and configuration named typed-actor objects."
origin: >-
  Filed during iteration252 independent protocol cross-check. Existing unused
  typed-front defects; the console path uses legacy WatcherActor and is unaffected. No
  implementation authorized in the252–257 takeover.
problem: >-
  WatcherFront::unwatch_resources calls send_and_wait_ack even though Firefox
  unwatchResources is oneway. Its sole method-style call site is a unit test that
  invents an ACK. The same typed surface also omits browsingContextID from
  getParentBrowsingContextID and has explicitly documented unused actor-reference wrapper
  mismatches.
evidence: "crates/ff-rdp-core/src/fronts/watcher.rs:64–78,131–137,200–243,374; specs/watcher.rs:93–105. Firefox checkout0088392ab4ccab730743ed188ddec62d04e578b7, devtools/shared/specs/watcher.js:29–36,47–52,67–98: browsingContextID required; unwatchResources oneway; blackboxing, breakpointList, configuration named actor keys. No live failure is claimed for the unused methods."
tasks: |-
  Tasks (4/4)
  - [x] Verify all named contracts against supported Firefox sources and live replies before changing code.
  - [x] Send unwatchResources without waiting for an ACK; remove the fictitious ACK assumption from its test.
  - [x] Correct the existing parent-context argument and unused actor-reference wrappers with real protocol evidence.
  - [x] Add meaningful real-Firefox coverage for corrected existing methods, update watcher KB, and reconcile a dual-gate closing sweep.
acceptance_criteria: |-
  Acceptance Criteria (4/4)
  - [x] A real Firefox unwatchResources operation returns promptly without an ACK and a reverted fix fails its bounded regression.
  - [x] getParentBrowsingContextID serializes the requested context ID and decodes its real nullable response.
  - [x] Existing blackboxing/breakpoint-list/thread-configuration accessors decode recorded live named-key replies.
  - [x] Ordered fmt/clippy/workspace gates, required xtask checks and dual-gate live sweep reconciled; preserve any unmet compatibility requirements honestly.
dogfood_path: |
  # Real-Firefox regression target added and executed by iteration 265.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-core --test live_watcher_protocol -- --include-ignored --nocapture --test-threads=1
scope_note: >-
  Preserve public API intent; introduce no unrelated capabilities. The existing
  typed Watcher methods are corrected and all four tasks and acceptance criteria are
  fulfilled. The parent-context signature change and live verification are
  documented below.
---

## Implementation preflight — 2026-09-19

Current source still waits for an ACK to one-way unwatchResources, omits browsingContextID from the parent-context request, and reads top-level actor IDs instead of named blackboxing/breakpointList/configuration reply fields. Verify with supported Firefox source and real replies before changing behavior. Correct both front and spec layers: UnwatchResources also lacks ONEWAY=true in the typed spec, so switching to typed call alone does not fix the wait. Explicitly decide compatibility for the parent-context method signature and identify real non-test consumers of new public request/reply types. Do not invent CLI capabilities or dummy consumers. These unused typed methods are separate from252's legacy WatcherActor console path.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Completion evidence — 2026-09-19

Firefox 156.0 (BuildID `20260909172920`, mozilla-release
`a80bd15ddee3b4bf3679aeba340e9d2db933c467`) was verified against its exact
`watcher.js` spec and actor source. The local checkout at `0088392a` has the
same named contracts but was not treated as installed-version provenance.

The real-Firefox regression recorded `browsingContextID: 2`,
`blackboxing.actor`, `breakpointList.actor`, and `configuration.actor` replies.
Both final-source live tests passed. With only `UnwatchResources::ONEWAY`
reverted, the bounded regression failed after 0.51 s with `Timeout`; restoring
the fix returned promptly.

The public parent-context method now requires `browsing_context_id: u64`.
There was no non-test caller to migrate, and retaining the zero-argument
signature would preserve an API that can only send an invalid Firefox request.
The new public request/reply types are consumed by the corresponding existing
`WatcherFront` methods; no dummy capability was added.

The dual-gate closing sweep reconciled all six tiers:
`executed=346 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0
total=346`, comprising 344 passes and the two failures disposed below.
`LIVE_SWEEP_PROFILES leaked=0 unattributed=0`.

## Carry-over

| Finding | Disposition |
|---|---|
| `live_137_consent_accept_via_daemon` failed because the daemon retained one target but never promoted it to live within the 15 s bound. | Folded into existing iteration 262 target-readiness work. |
| `live_repeated_hop_never_loses_the_connection` failed on hop 9 with daemon-auth EOF (`failed to fill whole buffer`). | Folded into existing iteration 268 pre-auth connection-loss work. |
| The closing sweep was red because of the two assigned failures above. | No iteration-265 protocol requirement is unmet; the exact failed verdicts and owners remain visible here and in the retained sweep log. |
