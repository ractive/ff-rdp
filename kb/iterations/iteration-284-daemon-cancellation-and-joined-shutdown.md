---
title: "Iteration 284: Daemon cancellation and joined shutdown"
type: iteration
date: 2026-09-25
status: done
branch: iter-284/joined-shutdown-20260925
depends_on: []
first_call_sites:
  - primitive: ff_rdp_core::RdpTransport::from_stream
    site: crates/ff-rdp-cli/src/daemon/server.rs::background_establish_watcher_loop
  - primitive: ff_rdp_core::RdpTransport::try_clone_stream
    site: crates/ff-rdp-cli/src/daemon/server.rs::run_daemon
  - primitive: ff_rdp_core::FramedWriter::send_raw_until
    site: crates/ff-rdp-cli/src/daemon/client_writer.rs::ClientWriter::send_raw_until
dogfood_path: >-
  Use a finite deterministic actual-thread schedule for cancellation, blocked I/O
  and joined shutdown, with normal-service controls and exact owned joins. Run the
  required ordered gates and this iteration’s own dual-gate closing sweep; do not
  substitute process death or passive capture completeness for worker return.
tags:
  - iteration
  - daemon
  - lifecycle
---

# Iteration 284: Daemon cancellation and joined shutdown

## Problem and boundary

The September25 protocol audit identifies a current lifecycle contract gap:
`run_daemon` discards worker handles, detaches client handlers and can return
after setting shutdown without joining them. Retaining handles alone is
insufficient: socket reads, bounded channels, continuous input, writer locks
and queued RPC claimants can prevent those workers from reaching an exit.

This prerequisite to [[iteration-268-daemon-pre-auth-connection-loss]] delivers
cancellable ownership and actual joined shutdown. It is a separate product
repair under the owner’s all-open adaptation request. It does not explain
historical 224/240 EOF/reset failures or complete268. No archived diagnostic
source is imported wholesale and no passive collector is added merely to
observe threads that the product still leaves detached.

## Tasks [4/4]

- [x] Define and implement private ownership of every acquired reader,
      dispatcher, release drainer, client handler and optional lazy watcher,
      with one idempotent stop reason and admission stopped once shutdown begins.
- [x] Make each blocking path cancellation-aware: socket/auth reads and writes,
      channel receive/send/backpressure, writer-lock contention and queued RPC
      claims. Wake or interrupt owned I/O without first acquiring a blocked
      writer lock; preserve normal traffic and 262’s startup-recovery reply sink.
- [x] Join all acquired workers before daemon return, including partial startup
      failure and panic paths. Record actual join outcomes and propagate failure
      honestly. Resolve optional lazy-watcher loss policy explicitly: its current
      continued-main-service promise and shutdown-on-any-return supervision must
      agree. Do not strand an acquired handle when startup or a worker fails.
- [x] Exercise a finite deterministic schedule through the actual product thread
      lifecycle, with repair-sensitive regressions and normal-service controls;
      obtain independent review, ordered gates and this iteration’s own required
      dual-gate closing sweep.

## Acceptance Criteria [4/4]

- [x] Stop admission and idempotent cancellation have a defined observable reason;
      controlled stop during authentication, blocked/full channel operations,
      queued RPC claims and occupied writers wakes all affected owned threads.
- [x] Every acquired worker/handler/lazy-watcher handle has an actual join outcome
      before daemon return, including partial startup, panic and optional-watcher
      loss. Successful return requires the declared successful shutdown contract;
      process death, an exit marker or missing output never substitutes for join.
- [x] Meaningful before/after controls exercise the real thread paths and preserve
      normal request/event ordering, 262 reply-sink behavior and the selected
      lazy-watcher policy; no arbitrary sleep, broad timeout increase or normal
      traffic loss is used to make shutdown finish.
- [x] Independent review and ordered fmt/clippy/workspace gates pass, followed by
      this iteration’s own dual-gate closing sweep with exact-name/profile
      reconciliation and explicit dispositions for other failures.268’s original
      tasks/AC 0/4 and separate 224/240 validation remain unchanged.

## Validation and scope limits

Use a declared finite schedule with synchronization at actual blocking points,
owned socket/peer endpoints and retained actual joins; include an ordinary
traffic control and partial-acquisition failure. The schedule should establish
cancellation mechanics directly, without a new native termination collector,
unbounded pass streak, artificial load campaign or extra discovery sweep.
A watchdog terminating a stuck process is a failed control, never successful
worker completion. Test cleanup must release its own controlled blockers.

Keep the implementation private unless a real non-test consumer requires an
interface. Do not redesign general protocol attribution, grip ownership or
public transport APIs. 259 owns request/reply handover and 266 owns grip lifetime;
serialize overlapping daemon edits but do not invent a dependency on their
unresolved historical investigations. After 284 merges,268 separately assesses
its required scoped attribution and224/240 validation, preserving consumed
capture claims and every missing historical outcome.

## Provenance

The bounded September25 protocol reconciliation reused current-main source
observations and retained268 evidence. This plan creates a prospective product
contract, not a finding that detached workers caused any historical auth loss.
Independent scope review is required before implementation. All tasks and
criteria remain pending.

## September 25 first candidate — focused proof recorded, repair pending

The separately authorized 284 work preserves the original tasks and criteria
above. Its first bounded non-live block captured failures in two unchanged
product paths: the full event-queue reader did not return on flag-only stop,
and an RPC claimant took ownership after stop. Cleanup retained and joined
both actual threads. Seven new private primitive controls passed; these passes
are not evidence that the old product defects were repaired.

The implementation retains all worker/handler handles, publishes
one cancellation reason under the common admission gate, interrupts independent
socket handles and writer leases before notifying RPC waiters, and joins before
registry removal or successful daemon return. Startup catch-up is consumed in
order before the live bounded FIFO, including sequences exceeding 4096 frames.
The existing shutdown response budget covers lease acquisition and every write
attempt; partial-frame expiry retires the connection and interrupted syscalls
consume the same absolute budget. The 262 recovery sink remains registered
before its send pair and retained on ambiguous failure.

Normal optional watcher loss degrades watcher availability while primary
service continues; optional panic is fatal. Generation invalidation rejects
queued old setup/events and retires only optional-owned live target state.
The lazy connector reuses the primary socket's actual peer and uses cancellable
readiness waiting; initial primary DNS and connection setup happen before
owned daemon workers and are not claimed to be signal-cancellable.

The finite focused schedule includes actual reader/dispatcher/handler/drainer
paths, occupied response and Firefox writers, recovery with the RPC mutex held,
partial acquisition and unwind, optional setup/EOF/panic, stale generations,
continuous input, normal request ordering and startup replay beyond capacity.
Private transport controls cover partial writes and EINTR through the same
absolute-deadline helper used by the real socket; they do not claim a particular
kernel partial-write schedule. Each control executes once when admitted; a
failed experiment is preserved and repeated only after an attributable repair.

The first candidate’s admitted non-live block formatted and compiled the CLI/core unit targets
without compiler failures, verifying the intended manifest and fresh artifacts.
All42 selected tests passed on their first qualified execution:20 product-path
controls,7 changed primitive controls,3 absolute-write controls,6 existing262
recovery regressions,3 client-writer regressions and3 additional proof controls.
Both archived old-product failures now pass. No unchanged retry or discovery
sweep was used.

The actual connector control observed its registered Waker through real Poll,
returned Interrupted before socket adoption and joined its actual thread. This
is preposted-wake proof, not observed kernel-pending TCP or an already-blocked
kernel poll. Isolated test children called the real run_daemon entrypoint: the
normal authenticated-shutdown case recorded all4 acquired worker joins, and
the controlled dispatcher-acquisition failure recorded both acquired joins.
Neither entrypoint returned nor removed its registry while its real reader
remained gated; registry removal was confirmed after join and return. These
cfg(test) scheduling gates establish unit ordering, not native timing.

Independent implementation review, any specifically justified sensitivity
mutations, the ordered workspace gates and the own closing sweep remain
outstanding. Historical 268 attribution, its original AC 0/4 and the
separate original 224/240 scenarios remain unproved by this work.

### Independent review and source-ownership repair draft

Fresh implementation review requested two related corrections. Optional target
publication could replace the primary snapshot, leaving no primary forms after
optional loss; implicit primary top replacement also retained stale actors in
the new source-ownership map. The42 recorded passes above belong to that reviewed
first candidate and do not establish these missing cases.

The narrow repair draft retains primary and current optional target snapshots
separately under the generation gate. Primary becomes authoritative once its
own target traffic is observed, including empty destruction/replacement gaps.
Source identity alone does not trigger resource purge; genuine same-source top
replacement still prunes outgoing forms. The ownership map is derived from
retained current forms at every publication, so implicit retirement removes
its metadata in the same operation. Optional invalidation removes its own state
and exposes the retained primary snapshot without manufacturing new events.

Two new actual-dispatcher controls capture primary top/child followed by optional
top/child and optional loss, and primary top replacement without individual
child destruction. Their unchanged-reviewed-source forms are archived for one
qualified before execution each, followed by one after execution each. The repair
has not been formatted, compiled or tested yet. Focused affected regressions
and a fresh independent scoped repair review remain required; all original
criteria and historical268 limits are unchanged.


A subsequent scoped source review found that implicit optional replacement
retired the forms/map without invalidating outgoing registry actors and their
dependent fronts. The follow-up draft invalidates those source-owned actors
before their last forms are cleared, preserving the incoming actor and ordinary
same-actor reannouncement. A third actual-dispatcher control covers optional
replacement without destroys followed by optional loss, including dependent
fronts and unrelated primary preservation. Its identical pre-fix/final source
is archived for one before and one after execution. This follow-up remains
uncompiled and unexecuted; prior repair archives and all original criteria are
preserved, with fresh scoped review and focused proof still required.

### September 25 source-ownership repairs — focused proof complete

The preceding unexecuted-draft notes are historical. Fresh scoped review
conditionally accepted the final retirement repair with zero remaining source
findings, subject to the declared focused proof. That proof has now completed:
all three review controls failed once at their specific defect assertions on
the appropriate archived pre-fix production source, then passed once on the
repaired candidate. The same formatted control source was used for all three
build variants. Each new control stopped and actually joined its dispatcher
before asserting the captured result; no watchdog fired.

All15 selected affected regressions passed once:3 frame-snapshot,3 registry,
2 top-switch,6 startup-recovery and1 stale-generation test. The unrelated first
candidate42-test block was not repeated. The first baseline compile failed
because the shared target reused a stale core artifact lacking APIs present in
this checkout. Its failure is preserved; byte-preserving entrypoint timestamp
invalidation produced the corrected fresh artifact. The three qualified CLI
build variants then compiled successfully, each with the intended manifest and
a fresh unit binary. No behavioral change was needed during this proof block.

The formatted final source was restored exactly before its build, and all
source variants, producer records, binaries, commands, exits, intended failures
and passing results are preserved in the private iteration284 repair2-proof
archive. This discharges the scoped review's focused-test conditions; ordered
workspace gates, final completion review and this iteration's own dual-gate
closing sweep remain owed. Original tasks/criteria remain0/4, and no historical
268 cause, native result or closing validation is claimed.

### September 25 ordered workspace gates — passed, closing still pending

On the stable toolchain verified against the same day's successful update,
ordered formatting, strict workspace/all-target Clippy and normal parallel
workspace tests passed. The final workspace result was2637 passed,0 failed,
421 ignored. One emitted nested iteration282 child summary was excluded from
the top-level count; iteration284's two isolated entrypoint child summaries
were captured separately and were not counted or subtracted. Their retained
proofs again record actual worker joins before entrypoint return.

The initial Clippy attempt reported style lints and stopped before workspace
tests. Its failure and pre-correction source remain archived. Only the reported
style corrections were applied, then the full required sequence ran in order;
there was one actual workspace-test execution. No focused block was separately
repeated. All12 selected binaries, including the CLI, unit/e2e targets, xtask
and the six closing tiers, were frozen from the actual intended-manifest
fresh compiler artifacts. Runtime inputs and the exact style delta are retained
in the private iteration284 ordered2 archive. This iteration's own dual-gate
closing sweep remains required; original tasks and acceptance criteria stay0/4.

### September 25 own closing sweep — failed, diagnosis pending

The first own dual-gate closing sweep observed all349 qualified names across
six tiers:345 passed and4 failed. One failed name was classified as a launch
timeout, so the runner summary reports348 executed,1 launch_timeout and349 total;
there were no skipped, preexisting, vanished or watchdog-timed-out tests.
Profiles leaked0 and unattributed0. The exact summary and all failed output
remain in the private iteration284 closing1 archive.

The four occurrences are distinct. Direct166 returned the correct complete
canonical document but status null/no_document_request after21125ms. Home212
reported the old tab URL after its click while its page heading described the
new Arrived document. Fragment220 never reached its action: owned Firefox failed
to open its debugging port within the existing30s launch bound. The timing test
reported579ms against1400ms wall time, delta821ms; its trace records711.603ms
before dispatch and skips postcommit refresh. These observations establish
neither their causes nor a common cause, and no unchanged sweep was repeated.

Original224/240 and the network/frame159 checks passed in this sweep; those
passes do not provide268's separately required occurrence attribution. The raw
browser was actually waited after scoped cleanup, all344 launch-ledger entries
paired, and all10 new retained profiles were attributed with the271-profile
baseline conserved. All578 source inputs, installed Firefox110 pins and four
real profile identities remained unchanged. The six actual running test
executables plus CLI/xtask were frozen before the shared Cargo workload changed.
Original tasks and AC0/4 remain pending, with no completion PR or merge claimed.

### September 25 closing corrections — focused proof and one166 capture

The212 contract correction requests the documented `click --with-page` wait,
asserts destination readiness and the Arrived heading, then retains the original
home URL/ref assertions. One actual CLI/daemon/mock before control failed at the
original second-home URL assertion after real cleanup; the final flow and its
wrong-heading/unready negative controls passed. A fourth focused control exercised
both actual166 event/fallback paths with the real CLI tracing subscriber. These
passes do not establish the cause of a historical Firefox occurrence.

Exactly one separately admitted original166 direct-parity native invocation then
passed both original navigation legs, each reporting200. The plain path retained
its qualifying request/status through all8 new diagnostic phases without fallback.
The earlier null-status failure remains unexplained; no retry was performed.
The full private evidence-root census conserved295 baseline profiles and found
zero owned survivors; one attributable new profile remains preserved privately.
Source580, installed Firefox110 pins and four real profiles verified unchanged.

The launch follow-up addresses a verified observability/reliability weakness:
startup stderr was piped but undrained, and the child status was checked only once
before a port-only wait. The reviewed repair draft drains a bounded stderr tail
without a reader thread, polls actual child status through the original startup
budget, and distinguishes natural exit observed before cleanup signals from a
live timeout or unknown status. Cleanup retains the existing owned-child/profile
rules. Public warnings show bounded counts and state; stderr content follows the
existing trace redaction policy. Success retains the existing close-on-return
stderr lifetime; post-handoff capture is not promised.

Two actual old-product controls failed once at their intended assertions after
child cleanup: late exit37 was misclassified as timeout, and measured65536-byte
stderr saturation blocked the child before it could bind. All three final controls
passed once: late exit was identified, a child writing an additional1MiB reached
its listener and remained alive at successful handoff, and a live never-binding
child timed out at the unchanged bound before owned cleanup. Actual waits and
bounded control-channel receipts are archived; process death is not a reader or
worker join. No reader thread is created. Windows nonblocking observation is
implemented but has not been compiled or executed on Windows in this block.

Focused compilation and strict production-bin Clippy passed. An initial Clippy
style failure is preserved. After proof, a test-only archival precondition was
made optional for ordinary Cargo/CI environments; actions/assertions and the
executed private-home branch are unchanged, and the final unit target compiled.
That absent-archive-home branch is not separately executed here. All exact
variants, binaries, intended failures and receipts remain private under iteration284
launch-repair1. Fresh independent implementation review, ordered full gates and
this iteration's successful own closing validation remain required. No new native
launch probe or sweep ran, and original tasks/AC0/4 and historical268 limits remain.

### September 25 isolation proof and resumed ordered gates — stopped at guard

Independent review found and corrected a test isolation defect: ordinary launch
controls could use real default state, and an intermediate record assertion read
a liveness-filtered record after its child had been reaped. The final control
wrapper uses an actual subprocess with a private FF_RDP_HOME, observes the owned
record before releasing the live controlled child, then waits before removing
its private record/profile/home. Fresh scoped review returned zero findings.
One exact absent-parent-home success control passed with actual outer, executor
and controlled-child waits and unchanged real default-state files/directory and
profile-marker observations. It did not rerun the answered production baselines.

On the same verified stable toolchain, ordered formatting and strict workspace
Clippy passed after preserving and correcting five test-helper style lints. The
single normal parallel workspace execution then stopped at
`unit_179_no_assertion_reports_stderr_without_stdout`:11 assertions in the new
closing284 fixture reference stderr without including stdout evidence. The guard
remains unchanged. CLI unit1341/0/4 and e2e423/0/0 had passed; partial top-level
totals across9 completed targets are1796 passed,1 failed,5 ignored. An emitted
nested282 child result was excluded by its actual test identity, not counted as
a workspace pass. Remaining targets and doctests did not run, so this is not a
passing full-workspace gate.

The unexpected guard failure is preserved without a repair or retry. All12
fresh intended-manifest binaries were frozen from that actual workspace producer,
including the normal CLI, unit/e2e, xtask and six closing tiers. CLI bytes also
match the actual dependency executable with current-checkout dependency inputs;
a stale top-level depinfo file is not used as identity proof. The exact style
fixes, isolation proof, source, output and failure are archived privately. A scoped
fixture diagnostic repair/review and a subsequent ordered gate sequence remain
required before any further closing admission. No native run or sweep occurred
in this block; original tasks/AC0/4 remain unchanged.

### September 25 stdout repair and ordered5 — stopped at annotation guard

Fresh independent review accepted the narrow closing-fixture diagnostic repair
with zero findings. Actual daemon stdout is retained through child wait and stderr
join, and all11 previously reported assertions now include both captured streams;
their predicates are unchanged. The subsequent single ordered sequence reused
the same-day verified stable toolchain: formatting made no changes and strict
workspace/all-target Clippy passed. The unchanged179 stdout guard passed.

The normal parallel workspace gate then stopped at
`check_source_invariants::real_tree_passes`: the new test-only launch control file
contains two stderr sites without the required `stderr-ok` annotations, at the
late-exit marker and absent-parent-home isolation receipt. Partial top-level
results are2618 passed,1 failed,422 ignored across32 completed targets. The named
nested282 child summary is retained separately and excluded; remaining targets
and doctests did not execute. No source repair or test retry occurred here.

All12 selected fresh current-manifest compiler artifacts are frozen, including
CLI/unit/e2e/core/xtask and all six closing tiers; their actual dependency files
resolve against the producer checkout and current source hashes. An offline
checker initially read stale top-level xtask depinfo; that failed check is retained
and the corrected check uses the byte-identical dependency executable, without
a rebuild. All582 runtime inputs and18 owned/new paths are archived. The full
private-root census conserved all311 baseline profile records and found no
unexpected workload; four new profile directories remain preserved in this run's
private test home with owner markers for the now-absent PID32112. The four real
profiles and default state remained unchanged. Actual command and controlled-child
receipts are retained. The ordered5 archive preserves the gate failure, source
and cleanup. The annotation finding requires disposition before another ordered
sequence; focused native212/220 and successful own closing validation remain owed.
Original tasks/AC0/4 and historical268 limits are unchanged.

### September 25 ordered6 — gates passed after annotation-only repair

The two ordered5 stderr sites received precise explanatory `stderr-ok` comments;
no behavior changed. Static source-invariant and live-test-layout preflights
passed using the exact frozen current-checkout xtask. One subsequent ordered
sequence passed on the same-day verified stable toolchain: formatting made no
changes, strict workspace/all-target Clippy passed, and normal parallel workspace
tests including doctests finished2644 passed,0 failed,423 ignored across37
top-level results. The named nested282 child summary is retained separately and
excluded. No passed focused control was separately repeated.

All582 runtime inputs and18 owned/new paths are archived in ordered6, along with
the exact two-comment delta and12 frozen fresh current-manifest binaries covering
CLI/unit/e2e/core/xtask and all six closing tiers. Actual dependency inputs resolve
under this producer checkout and match current source hashes. The full private
census conserved315 baseline profile records, retained four new test-home profiles,
and found no unexpected process. Real four profiles, preexisting probe.txt and
default state are unchanged; actual waits and cleanup receipts remain archived.
Writer/global workload were released after verification. Prior failures remain
preserved. Focused native212/220 and successful own closing validation remain
owed; original tasks/AC0/4 and historical268 obligations are unchanged.

### Final focused qualification — September 25

Ordered6 passed formatting, strict workspace/all-target Clippy and the complete
normal parallel workspace suite:2644passed/0failed/423ignored, including doctests
and excluding the separately identified nested282 child. The preceding ordered5
failure was the source-invariant guard requiring two stderr annotations; only
two explanatory comments changed, verified by the supervisor. Both static
preflights passed before ordered6. No behavior or assertion was weakened.
All582 runtime inputs and12 actual fresh producer binaries are preserved.

The separately finite original212 and220 native cases each ran once and passed:
home URL/ref flow3.34s, fragment-click readiness2.76s. Both original assertions
and internal budgets remained; both direct test processes were actually waited
with exit0 and no watchdog. Each launch returned its exact owned PID/port/profile
and the new redacted startup observation recorded port_wait/Opened. All319
baseline profiles were conserved, each temporary case profile was removed and
no owned process remained. Source582/Firefox110/real4 stayed unchanged. These
passes do not explain historical startup failures or imply unrecorded daemon
worker returns.

An initial supervisor offline parser wrongly expected a literal ready marker
rather than the implemented Opened enum. Its failed assertion and the second
case's refusal before launch are preserved. The parser was corrected against
the actual enum and raw event, with no repeat of212 and no changed capture.
The successful second case began only after the corrected first qualification.
The changed final implementation is now undergoing its separately admitted
second own closing sweep; no completion result is claimed yet.

## Carry-over

| Finding | Disposition |
| --- | --- |
| Flag-only stop strands full event-queue reader; RPC claim admitted after stop | Closed in this candidate by lifecycle cancellation/admission and actual acquired joins; two old failures and42 qualified controls retained. |
| Optional state overwrites primary, stale ownership on implicit replacement, outgoing optional registry actors remain live | Closed by source-owned snapshots/retirement; three exact before failures, three after passes and15 affected regressions with actual joins. |
| Startup undrained stderr and natural late exit misclassified as port timeout | Closed as demonstrated controlled-child mechanisms by bounded startup observation; two old failures and three final controls, actual waits, and reviewed private-state isolation. This does not establish the cause of any historical Firefox timeout. |
| Closing1 direct166 complete document/status null after21125ms | Fold into [[iteration-203-live-sweep-watch-conditions-third-holder]] as a distinct direct-route null-status occurrence. One original two-leg native pass does not explain it; retained event diagnostics define the next evidence needed. |
| Closing1 home212 old tab URL after an unwaited click | Closed in this candidate's fixture by documented click--with-page readiness; repair-sensitive before/after and negative controls plus original native pass, without changing home assertions. |
| Closing1 fragment220 Firefox77389/62813 did not open its debug port | Fold into203 startup watch; historical cause unknown. Current launcher has independently proved stderr/status corrections and original220 passes once, without attributing that old occurrence. |
| Closing1 wall1400/report579/gap821 timing comparison | Fold into [[iteration-281-ci-submission-handover-timeout-attribution]] diagnostic evidence and203's terminal watch accounting. The trace accounts711.603ms before dispatch and skipped refresh; scheduling/RPC causes remain unknown. Public timing/assertions remain unchanged. |
| Initial Clippy/style, isolation-fixture and source-guard failures | Closed by the reviewed narrow fixture corrections and comment-only annotations; all failed output/source variants retained, final ordered2644/0/423. |
| Historical268 EOF/reset/timeout attribution and original224/240 proof | Remain with [[iteration-268-daemon-pre-auth-connection-loss]], unchanged0/4.284's joined-worker contract is its prerequisite, not substitution for its evidence. |
| Windows startup nonblocking path | Exact-head Windows CI remains required before merge; local macOS validation is not Windows execution. |

## Final local closure — September 25

The changed implementation's second own closing sweep passed all349 exact names
across all six required tiers: CLI338, frame-target1, registry3,61u3, Firefox2
and watcher-protocol2. Both FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1
were set; default6jobs and300s/900s watchdog bounds were retained.

```text
LIVE_SWEEP_SUMMARY executed=349 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=349
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

Actual sweep wait0; owned raw browser actual wait-15 after scoped shutdown.
All319 baseline profiles were conserved and all10 new retained profiles
attributed;341 launch attempts paired, no owned process remained, desktop
identities unchanged. Source582, Firefox110 and real4 stayed unchanged. Eight
actual sweep binaries are frozen under the private closing2 archive. The profile
summary's root is the isolated run FF_RDP_HOME; it is omitted above only for
brevity. Earlier closing1 failures and every failed control/gate remain retained
with their dispositions, not rewritten as passing results.

All enumerated static xtask checks passed, including plan/ref checks for this
plan and the affected203 watch holder. Both dogfood checks explicitly skipped
because neither plan declares a dogfood_script; native proof comes from the
recorded original166/212/220 cases and closing sweep. Hyalo HYALO005 and diff
whitespace checks passed. Fresh independent reviews cover lifecycle behavior,
source retirement, fixture readiness, launch observation and private-state
cleanup; the supervisor verified later comment-only and completion metadata.

Original tasks and AC4/4 now have local implementation/review/gate/closing
evidence. Exact-head GitHub CI remains the publication/merge condition; no
Windows result or remote merge is claimed here before that condition succeeds.
268 remains separate with its original0/4 and original224/240 requirements.

## Exact-head CI blocked — September 25

PR275 head648b2c9f245320d84c78a4523d444721dc91722f failed macOS CI at
`unit_240_goodbye_frame_shares_the_one_client_writer`: round0 decoded six
complete event frames before a400033-byte frame ended early. The job reports
1340 passed,1 failed,4 ignored. Local ordered6 and the own349/349 closing2
remain recorded passes; they do not override this exact-head CI failure. No
merge is claimed. Other CI job dispositions are tracked separately by the
supervisor.

Read-only source investigation established that the test deliberately slows
its reader and discards writer/read errors while demanding unconditional
completion, whereas284's unchanged250ms goodbye budget now includes writer
lease acquisition and retires the shared socket on expiry. The CI log does
not retain the failure detail needed to assign that mechanism to the observed
occurrence. No production defect or scheduling cause is inferred from local
passes. An unapplied test-only repair draft preserves counts/concurrency and
all original framing/completeness assertions, adds outcome diagnostics, and
proposes a separate occupied-lease control with a serialization-bypass mutation.
The exact draft and finite three-execution proposal are private under
ci-goodbye-preparation1. Scoped review and supervisor admission are still
required; no compilation, test or retry ran during preparation.

## CI fixture repair qualified locally — September 25

The first exact-head CI run on648b2c9 finished with nine green checks, including
Windows, and one macOS failure; publication/ci-final1.json records the complete
rollup. The macOS occurrence and its attribution limits remain preserved.
Fresh independent scoped review accepted the paired test-only correction with
zero findings. An earlier reviewer refusal before inspection is retained and
was not treated as a passing review.

The admitted three-execution proof qualified without retries. The new occupied
shared-lease control passed once on unchanged production. A freshly compiled
serialization-bypass mutant failed once at the intended assertion: no latched
writer failure instead of DeadlineExpired with writer-lease detail. Cleanup
and the actual closer join precede that assertion; this was not a compilation
error or watchdog. Exact formatted reviewed source was restored, compiled fresh,
and the original named240 normal-success test passed once with its original
three rounds, dispatcher/payload counts and all strict framing/completeness
assertions. No original slow-reader baseline was repeated. This demonstrates
fixture sensitivity and bounded-close behavior, not the exact historical CI
termination cause or a specific blocked kernel write.

Static source-invariant/live-layout preflights passed, followed by one ordered7
sequence: fmt, strict workspace/all-target Clippy and normal parallel workspace
tests all passed. Totals2645 passed,0 failed,423 ignored across37 top-level
results including doctests exclude the explicitly named nested282 child. One
offline count checker initially missed that child's name because parallel output
interleaved39 lines; the corrected block-based identification used the existing
output with no build or test repeat. All failure bytes remain archived.

All582 source inputs and12 fresh current-manifest binaries are frozen. Only
server.rs's test module differs from648b2c9; its production prefix and the other
581 inputs, including all live tests, are unchanged. Each of the three proof
variants has its exact582-source archive, fresh producer and frozen binary.
The full private census conserved329 baseline profiles, retained four new
private test-home profiles, and found no unexpected process. Real four profiles,
default state and preexisting probe.txt are unchanged; actual waits are retained.

The supervisor's prior own349/349 closing2 result is reused for unchanged
production/live inputs; no new sweep ran. This local repair has not been
committed or published by the execution agent, and exact-new-head CI remains
required before merge. Original criteria and their local evidence are unchanged;
268's separate0/4 and original224/240 obligations remain.
