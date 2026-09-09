---
type: iteration
title: "Iteration 265: typed WatcherFront protocol fidelity"
date: 2026-09-09
status: planned
branch: iter-265/typed-watcher-protocol-fidelity
tags:
  - iteration
  - carry-over
  - protocol
depends_on: []
first_call_sites: []
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
  Tasks (0/4)
  - [ ] Verify all named contracts against supported Firefox sources and live replies before changing code.
  - [ ] Send unwatchResources without waiting for an ACK; remove the fictitious ACK assumption from its test.
  - [ ] Correct the existing parent-context argument and unused actor-reference wrappers with real protocol evidence.
  - [ ] Add meaningful real-Firefox coverage for corrected existing methods, update watcher KB, and reconcile a dual-gate closing sweep.
acceptance_criteria: |-
  Acceptance Criteria (0/4)
  - [ ] A real Firefox unwatchResources operation returns promptly without an ACK and a reverted fix fails its bounded regression.
  - [ ] getParentBrowsingContextID serializes the requested context ID and decodes its real nullable response.
  - [ ] Existing blackboxing/breakpoint-list/thread-configuration accessors decode recorded live named-key replies.
  - [ ] Ordered fmt/clippy/workspace gates, required xtask checks and dual-gate live sweep reconciled; preserve any unmet compatibility requirements honestly.
dogfood_path: |
  # Future implementation must add and execute this real-Firefox regression target.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-core --test live_watcher_protocol -- --include-ignored --nocapture --test-threads=1
scope_note: >-
  Preserve public API intent; introduce no unrelated capabilities. This is a
  future carry-over plan, with every task and AC still unticked. Full substantive plan
  is in metadata because installed hyalo0.22 has no general body editor.
---
