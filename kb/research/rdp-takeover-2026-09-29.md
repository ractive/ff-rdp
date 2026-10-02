# Read first: ff-rdp takeover, 29 September 2026

This is the consolidated takeover record requested by the owner after the 292
closeup. It documents existing results; it does not resume execution. The dated
execution notes and every failure remain intact. Read the required guidance in
AGENTS.md, then this note with Hyalo. Use the exact source and evidence below,
not session memory or an older clean checkpoint.

## Current outcome in plain English

Iteration 292 is unfinished. A historical test saw one frame through the daemon
and two through a later direct connection. We still do not know which transition
lost or withheld the child frame. The new diagnostics can describe observed
transitions, but cannot yet distinguish an event Firefox never emitted from one
whose diagnostic record never reached the collector.

Four defects in the diagnostic code were fixed and independently reviewed. This
is not a fix for the original Firefox/ff-rdp failure. Original Tasks and Acceptance
Criteria remain 0/3. No original 159 live attempt was admitted or consumed; no 292
commit, PR or merge exists. Full ordered workspace gates and closing live gates
have not run for 292. The code is an archived, uncommitted diagnostic candidate.

All delegated work stopped and the owned lock was released. The final process
census found no unexpected processes and conserved 571 private profiles and the
protected desktop Firefox. Real-home state was not conserved: daemon files
changed and one pre-existing managed profile disappeared. Attribution is unknown;
the discrepancy is documented below rather than treated as a clean-state pass.

Fresh weekly remainder was 5% at 17:19:31 CEST on 29 September, reset 13:58 on 4 October.
The owner's current rule is to begin persisted safe closeup below 5%; this replaces
the older 3% execution floor. Closeup reserved capacity near the threshold; no
below 5% reading is claimed. This follow-up performs documentation only.

## Authoritative locations and exact recovery

Paths below are local. In this note, `R` means:
`/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter292/`.
Phase/evidence paths are relative to R unless another root is given.

| Item | Exact location/state |
|---|---|
| Preservation checkout | `/Users/james/devel/ff-rdp`, HEAD `db6aabd1048a1731f3ff57536ea0771ac6ddc0f0`; existing dirty/untracked handoffs retained |
| Clean merged main | `/Users/james/.cache/ff-rdp/remaining-20260919-262`, `e4d75e013ed5af76bbdc6059e91aee9bf45bf5b9` |
|292 execution checkout | `/Users/james/.cache/ff-rdp/all-open-20260929-292` |
|292 branch/base | `iter-292/missing-child-target-delivery`, base `e4d75e013ed5af76bbdc6059e91aee9bf45bf5b9` |
|292 working state |10 dirty paths:9 Rust source files and its original plan plus checkpoint prose; no staged/committed 292 result |
| Canonical 292 plan | Execution checkout `kb/iterations/iteration-292-missing-child-target-delivery.md`; the older 286 copy is a retained historical plan |
| Source archive | `final-checkpoint/source.tar.gz` |
| Exact path/hash inventory | `final-checkpoint/source-manifest.json` |
| Checkpoint/verification | `final-checkpoint/checkpoint.json`, `checkpoint.sha256`, `final-verification.json` |
| Original closeup note archive | `final-checkpoint/primary-handoffs.tar.gz`,30 primary dirty/untracked paths as of 17:18 |
| Follow-up documentation archive | `documentation-closeout/updated-handoffs.tar.gz`; supplements, does not replace, the original archive |
| Complete 346-file 292 evidence index | `documentation-closeout/evidence-manifest.json`, generated before this documentation delta |
| Full 297-document iteration inventory | `documentation-closeout/full-iteration-inventory.json`, with canonical checkout per path |
| Phase/review index | `validation-index.json`; exact commands, environments, logs, waits and source snapshots are in each phase directory |

Verified closeup hashes:

- Source archive: `21b244d38416f7efe1e4602f9ceec446650d9c454b883f1491c96c90251bf85f`.
- Original checkpoint JSON: `74759b67570f0d6c3bb2ac7842e5705deb7abdff88288fa906629e2ac60d583b`.
- Original primary-handoff archive: `97bef7e1bd954bceac898a346b227fc64f4b391847d007244fbc82867fb77dcd`.

The original archive was read back and every source entry matched its manifest.
The documentation follow-up verifies those bytes again and records its own
manifest/verification without rewriting the old checkpoint. Earlier phase hashes
may differ from final source: always use the final source manifest for recovery.

Keep the existing dirty checkout if available. If it is lost, first verify the
archive hash, extract into a separate staging directory, verify every entry
against the manifest, and reconstruct from the exact base in an isolated checkout.
Inspect paths before applying them. Do not blindly extract over newer main, reset
the existing checkout, delete evidence, or import 286's unrelated source/history.
Private evidence is intentionally outside tracked product history under.git;
its presence is not a commit or a remote backup. Do not clean that directory
until retained evidence has a separately verified replacement and the owner has
requested the cleanup.

## What the nine source files do

Paths are relative to the 292 execution checkout.

| Source | Responsibility |
|---|---|
| `crates/ff-rdp-core/src/frame_diagnostics.rs` | Opt-in bounded per-process journal, observation permits/loss accounting, non-certifying interim markers, permanent final seal, concurrency/loss controls |
| `crates/ff-rdp-core/src/lib.rs` | Module exposure |
| `crates/ff-rdp-core/src/transport.rs` | Passive raw receipt/error and process-local connection bindings; bulk errors are not all terminal |
| `crates/ff-rdp-core/src/actors/tab.rs` | Existing raw tab/descriptor information used for bindings |
| `crates/ff-rdp-core/src/actors/watcher.rs` | Direct watcher subscription/catch-up/drain/error and raw form observations |
| `crates/ff-rdp-cli/src/commands/frame_targets.rs` | Connection-bound command intervals and actual local snapshot receipt |
| `crates/ff-rdp-cli/src/daemon/server.rs` | Source/generation admission, rejection, selection and publication observations; I/O after releasing the generation gate |
| `crates/ff-rdp-cli/src/main.rs` | Parsed command begin/end and final host seal attempt |
| `crates/ff-rdp-cli/src/daemon/server/frame_diagnostic_controls.rs` | Production-path fixtures and record-driven identity/lifecycle/cut checker; test-only, not a native admission CLI |

`FF_RDP_FRAME_DIAGNOSTICS` points to an existing private journal directory.
Default-off behavior adds no protocol requests. Retained journal limits are 4096
ordinary records,32 KiB payload and 8 MiB total; loss and invalid evidence must
refuse absence claims. These limits do not bound transient allocation or disk
latency. Synchronous diagnostic I/O has overhead; timing equivalence is not
claimed. A host seal covers the admitted host observation interval only, not
Firefox emission, future delivery, process durability or worker shutdown.

## Review and validation ledger

Counts below describe each scoped control, not a full suite. Parent and isolated
child output for the same test is one control, not two independent passes.

| Phase | Result and retained limitation | Independent review |
|---|---|---|
| `offline-collector1` | Initial CLI 1/0 plus core 1/0; raw host journal and existing 283 same-cut control. Incomplete/unsafe collector findings followed. | `review-collector1`: R1/R2 required |
| `offline-repair1` | Real-contention and slow/failing-sink counterfactuals failed as intended; final CLI 2/0 plus core 2/0 | `review-collector-repair1`: R1/R2 repaired, R3 required |
| `offline-repair2` | Actual public-entry initialization counterfactual failed; corrected core 3/0 | `review-collector-repair2`: ACCEPT, zero findings |
| `upstream-collector1` | Four syntax checks; actual watcher-method baseline/overlay produced the same 8 events/state; five local loss controls | `review-upstream1`: ACCEPT for positive observations only |
| `upstream-lifetime2` | Six syntax checks; actual enumeration/query/membership controls; timeout/frozen completion distinguished from real child response | `review-upstream2`: ACCEPT for positive observations only |
| `offline-host2` | Real ConnectedTab direct/daemon paths, catch-up/drain/error, stale queue rejection and record-driven joins; final 1/0 | `review-host2`: R4 required; no additional production-hook defect found |
| `offline-host2-repair1` | Duplicate-actor negative failed on old checker; corrected 1/0 | `review-host2-repair1`: ACCEPT, zero findings |

R1 was a race between a failed journal try-lock and publication of a lost-record
counter. Active permits now cover that interval and final closure excludes them.
R2 performed diagnostic I/O under the daemon generation lock; immutable capture
stays under the lock and emission occurs after release. R3 allowed public seal()
to wait on OnceLock initialization; it now refuses via nonblocking lookup. R4
accepted `[top, top]` instead of `[top, child]`; the checker now requires unique
actors and full equality with the replayed lifecycle set. These are diagnostic
repairs, not evidence of the historical product mechanism.

Preserved failures and deviations:

- Initial collector compile failure attempted to serialize a non-Serialize event;
  final code records actor IDs while retaining raw forms separately.
- Initial core-only test selection did not exercise CLI coverage; CLI coverage
  was added explicitly rather than counting the first selection as sufficient.
- R1/R2 counterfactuals each failed once at their intended assertion; not repeated.
- Repair 1 compile failed on a missing test initializer field. A later real-origin
  elapsed-time correction invalidated the earlier focused result and caused one
  changed-input rerun.
- R3's first test draft failed to compile due to an undeclared tempfile dependency.
  The separate runtime-before failure is the behavioral evidence.
- Host 2's checker-edit script failed before saving. The shell then accidentally
  repeated the unchanged passing test. That repeat is preserved and earns no new
  qualification. The corrected cut-order checker and isolated test then passed.
- Host 2's final module-declaration formatting check initially failed; formatting
  was corrected and checked. No Cargo repeat was needed for declaration order.
- R4's actual-record duplicate negative failed before the fix and passed after it.
- Source extraction initially requested a wrong member path; corrected extraction
  retained the failure. A private-directory command failed before execution when
  the directory did not yet exist. Neither was a native Firefox attempt.
- The first closeup inventory type filter omitted 19 historical done documents;
  the retained corrected inventory counts document paths, including missing/guide
  types, as earlier ledgers did.
- Freshness-unverified weekly monitor results remain unknown, not zero or success.

Exact argv/env/PIDs/exits/actual waits, failure logs, preimages and candidate pins
are in phase directories. Passing inputs can be reused where contracts and bytes
remain unchanged; do not rerun every control merely because another session starts.
No 292 strict workspace Clippy, full workspace tests, closing dual-gate sweep,
exact-head CI, commit or merge is claimed.

## Firefox work and the exact admission blocker

The normal Firefox package remained unchanged by the recorded diagnostic phases:
Firefox 156.0.1, BuildID 20260921121718; browser omni.ja SHA 256
`85f891cec3e54150027582ac74eb96fc3774cc0ab4bfd96f7192905149d05fff`.
Extraction verified unique local headers, stored-method sizes and CRCs. Source
maps and reversible overlays live in `upstream-offline1`, `upstream-collector1`
and `upstream-lifetime2`. No diagnostic app was packaged, signed, installed or
launched in those phases. No privileged runtime behavior is proven by Node stubs.

Observed boundaries include initial/context enumeration, target construction and
enqueue, parent validation, watcher immediate emission/early-child queue/flush/
destruction, existing registration/query/startup and normal teardown. A child
can be queued under its raw topInnerWindowId. That is a possible observation
boundary, not a demonstrated cause. The historical cumulative 2 is already
accounted for by two successive top-level announcements, not a proven child.
Do not repeat that answered lookup.

The packet lacks all of the following:

1. An independently justified inventory of every expected process/module/watcher
   incarnation over the command interval, including a producer that emits nothing.
2. A natural recorded end for every expected producer and actual loss-accounted
   collector drain. Process death, a last line, local teardown return, or EOF alone
   does not establish those facts.
3. An acknowledged owned sink. dump() has no retention acknowledgement; missing
   trailing output and a wholly absent producer can be invisible. A checked file
   or owned pipe alone does not supply the missing inventory and completion proof.
4. Completed host primary/optional startup binding and cross-connection Firefox
   lifetime/document identity joins. Interval overlap alone is insufficient.
5. Qualified diagnostic runtime selection and privileged hook execution. Normal
   launch prefers `/Applications/Firefox.app` over PATH and discards stdout;
   PATH-only substitution or an assumed captured dump stream will not work.

Existing parent watchTargets may resolve after 1000 ms or immediately for a frozen
process before the actual child response. Its outer return is not delivery proof.
The bounded inspected source/control questions are answered. No proof here makes
every possible passive collector impossible or a new IPC barrier logically
mandatory. A future proposal needs a concrete missing mechanism; more equivalent
logs, wider waits, blind retries or a full sweep do not satisfy these gaps.

## Finite live schedule and next execution steps

The original 292 schedule remains unconsumed and unadmitted: at most one original
`live_159_frame_targets_survive_the_fix`,120 s active plus 30 s cleanup,150 s hard total.
Preserve its cross-origin fixture, navigate → daemon-frame → direct-frame → status
order, original count assertion,30000 ms command timeout and 800 ms settle. A pass,
setup failure or incomplete record consumes the attempt without a retry and does
not explain the historical failure. H1 construction/notification, H2 parent/queue,
H3 transport and H4 source/publication share this one occurrence, not four grants.

At an explicitly resumed execution: verify guidance, fresh usage, actual refs,
archives, dirty files, ownership and real state first. Finish the missing host
bindings and demonstrate the upstream inventory/end/sink mechanism offline as far
as possible; qualify any diagnostic package and ownership packet independently.
Only then decide whether the original occurrence is admissible. Repair only a
demonstrated mechanism. Preserve focused before/after proof, independent review,
original criteria, ordered fmt → strict workspace Clippy → workspace tests, its
own required closing validation, and all exact-head CI checks before merge.

The standing merge grant uses GitHub through `gh pr merge --merge` after all CI
checks on the exact head are green and independent local findings are addressed.
It is not permission to claim completion from diagnostics alone. Keep one writer
per checkout and one global Cargo/Firefox workload; retain one implementer across
repair cycles and use fresh independent reviewers. Never message a running
workflow agent. Do not reset historical capture/time ledgers or invent another
two-batch limit. New 292 expenditure is separate; measured token usage is unavailable.
Recorded run wall span is 8607.9 s, not measured active compute or a sum of overlapping
agent time. Individual phase/review intervals retain their own measurement limits.

## Real-home discrepancy and cleanup evidence

The final census was complete with no unexpected processes;571 private profiles
and desktop Firefox 79047/helper 79051 were conserved. The run-owned lock release
is `final-checkpoint/lock-release.json`; command children were actually waited in
the phase receipts. Detached process disappearance is never worker-return proof.

Real-home spawn/write locks changed at 13:04:36–37 UTC (15:04 CEST); daemon.log grew
by 230 bytes by 13:07:32 UTC. Its intact 1423990-byte prefix matches startup. The append
names daemonPID 10257/proxy 59448, an EOF, and worker/lifecycle failure. The previously
present `ff-rdp-profile-ZbwnrIYH6IN4YsDR` is absent. Directory mtime is not a deletion
receipt, and owner-marker hashes cannot reconstruct missing bytes or ownership.

The recorded initial controls were PIDs 5224/7285, last observed 13:02:38 UTC; inspected
paths contain no daemon startup or profile cleanup. This narrows the evidence but
does not identify an external caller or exclude every unrecorded invocation.
The repeated EOF wording is not attribution. No restoration/deletion/source fix
was justified or attempted. Preserve `real-state-audit1/report.md`, its actual
real-state copies, append, pins and timeline. Report SHA 256:
`f8a7766960d45fde5551988e8773a8b2067bb94e2ee49d994fe873a232a6d0b1`.
Any future attribution needs contemporaneous caller/executable/argv/environment
or deletion evidence; do not rerun a live capture to manufacture past evidence.

## Past completion and the remaining queue

The last verified inventory is 297 plan documents, not unique iteration numbers:
264 done,23 obsolete,7 planned,3 in-progress. The complete machine-readable
inventory is in documentation-closeout. Only 292 advanced during this execution.
Other historical statuses below are retained observations, not fresh runtime proof.

- 262 completed in PR 264, merge `c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2`;
  verified ancestor of the 292 base. Its reviewed repairs remain retained.
- 287 merged in PR 283 at `c696f09f98ff17c4c8a18bccd1630f00076d9a06`.
- 288 completed in PR 284 at `e4d75e013ed5af76bbdc6059e91aee9bf45bf5b9`;
  reviewed head `73fcce24c693d2aa0caafa83dd5b586912f3ce9c`, ten CI checks green.
  Its closing sweep remains 348 pass/3 fail of 351, with three separately passing
  supplements. Never relabel it a single 351/351 sweep. Causes of those startup
  failures remain with 294. Installed CLI was last recorded as the earlier merged
  c 696 build; neither 288 nor 292 reinstalled it.
- 289/292 plan reconciliation completed and was independently accepted before
  execution. That completed planning task did not complete either iteration.

| Iteration | Status/disposition | Next prerequisite |
|---|---|---|
|259|Planned; PR 281 Windows hang deferred by owner|Obtain Windows machine and attributable hang evidence. Last recorded head `cee57bf48e753b3712d9750005a6102b7640145d`; verify afresh.|
|266|Planned, dependent|259 must merge; do not bypass reply-ownership dependency.|
|268|In progress, blocked, AC 0/4|Qualify native ownership/return attribution. Latest after 287 original 224 was refused for orphan ancestry;240 was unrun in that attempt.|
|286|In progress, blocked, AC 0/4|Native-source lifecycle and missing-target/child-delivery attribution; retained C 1/C 2 failures are not discharged.|
|289|Planned, AC 0/3|Adapt only its private phase-bound 262 harness to merged main and qualify producer/loss/ownership. Separate 90+30 s single schedule remains unused/unadmitted.|
|290|Planned|Local action readiness failure attribution under its original plan.|
|291|Planned|Navigation elapsed-time outside the observed interval; preserve timing interpretation.|
|292|In progress, blocked, AC 0/3|Collector/startup/lifetime/runtime gaps detailed above.|
|293|Planned|Cancelled submission latency attribution.|
|294|Planned|Managed startup timeout attribution from 288; later passes are not historical causes.|

268 source remains in `/Users/james/.cache/ff-rdp/all-open-20260928-268`.
Its guarded 24-path checkpoint is under the parent run directory
`iter268/blocked-after287-native224-checkpoint1/checkpoint.json`, SHA 256
`e5a17938b6de69794a91d5f01665673741173f66f74810ea1bdfe1229ea53147`.
286 remains in `/Users/james/.cache/ff-rdp/all-open-20260928-286`, with its 18-path
candidate and private plans. Preserve both;292 used an isolated checkout and did
not import those candidates. No new execution of other iterations is implied by
this document. The earlier 271 exclusion is not a new queue entry or restart grant.
GitHub open issues were last verified zero at 288 closeout; this documentation
request did not re-query GitHub or claim that observation is current indefinitely.

## Navigation and documentation verification

Detailed chronology: [[rdp-292-execution-2026-09-29]].
Prior completed work and queue history: [[rdp-active-handoff-2026-09-29]].
Original reconciliation: [[rdp-289-292-reconciliation-2026-09-29]].
The historical sections in those files are preserved; their old active-state
sentences are superseded by their leading stopped summaries and this record.

Documentation-only changes, preimages, all 346 evidence hashes, full inventory,
updated note archive, link/path checks and source/archive verification are under
`documentation-closeout/`. No new test, capture, implementation, PR or merge was
performed for this owner request. The source checkpoint remains byte-identical.
