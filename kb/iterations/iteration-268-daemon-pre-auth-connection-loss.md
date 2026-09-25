---
title: "Iteration 268: diagnose recurrent daemon pre-auth connection loss"
type: iteration
date: 2026-09-13
status: in-progress
branch: iter-268/auth-loss-attribution-20260921
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection -- --include-ignored --exact --nocapture --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise -- --include-ignored --exact --nocapture --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
tags: [iteration, daemon, authentication, testing, carry-over]
depends_on:
  - "284"
---

# Iteration 268: recurrent connection loss during daemon authentication

Filed from [[iteration-255-infobox-facts-refs-and-query-matching]] after the
pre-auth connection-loss watch in [[iteration-203-live-sweep-watch-conditions-third-holder]]
recurred. This is separate from [[iteration-267-daemon-post-auth-timeout-recurrence]]:
that plan owns authenticated request timeouts, whereas both observations below
failed while authenticating. EOF and connection reset remain distinct wire outcomes;
a shared cause has not been established.

## Observations

- The iteration-252 Windows compatibility sweep failed
  `live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise`
  at hop 27/40: `User: daemon auth failed: recv failed: failed to fill whole buffer`.
  Exact isolation passed all 40 hops in 36.44 s. The shared daemon log had no
  reliable failed-occurrence timestamp/PID attribution. Plan 203 explicitly required
  capture and a scoped follow-up on recurrence.
- Iteration 255's unpaused dual-gate sweep failed
  `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection`
  at hop 7/12: `User: daemon auth failed: recv failed: Connection reset by peer
  (os error 54)`. Daemon proxy port 58890; live-launch log records Firefox PID
  97083, debug port 58776, epoch 1789322081. No attributable failed-connection
  daemon auth/request/dispatcher timing was captured. The failure precedes page
  collection, and this fixture contains no fact rows; this does not establish a
  cause or prove a particular server deadline.
- Iteration 255's repair sweep (2026-09-13) failed
  `live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise`
  at hop 16/40: `User: daemon auth failed: recv failed: Connection reset by peer
  (os error 54)`, with zero reconnects. Proxy port 59843; live-launch log records
  Firefox PID 68682, debug port 59766, epoch 1789324143. The shared daemon log
  contains `listening on port 59843, PID 68787`, but no attributable failed-auth
  timing. This is another pre-auth reset, not post-auth plan 267; a shared cause
  with the original EOF or the 224 reset remains unproved. The 224 repeated-hop
  test passed in this sweep; that does not erase its prior failed occurrence.
  Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter255-repair1/sweep.log`
  and `live-launches.log` in that directory.

Current evidence: `.git/ralph-loop/20260912-validation-efficiency/iter255-implementation/sweep.log`
and the exact isolated log and verdict in that directory. Historical evidence:
`.git/ralph-loop/20260912-validation-efficiency/pr247-windows-abort/sweep.log` and
its `isolated-live_240_daemon_frame_desync_and_wedge.log`.

The exact 255 isolation passed all 12 hops with zero reconnects in 15.76 s on
daemon proxy 63286. It does not reproduce the failure or supply the missing
failed-occurrence timing, and does not close this plan.

The repair's exact 240 isolation passed all 40 hops with zero reconnects in
27.46 s (proxy 63802). The original hop-16 reset is preserved, and the missing
failed-occurrence auth/dispatcher timing remains an unticked requirement.

## Tasks [0/4]

- [ ] Capture attributable client and daemon timing for auth receive, auth reply,
      connection lifecycle and dispatcher/RPC-slot state at a failing occurrence,
      including PID, debug/proxy port and monotonic timestamps
- [ ] Explain each observed pre-auth EOF/reset, distinguishing timeout, intentional
      close, process exit and other transport failures; do not infer a common cause
- [ ] Fix the demonstrated mechanism while preserving auth ownership and request
      attribution; do not hide failures with retries or widened timeouts alone
- [ ] Add a meaningful regression test, exercise both named live scenarios, and
      run the required closing dual-gate sweep

## Acceptance Criteria [0/4]

- [ ] A failing occurrence has attributable client/daemon auth and dispatcher timing
- [ ] The demonstrated cause and EOF/reset relationship are documented with evidence
- [ ] The regression fails before the fix and passes after it without weaker assertions
- [ ] Both named repeated-hop tests pass in the closing dual-gate sweep, with any
      remaining connection-loss signatures explicitly preserved and dispositioned

## Out of scope

- Post-auth request timeouts, owned by iteration 267.
- The target-promotion and distinct Sourcepoint consent-action failures, owned by
  [[iteration-262-daemon-live-target-never-promoted]].
- Raising timeout defaults or claiming an isolated pass discharges a sweep recurrence.

## Implementation preflight — 2026-09-19

Coordinate diagnostic boundaries with267: its 'timeout after auth' message only proves the client sent authentication and then waited for a greeting, not that authentication succeeded. Keep EOF/reset and timeout observations separately attributable, without assuming different root causes. Instrument a connection identity before authentication completes. Existing isolated passes do not explain failed occurrences; obtain bounded failed-occurrence evidence before repair, and avoid timeout increases or blind retries.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Iteration263 recurrence — 2026-09-19

Final263 repair sweep on Firefox156.0 failed
`live_240_sustained_hops_never_desynchronise` at hop20/40, zero reconnects,
proxy58186: `daemon auth failed: recv failed: Connection reset by peer
(os error 54)`. All344names reconcile (342pass/2fail), with no skipped or
reclassified tests and zero profile leaks. Attributable failed-auth timing is
still unavailable; no root cause or shared mechanism is claimed. No isolated
pass replaces this failed occurrence. Original tasks/ACs remain unchanged.
Evidence: `.git/ralph-loop/20260919-queue/iter263-repair2/live-sweep.log`,
`reconciliation.json` and `live-launches.log`.

The final263 security-dependency sweep separately failed the same240 scenario at
hop3/40, proxy55319,0reconnects: `daemon auth failed: recv failed: failed to fill
whole buffer`. Keep this EOF distinct from the preceding hop20 reset; attributable
failed-auth timing remains unavailable and no shared cause is proved.
Full344=341pass/3fail, no missing names or profile leaks. Evidence:
`.git/ralph-loop/20260919-queue/iter263-security/sweep.log`.

## Iteration265 sweep recurrence — 2026-09-19

The final265 sweep failed `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection` on hop9, proxy63178: `daemon auth failed: recv failed: failed to fill whole buffer`. This is another EOF observation, not proof of a common cause with resets or greeting timeouts. Attributable failed-auth timing remains unavailable. Full346=344pass2fail, zero profile leaks. Evidence: `.git/ralph-loop/20260919-queue/iter265/live-sweep.log` and `supervisor-accounting.json`. Original tasks/ACs remain unchanged.

## Attributed267 handshake evidence — 2026-09-19

Iteration267 reproduced the scope-table greeting-wait Timeout envelope with
attributable connection timing: daemon13895/proxy56229 rejected its auth read
about88microseconds after handler entry, before client13984/local56297 completed
its auth send. The client then received reset54; the existing transient-error
mapping presented that reset as "timeout after auth". A separate control
confirmed accepted sockets inherit the listener's nonblocking mode on macOS.
The scoped267 repair restores blocking mode before applying existing deadlines;
its explicit nonblocking-socket regression fails without that repair and passes
with it. See [[iteration-267-daemon-post-auth-timeout-recurrence]] and primary
checkout evidence `.git/ralph-loop/20260919-queue/iter267/trace-v2.log`,
`loaded-modules-v2.log`, and `trace-flags.log`.

This demonstrates why presentation alone cannot separate267 and268 handshake
failures. It does not supply missing failed-occurrence traces for this plan's
historical224/240 EOF/reset observations, and does not close this plan or any of
its original ACs. Preserve every original observation and await the required
attributed evidence before assigning those occurrences the same cause.

## Bounded investigation — 2026-09-19

Investigation baseline: `03ba1fd9e2d74193e99b3ff51d4e732b8e7b5337`, containing
the merged iteration 267 repair. No new product change was justified. Temporary
diagnostics recorded client and server socket endpoints, PID, monotonic auth
milestones, shutdown state, dispatcher counters and a nonblocking snapshot of
whether the RPC slot was empty, occupied or locked. No auth token or request
payload was logged. These snapshots were observation only; this did not investigate
iteration 259's request/reply handover or iteration 262's target lifecycle.

The two named repeated-hop tests passed with the merged repair and diagnostics
intact: 2 passed, 0 failed, 43.17 s, with all 12 and 40 hops completed. Removing
only iteration 267's `set_nonblocking(false)` repair, retaining the same diagnostics
and test assertions, also passed the serial pair: 2 passed, 0 failed, 43.84 s.
One six-worker diagnostic batch then ran those modules alongside the established
160/161/164/165/219 load controls: 36 passed, 1 failed, 62.40 s. Both named
224/240 repeated-hop tests passed again. No EOF or reset in either named scenario
was captured. These were targeted investigations, not a closing live sweep.

The mutant batch's failure was
`live_165_eval_call_scope::live_165_wrap_trigger_is_confined_to_declaring_scripts`,
whose greeting-wait Timeout envelope contained an underlying reset 54. The trace
joins client 1170/local 53521 to daemon 853/proxy 53400: handler entry at monotonic
45366.878300916, auth-read error at 45366.879477916 (about 1.177 ms later), intentional
auth rejection at 45366.879522083, and client reset at 45366.893829250. At rejection,
shutdown was false, the dispatcher was alive with 41 frames started and finished,
and the RPC slot was empty. Client auth-send completion was recorded after
rejection. This is an additional controlled observation consistent with 267's
already-demonstrated mechanism, not a newly attributed 224/240 failure. It does
not explain any historical EOF, nor establish that all historical resets shared
that cause. The removed repair was restored; no retry or timeout bound changed.

Evidence and exact command/env/start/end/exit metadata are retained under
`.git/ralph-loop/20260919-queue/iter268/`: `fixed-pair.log`, `fixed-trace.log`,
`mutant-pair.log`, `mutant-trace.log`, `mutant-loaded.log`,
`mutant-loaded-trace.log`, their launch logs and `.meta` sidecars. The temporary
patches are retained there for a reproducible diagnostic setup; product sources
were restored to the baseline and rebuilt (`restored-build.log`).

**Blocked, not complete:** no attributed failing 224/240 occurrence was obtained,
and the missing historical auth/dispatcher traces cannot be recovered from passing
controls. Every original task and AC remains unticked. A new regression or second
fix would be speculative; 267 already supplies its own failing-before/passing-after
regression. A closing 268 dual-gate sweep has not run. Resume on a new attributable
224/240 failure or a concrete new diagnostic hypothesis, preserving the original
EOF/reset observations and acceptance strength. This investigation adds no new
carry-over plan and requests no completion PR.

## Restart plan — 2026-09-19

Follow [[ralph-loop-open-iterations-2026-09-19]]. Recover the docs-only checkpoint
`f5fed086c0a3992b165e7f231856a8990ccd3661`, integrate verified current main on
its branch, and preserve every historical EOF/reset row. The original four tasks
and four ACs remain unmet. Merged267's socket-mode repair is a verified input;
its historical failure cannot be substituted for this plan's named occurrences.

**New work must improve attribution before adding runs (Astra).**

1. Restore/adapt only the retained temporary tracing patch from `iter268/` after
   comparing it to current `daemon/server.rs` and the client auth path. Record
   connection identity on accept, client/server endpoints and process IDs,
   monotonic auth-read/write milestones, greeting-write outcome, close initiator
   and reason, dispatcher state and RPC-slot state. Assign a trace-only identity
   before authentication (and before fallible socket setup); the current normal
   client ID is allocated only after auth succeeds and misses rejected clients.
   Record auth read error/category and decision separately without token content.
   The logger must preserve
   complete records under concurrency; validate that first. Do not log tokens,
   page payloads or unrelated traffic. Keep this scoped to connection setup,
   without retrying259's rejected request/reply investigation.
2. State a differentiating hypothesis before testing. Trace the causal path:
   auth-read failure followed by explicit rejection, independent shutdown/early
   handler exit, or greeting-write failure after successful auth. A failed auth
   read can itself cause an intentional close; those are not competing causes.
   Record the peer-close observation and preceding milestones so an EOF can be
   located within the sequence instead of classified by its error string alone.
   Existing fixed/reverted controls and the incidental165 failure are already
   recorded; do not repeat the267 mutation to manufacture new268 evidence.
3. With instrumentation active, run one exact named224/240 pair on current source.
   If neither fails, one bounded reproduction block may run the same pair with
   the existing six-worker160/161/164/165/219 contention set, at most three batches.
   Preserve all results. Do not change assertions, timeout defaults, ports or
   browser ownership to manufacture a failure. Stop early on an attributable
   named failure and inspect its joined trace before another invocation.
4. If a named failure is captured, distinguish auth rejection, daemon shutdown,
   greeting delivery and later transport framing before changing code. Preserve
   EOF versus reset as separate observations until evidence relates them. Build
   a deterministic regression from the demonstrated cause; prove it fails before
   and passes after the scoped repair, then run both named tests and this
   iteration's own closing dual-gate sweep and ordered gates.

A complete trace with no failure yields a bounded negative result and a preserved
checkpoint, not a completed iteration. A missing trace field yields a precise
instrumentation repair task, not another blind batch. The three-batch allowance is
an investigation ceiling, not a new acceptance requirement. An unresolved failure
outside the two named cases receives its own existing owner or newly filed plan,
without expanding execution scope. All four original ACs remain binding.

## Resumed bounded capture and owner pause — 2026-09-19

Recovered the preserved checkpoint and integrated main
`07e736e9b8567c40f60f819ac4de78cbf106038b`, retaining267's blocking-socket
repair. Astra identified that the old diagnostics omitted
`dispatch::resolve_ref_via_daemon`, the actual source of the historical named
errors. Sol implemented temporary tracing of all three authentication entrypoints,
pre-fallible-setup server identities, auth/greeting/terminal milestones and copied
nonblocking dispatcher/RPC-state observations. No259 investigation, socket-mode
mutation, timeout change or product repair was performed.

Control attempt5 passed:135 records in5 process files with all three routes joined.
Concurrent logging produced exact515+67+67 records, and copied missing-auth-read,
missing-terminal and truncated-record mutations were rejected. Earlier control
attempts and their corrections are retained; attempt4 also passed before the
final refinement. Scope-exit records identify a close decision before destructors,
not a kernel FIN. Cross-process ordering uses socket joins and causal milestones,
not a shared monotonic clock.

The complete allowance was consumed: one exact224/240 pair passed2/2 in37.08s,
then three six-worker batches with the existing160/161/164/165/219 contention
controls passed35/35 each in57.04s,57.10s and56.76s. Both named scenarios passed
in every invocation; no attributed named failure was obtained. These are targeted
diagnostics, not a closing live sweep. General record validators passed with
5256/14289/14297/14293 records respectively. A later stricter all-route check
passed the serial pair and batch1 but rejected batch2 and batch3 because daemon_rpc
connections16251-1 and19452-1 recorded greeting_read_error/reset54 rather than
greeting_read_ok. Preserve these anomalies and their raw traces; neither is
evidence that the named tests failed, nor may every connection be called a
complete successful handshake. Independent review found both resets associated with recorded daemon shutdown
lifecycles, with no accepted-server endpoint join. A listener-destruction reset
is consistent with the records but unproved; exact shutdown initiator and cause
remain unresolved. They use different proxies from the named224/240 scenarios.

Evidence: primary checkout `.git/ralph-loop/20260919-remaining-2211/iter268/`,
including `control-manifest.md`, `logs/` command/env/time/exit metadata, all
`traces/`, final temporary source patch and standalone `trace268.rs`. The
seven pretrace snapshots and tracked baseline preserve restoration inputs; the
temporary test change is preserved in the final patch. Temporary source was
restored exactly to main. The supervisor took over checkpoint cleanup after the
owner requested a safe stop; no further live experiment is authorized by that stop.

**Blocked, not complete:** original tasks and ACs remain0/4. No cause, repair,
regression or closing dual-gate sweep is claimed. Do not reset this consumed
allowance in another session. Resume requires a new attributable named failure
or a concrete new diagnostic hypothesis with a newly bounded investigation.

Independent Astra review (`iter268-review/report.md` beside the evidence root)
returned zero actionable findings against this blocked documentation checkpoint.
Read-only joins verified unique endpoints and required milestones for every
connect_tab/resolve_ref connection, including12/40 named hop refs per invocation.
Before any newly authorized capture, add the omitted worker-return/panic,
Firefox-reader shutdown-reason and explicit shutdown-request hooks; improve the
checker to accept error alternatives, verify causal order and reject ambiguous
process-qualified endpoint joins. Those are precise diagnostic repairs, not
permission to repeat the exhausted batches. Synchronous tracing may perturb timing.
No universal trace-completeness or uninstrumented absence claim is made.
Restored bins/tests build passed; unchanged-source ordered gate evidence was
reused from the independently verified main baseline. Selected-plan validation,
HYALO005 and diff checks passed.

## Additional investigation block — 2026-09-20

The owner authorized one new 60-minute active investigation block with at most six targeted captures; prior exhausted blocks remain preserved. The reviewed proposal distinguished authentication failure from independently initiated shutdown and required all shutdown initiators plus outcome-aware, ordered, unique joins. Source instrumentation and offline checkers were developed through two repair batches. No diagnostic Rust build or targeted capture ran (0/6).

This block stops with known capture-tooling gaps: the mock control still emits the old process-exit schema and contains an invalid shell environment assignment; the live helper's Firefox version/build fields are executable/command identity rather than actual version/build provenance. The two repair batches are exhausted. Do not run these tools or treat offline checker success as a reproduced failure. The final patch, exact source copies, helper/runner tests, reviews and manifests are preserved under `.git/ralph-loop/20260920-additional/iter268/final-instrumentation-archive/`; source was restored exactly to `057f03026e8a2e3eedf1354cd22e584258d10ab3`, verified against the passing 565-file source manifest.

The recorded source/review/handoff debit is 3148 seconds, plus a conservative 90 seconds for the reported post-freeze audit and 60 seconds for supervisor disposition: 3298/3600 seconds. The latter additions are conservative accounting, not measured latency. Remaining time is not a renewed block. Closeout preservation is recorded separately and introduces no investigation or sampling. All original tasks and AC0/4 remain unmet. Next entry requires resolution of the concrete tooling gaps, measured compilation, fresh independent approval, and an explicitly authorized capture schedule consistent with the retained limits; no fresh-session reset or broadened batch. See `instrumentation/repair2-report.md`, `repair1-review.md`, and the supervisor ledger in the same run directory.

## Foundation repair checkpoint — 2026-09-20

The owner explicitly authorized two additional preparation/repair batches and a separate3600-second preparation allowance, retaining the historical3298/3600 discovery debit, six unused capture slots and302 remaining discovery seconds. Verified foundation merge8666a5324905793538c59532e909e0808beee7f3 was integrated above this branch's preserved e111071e checkpoint. No262 correction was imported.

Both new batches are consumed. The first repaired the three known tooling defects: invalid shell environment assignment, obsolete mock-control process-exit schema, and Firefox command identity used in place of actual Version/BuildID/package provenance. It compiled and passed20 offline test methods. Independent review rejected capture readiness for source/checker transition mismatches, incorrect aggregate interruption success, lifecycle gaps, unbounded execution/cleanup, incomplete worker lifetime, and deferred writer timestamps presented as operation timing.

The final batch substantially repaired route-specific chains, aggregate interruption propagation, startup/refusal/spawn outcomes, worker-qualified ordering and explicit deferred emission labels. It passed strict local Clippy, CLI/live/e2e/unit compilation and15 offline methods. These synthetic checker/mock-process tests are not actual Firefox captures, workspace-test completion or regression acceptance evidence. All failed preparation logs and both source/helper/binary freezes remain preserved.

The capture prerequisite is still unmet. The daemon discards worker JoinHandles and can exit while its existing one-second drainer wait is pending. Externally observed process exit can establish interruption; it cannot establish that the worker returned or that all shutdown transitions were observed. No joins, sleeps, timing changes, widened deadlines or weaker original assertions were added to manufacture completeness. A partial auth/shutdown control cannot unlock the reviewed complete-capture schedule. All current capture wrappers and the observer now refuse before launch; the compiled control is ignored and guarded before child creation. No attempt was spent merely to rediscover this known limitation.

Fresh independent review approved only an honest blocked checkpoint and restoration, retaining two additional defects in the archived tooling. First, a worker_outcome can be accepted without requiring its subsequent mandatory shutdown swap; deleting both swap records or process exit between outcome/swap can still produce a false completeness result when another initiator exists. Second, the generic unwired watchdog invokes observe synchronously and gives each PID a new cleanup interval, so it does not enforce one aggregate deadline. It remains explicitly unqualified for capture. These findings do not authorize a third repair batch.

The supervisor restored the eight temporary tracked files and removed only the archived untracked logger using the reviewed guarded restoration. All567 restored source/build inputs match the verified foundation. The original source and every diagnostic generation remain recoverable. Ordered foundation fmt/strictClippy/workspace-test evidence (2499passed/0failed/418ignored) is reusable only for this identical restored documentation checkpoint; it is not new268 acceptance evidence. Restored CLI/live/e2e/unit artifacts rebuilt successfully in7.370s, exit0; actual Cargo JSON and command receipts are retained before commit.

Current tasks and original AC0/4 remain unchanged: no attributable failing occurrence, demonstrated EOF/reset relationship, before/after regression or qualifying closing sweep. No268PR, product repair or main merge occurred. New active preparation debit3058/3600 includes conservative review accounting; compilation mixed with implementation was not subtracted. All302 discovery seconds and six slots remain numerically untouched, but both new repair batches are exhausted and there is no approved capture executor. A new session does not remove these gates or259's restriction.

Evidence lives in the primary checkout's .git/ralph-loop/20260920-foundation-repair/iter268/: repair1/, review1.md, repair2/, review2.md and blocked-checkpoint/. Final review verified568 diagnostic source,76 artifact and4 executable identities, preserved repair1 artifacts, and the exact guarded restoration. The actual restoration manifest is freeze.json; a delegated reference to restore-files.json was stale. Desktop Firefox57827 was preserved; original1112 is not claimed preserved. Actual token usage is unavailable.

## Reviewed attribution checkpoint — 2026-09-21

Under the owner's new268-only investigation grant, four independently reviewed
diagnostic repair phases produced a passive worker-completion observer and a
mock-only executor. One separately admitted mock ran at10:02:50.829–10:03:02.647CEST
(11.817780166s). Native1/1 passed, but the strict occurrence checker rejected
236records across5processes as interrupted. All three worker spawns and observer
registrations succeeded and all six client handlers joined. No body outcome,
supervision epilogue or actual join was recorded for the Firefox reader, grip
drainer or event dispatcher. Authenticated shutdown preceded the main-scope
process-end marker by103.844625ms; that marker is not a kernel-exit or worker-return
timestamp. Worker final positions remain unknown.

Ownership inventory and cleanup completed for all12children; no executor cleanup
signal was delivered and desktop Firefox57827 was preserved. The one-shot claim
is consumed. No retry, named224/240 pair or closing sweep ran in this grant.
No ordinary tooling defect explaining the missing worker evidence was established.
Do not change lifecycle or timing merely to make observation complete. General
Firefox ownership remains unresolved independently of this mock-only executor.
This passing native mock establishes no cause for the original224/240 EOF/reset
occurrences. All original tasks and acceptance criteria remain0/4, unchanged.

Independent evidence review approved the blocked interpretation and exact source
restoration with zero findings. The nine tracked diagnostic Rust files were
restored to merged262/PR264 baseline `c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2`;
only the archived, hash-matched diagnostic logger was removed. The reviewed
source/helper/binary generations and every failed occurrence remain preserved
under the primary checkout's `.git/ralph-loop/20260921-resume268/`, particularly
`implement1–4/`, `review1–4/`, `admission1/`, `mock1/`, `mock1-analysis/`,
`mock1-review/` and `closeout/`. Historical allowances and ledgers are unchanged;
this checkpoint does not reset any grant or complete268.

All569 restored source/build inputs match that baseline. The active CLI/unit/e2e/live
artifacts were rebuilt and identified through actual Cargo JSON; archived admitted
and refused binaries remain recoverable. Ordered updated-stable, format, strict
workspace/all-targets Clippy and normal parallel workspace tests passed:
2529passed/0failed/419ignored. These are restoration/checkpoint gates, not268
regression or live acceptance evidence. Command receipts and exact hashes are in
`closeout/`. No completion PR or merge is claimed.

## Continued partial-observation checkpoint — 2026-09-21

The owner resumed268 under the existing grant. Reviewed temporary diagnostics
added passive worker call-site phases, main exit boundaries and exact product
stop-RPC/escalation/signal attempts. R6 advisory-reader and strict operation-family
integration repairs passed independent review; no product lifecycle, timeout or
retry change was made. One separately admitted100-second mock-only schedule ran
at18:19:34.570–18:19:46.438CEST (11.867929542s), then stopped. Native1/1 passed,
outer exit2. The original strict checker failed on a new stop-RPC operation;
that failure remains preserved. The repaired checker processed the same251records
in5files: integrity pass, ten successful connection outcomes, path interrupted.
All three required workers spawned and registered successfully; six actual
client-handler joins were observed, but the reader, drainer and dispatcher each
lack body outcome, supervision epilogue and actual worker join records.

The final published phases were reader before_firefox_receive/46, drainer
before_queue_receive/6 and dispatcher before_queue_receive/94; the optional
watcher was not entered. These are independently sampled call-site publications,
not final program counters or return receipts. Authenticated shutdown preceded
accept-loop exit by103.311667ms and the main-scope drop marker by104.272417ms.
Four product group-signal calls targeting-79522 (TERM/KILL twice each) returned
-1/errno3. The intended numeric target joins to the owned daemon child, but no
process-group identity, existence, signal delivery or termination is proved by
those failed calls. No cause for a historical224/240 EOF/reset was established.

The new phase/signal questions are answered and this one-shot claim is consumed;
no unchanged retry, Firefox pair or closing sweep ran. Ownership inventory and
cleanup completed for all12children, all absent, with zero actual executor
cleanup signals. The current desktop Firefox census was empty; the historical
PID57827 is not claimed preserved in this continuation. Missing actual worker
completion and general Firefox ownership remain separate blockers. All original
tasks and AC0/4 remain unchanged; status remains in-progress, not complete.

The tested DTrace probes require additional privileges, and noninteractive sudo required a password. No adequate native termination collector has been established in this run; these probes do not prove all native observation approaches impossible.

Independent review approved the strict-checker repair, occurrence interpretation
with the capability wording above, and exact guarded restoration. Nine tracked
Rust paths were restored to merged262/PR264 baseline
`c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2`; only the archived, hash-matched logger
was removed. All569 restored source/build inputs match the baseline, preserving
262/267 repairs. Every diagnostic generation, capture failure, raw record,
consumed claim and historical ledger remains recoverable under the primary
checkout's `.git/ralph-loop/20260921-continue268/`: `implement1–3/`, `review1–3/`,
`admission1/`, `observation1/`, `observation1-analysis/` and `closeout/`.
The capability correction supersedes only the overbroad sentence in the frozen
worker report; the original is retained. This checkpoint does not reset a grant,
complete268 or request a completion PR.

Restored active CLI/unit/e2e/live artifacts were rebuilt and identified through
actual Cargo JSON. Ordered updated-stable, format, strict workspace/all-targets
Clippy and normal parallel workspace tests passed:2529passed/0failed/419ignored.
These restoration/checkpoint gates supply no268 live acceptance or regression
proof. Exact inputs, binaries, command receipts and preservation checks are in
`closeout/`; the consumed admission cannot be reused. No268 completion, PR or
merge is claimed.

## Latest preserved attribution state — 2026-09-23

[[rdp-268-native-attribution-checkpoint-2026-09-23]] records the later
original240 capture:40 hops, zero reconnects, all87 writer ledgers collected
and10085 classified records. All485 per-side handshake outcomes succeeded.
Actual joins covered363 handlers and the Firefox reader; dispatcher/drainer
returns remained unknown. The readerEOF occurred during post-assertion teardown
after a product SIGTERM attempt; kernel delivery and exclusive cause were not
proved. The independent historical265 EOF audit also established no cause.
These outcomes preserve original tasks/AC0/4 and do not authorize an unchanged
retry of a consumed capture.

The guarded18 diagnostic source paths in the execution checkout remain private
and untouched at checkpoint551abe6a715cd5c2ab8ee91058e19f3380d83cb6. Exact
source, binaries, consumed claims and ownership/lock receipts remain under
`.git/ralph-loop/20260921-native268/`, especially `live-attribution16/` and
`final-checkpoint.json`. This plan import does not restore those diagnostics.

## Current-main adaptation — 2026-09-25
Historical 224/240 EOF/reset causes remain unresolved. The September23 original240 capture passed40 hops with zero reconnects; dispatcher/drainer outcomes remained unknown. Neither282's ownership repair nor279's navigation change supplies the missing occurrence attribution. Preserve all consumed claims, diagnostic source and historical ledgers.

The next actionable product work is an explicitly scoped daemon lifecycle prerequisite, separate from historical-failure attribution: own workers and handlers, make every blocking path cancellable, stop admission, wake queued work, and collect actual joins before successful daemon return. Include partial startup, panic, authentication, channel pressure, writer contention and the optional lazy watcher; reconcile its documented loss policy with supervision. Adding joins without wakeup/cancellation is insufficient.

The prerequisite is [[iteration-284-daemon-cancellation-and-joined-shutdown]]. Independently validate and merge 284 as its own iteration before 268 resumes its required scoped validation. Its success must not tick268's historical attribution or regression criteria. Reuse reviewed connection identities and stop-reason ideas selectively; do not reinstall the archived source wholesale or repeat an unchanged passive-capture schedule. After the prerequisite, collect durable attribution during required scoped validation. Original 268 tasks and AC0/4 remain unchanged.

284 repairs cancellation and successful-return guarantees. It neither supplies
an old EOF cause nor discharges the separate original 224/240 validation and
268 closing sweep. Missing historical attribution remains an honest blocker
after 284 if no attributable failure or evidence is obtained.
