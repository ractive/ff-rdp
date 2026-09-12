---
type: iteration
title: "Iteration 266: daemon console resource grip lifetime"
date: 2026-09-12
status: planned
branch: iter-266/daemon-console-resource-grip-lifetime
depends_on: []
first_call_sites: []
origin: >-
  Filed during iteration252's direct-console grip-lifetime repair. This is a
  pre-existing daemon ownership/extraction gap; the new direct path was fixed in252.
  No266 implementation is authorized by the current252–257 takeover.
problem: >-
  Daemon dispatch_firefox_message only constructs ResourceGripGuard inside
  is_watcher_event, which requires from==the daemon watcher actor. Content-process
  console resources originate from windowGlobalTarget and bypass that branch. Even within
  it, extract_grips only visits nested message.arguments/styles, only immediate
  object/longString values, missing flat fields, recursive previews and symbols.
  GripHandle/ResourceGripGuard have no symbol handling. These paths are unchanged from
  origin/main05634cbd42f02995f50e06c35d68b868cd39d0ba, where the daemon already
  enabled server switching and frame targets. Actor allocation therefore predates252's
  parser change.
evidence: "Firefox155.0.1 and source0088392ab4ccab730743ed188ddec62d04e578b7: createValueGripForTarget stores actors in target.objectsPool; object previews and symbol names allocate independently managed grips. Objects and strings ACK release, but SymbolActor destroys itself before protocol/Actor can send its declared ACK. Actual release fixtures and900-actor direct before/after proof are retained in252's pr247-grip-lifetime evidence. This source audit establishes missing daemon cleanup paths; a quantified real-daemon lifetime baseline remains a task here."
scope_note: >-
  Resolve cleanup in the owning daemon, not in a CLI stream client that receives
  copies of shared events. Audit RPC consumers, stream subscribers, resource caches
  and asynchronous release replies before choosing an ownership policy. Keep
  symbol wire behavior and nested-grip extraction separate from any optional general
  Grip type redesign. Coordinate with259 if release replies expose its
  request-attribution hazard; do not silently expand259 or weaken its ACs.
dogfood_path: "# Future implementation must add a bounded real-Firefox daemon lifetime regression, then execute it with both FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1. Prime the daemon with plain console before follow, log fresh object/longString/symbol values on a page timer, and verify actor existence/release on the same shared Firefox connection while keeping concurrent consumers correct."
tasks: |-
  Tasks (0/4)
  - [ ] Reproduce and quantify daemon resource grip retention on real Firefox, distinguishing flat resource, legacy and nested grip shapes.
  - [ ] Define daemon ownership across cache, RPC and stream consumers; release after the last consumer without racing inspection or request/reply attribution.
  - [ ] Cover all actual actor-bearing console fields, including styles, object previews and long symbol names; respect symbol release's missing ACK on Firefox155.
  - [ ] Add recorded replay, omitted-cleanup mutation and sustained live lifetime evidence; update protocol docs and run ordered gates plus the dual-gate closing sweep.
acceptance_criteria: |-
  Acceptance Criteria (0/4)
  - [ ] Sustained daemon console logging has a demonstrated bounded server actor lifetime, including primed duplicate delivery, filtered messages, flat fields and nested object/longString/symbol grips.
  - [ ] Concurrent daemon consumers retain their intended actor lifetime; a CLI stream subscriber cannot prematurely release another consumer's actors.
  - [ ] Release replies cannot be mistaken for unrelated RPC replies, and missing symbol ACKs cannot deadlock or drop/reorder console events.
  - [ ] Meaningful recorded replay and mutation coverage pass, as do ordered fmt/clippy/workspace gates and a reconciled dual-gate live sweep; every unrelated failure has an explicit disposition.
tags:
  - iteration
  - carry-over
  - daemon
  - console
  - protocol
---
