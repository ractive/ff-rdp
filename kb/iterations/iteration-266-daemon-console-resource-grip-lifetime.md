---
type: iteration
title: "Iteration 266: daemon console resource grip lifetime"
date: 2026-09-12
status: planned
branch: iter-266/daemon-console-resource-grip-lifetime
depends_on: ["259"]
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

## Implementation preflight — 2026-09-19

The direct-console lifetime repair in252 does not close daemon cleanup. Quantify actual retained actors, then define ownership across caches, RPC clients and stream subscribers before broadening extraction. Current guard scope can end after raw copies are forwarded, so indiscriminate cleanup could release actors still in use. The release drainer writes on the shared Firefox connection, making259's response-attribution implications a prerequisite to resolve before implementation despite the historical empty depends_on list. Coordinate while preserving one iteration per PR. Cover observed flat/nested fields and object/longString/symbol behavior. The original substantive requirements reside in YAML frontmatter; read them explicitly rather than treating an empty historical body as an empty plan.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Restart plan — 2026-09-19

Follow [[ralph-loop-open-iterations-2026-09-19]]. The substantive tasks and ACs
are in this file's YAML frontmatter; read them explicitly. This iteration has no
product implementation. Its dependency correction was preserved on259checkpoint
`cc44e873e977c979729372010ba65fcf088341b9` and is now also prepared here.

**Entry gate.** Start only after259 supplies independently reviewed reply-ownership
semantics adequate for release traffic, and its required delivery is verified on
main. A merged259 limitation alone may be insufficient: explicitly test whether
it permits safe daemon-originated releases. If not, retain266blocked with the
specific missing contract. Do not start while259's execution restriction persists.

**Implementation plan (Astra).**

1. Reverify running Firefox's release behavior for object, longString and symbol
   actors;155's symbol missing-ACK evidence is historical, not proof about every
   later build. Read252's direct-path cleanup and265's typed watcher one-way
   contracts, then identify their actual daemon call sites. Do not blindly port
   the CLI subscriber's cleanup guard to a shared daemon event copy.
2. On a local recorded fixture, prime with ordinary console, then subscribe to
   follow and emit uniquely counted flat/nested object, longString and symbol
   values. Track which actual server actors remain usable and when they are
   released on the same owned connection. Establish a sustained before baseline
   covering catch-up duplicates, filtering, cache eviction and subscriber exit.
   Reuse252's supported actor-existence measurement technique where applicable;
   process-memory growth alone cannot prove actor lifetime or release.
3. Write an ownership table for cache, RPC inspection and each stream subscriber,
   including duplicate delivery, disconnect, eviction and shutdown. Define the
   final-owner transition before writing release code. Verify acknowledgements
   are consumed by the right owner and a missing symbol ACK cannot block event
   delivery. Avoid redesigning unrelated Grip APIs or broadening259's scope.
4. Cover the actual recorded actor-bearing fields: arguments/styles, flat fields,
   nested previews and long symbol names. Add recorded replay plus an omitted-
   cleanup mutation that demonstrates retained actors before and bounded lifetime
   after. Exercise two concurrent consumers and prove one subscriber cannot
   prematurely release another's actors or reorder/drop console events.
5. Repeat the same sustained workload against the repair with its declared event
   count/rate and retention bound, preserving actual actor counts and duplicate
   accounting. Update actor/protocol KB entries, run own dual-gate closing sweep
   and ordered gates, and obtain independent Astra review of ownership and wire
   behavior. All four original ACs must be supported, including release replies.

Before the first live lifetime measurement, derive the expected retention bound
from the actual cache/ownership contract and record finite event count/rate, actor
inspection points and a maximum wall time. The workload must exercise eviction
and sustained operation, not stop before the cache could fill. If that contract
is unknown, resolve it first instead of beginning an open-ended baseline. Keep
these declared inputs/bounds unchanged for the before/after comparison; record
actual baseline counts separately. On a measurement budget expiring, preserve the
partial result and unmet requirement rather than silently extending the run.
Missing safe shared-connection semantics blocks product
release changes; it is not permission to discard events, skip actor kinds or
relax the concurrent-consumer lifetime requirement. No paid measurements are needed.
