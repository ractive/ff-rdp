---
title: "Iteration 283: Frame parity across observation windows"
type: iteration
status: done
date: '2026-09-25'
branch: iter-283/frame-parity-integrated-20260925
depends_on: ['272']
first_call_sites: []
dogfood_path: >-
  Preserve the original live_159_daemon_watcher_regression::live_159_frame_targets_survive_the_fix
  assertions, operation order and bounds. Before any new owned-Firefox occurrence,
  declare a finite source-backed schedule with independently reviewed attribution,
  exact source/binaries and actual owned-child waits. Stop answered experiments;
  do not use full sweeps for discovery or repeated passes to erase a failure.
---

# Iteration 283: Frame parity across observation windows

## Evidence and scope

Iteration272's closing1 on September24 failed the original159 frame-survival
regression with daemon1/direct2. Its exact source, binaries and complete log are
preserved under the primary checkout's private
`.git/ralph-loop/20260924-all-open/iter272/closing1/`. A later isolated original159
passed2/2. That passing sample does not explain the earlier mismatch.

The original test navigates, queries daemon frames, queries direct frames and
reads daemon status in successive windows. Connection-local actor strings do not
establish cross-route document identity. Passive records now retain target forms,
operation windows and daemon lifecycle messages when available; missing records
remain unknown, not proof of missing targets or worker return.

272 separately corrected largest-historical-snapshot retention. Its production
helper now retains observed removals, equal-count form replacements and readiness
withdrawal. Three meaningful controls fail before that correction and pass after;
a fourth shows that different event cuts can differ while same-cut sets agree.
That fourth control is not proof of the old1/2 cause and does not replace the
unchanged sequential159 regression. The September25 closing2 reports159 `ok`,
but passing libtest stdout is suppressed; no new occurrence-specific full-body
trace or old-cause conclusion is inferred from that verdict.

This plan owns the remaining parity/observation-window question. Original275's
Guardian consent outcomes and279's navigation timing gaps remain separate.

## Tasks [3/3]

- [x] Map all retained original159 identities and observation windows; enumerate
      unavailable identity/order joins. Distinguish missing evidence from missing
      targets and account for connection-local actor identifiers.
- [x] Declare a finite source-backed investigation and meaningful production-path
      controls for observed add/remove/update events and differing observation
      windows. Preserve original159 assertions, bounds, command order and every
      old/new failure. Independently review and admit any needed native schedule
      before execution, with actual owned-child waits and explicit unknowns.
- [x] Apply only a demonstrated correction to the established cause or contract,
      preserving original frame-survival and count-parity coverage. No readiness
      delay, lifecycle hold, widened wait, blind retry or reduced assertion may
      manufacture agreement. Leave the historical cause unknown unless its own
      occurrence evidence establishes it.

## Acceptance Criteria [3/3]

- [x] The failed1/2 occurrence remains preserved with exact attribution limits;
      the direct/daemon parity contract states identity and observation-window
      requirements, supported by production-path controls detecting real missing
      or stale targets. Unavailable historical joins remain explicit.
- [x] Any correction has meaningful before/after failure and success and passes
      independent review, with original159 assertions and bounds retained. A later
      passing sample alone does not close an unexplained behavioral claim; state
      exactly which cause or contract question the work actually resolves.
- [x] For product/test source changes, ordered gates and this iteration's own
      dual-gate closing pass with exact names and profile reconciliation. Audit
      prose does not replace original required behavioral coverage.

## Out of scope

Reconstructing unavailable historical packets as facts; treating differing actor
strings as distinct documents without connection context; relaxing original159;
absorbing original275 consent or279 timing criteria; interpreting process death
as worker return; declaring272 complete without its own successful closing.

## Carry-over provenance

Filed from272's independently reviewed carry-over proposal. The private review
`iter272/review-carryover/report.md` accepts this separate scope with zero findings;
it does not approve a future test-contract change, native capture or completion.
272 still has unmet closing acceptance after its latest348/349 failed sweep.

## Post272 implementation boundary — 2026-09-25

Begin from merged 272. Its snapshot removal, equal-count replacement and
readiness-withdrawal repairs and controls are reusable, not new 283 work.
First inspect retained 159 operation windows and available identity joins.
A differing-event-cut control establishes a possible contract distinction,
not the historical1/2 cause.

The remaining delivery is an explicit current parity/observation-window
contract and only the additional production-path proof or correction needed
to discriminate real missing/stale targets. Preserve original 159 unchanged.
Do not add another full sweep or native schedule solely to repeat evidence
already supplied by 272.

## Verified 272 completion — 2026-09-25

Iteration 272 merged through GitHub as PR271 at
`e4c3efb0a931860af2f9f0e050e512ce5bd19c9f`, after all ten CI checks passed
on reviewed head `04e0086d`. Its final-source closing sweep passed 349/349
with profile summaries leaked0/unattributed0. This supersedes the earlier
pending 272 closing/publication state above. The earlier347/349 sweep retained
both the159 daemon1/direct2 mismatch and a timing failure; the later348/349
sweep failed only timing. Both failed sweeps remain historical evidence.
Neither the passing closing nor the merge attributes that old frame
mismatch or completes283; its original tasks and criteria remain unchanged.


## Executed contract investigation — 2026-09-25

The owner launched283 from merged main `c25a80c5` after281 closing2 failed
original159 again (daemon1/direct2). This section supersedes preparation-only
language; it does not complete283. Private source/evidence and command receipts
are under `.git/ralph-loop/20260924-all-open/iter283/` in the primary checkout.
`investigation-plan.json` declares the finite hypotheses before the new scripted
control. No native experiment or extra original159 retry was executed here.

### Retained occurrence joins

| Occurrence | Observation and identity evidence | Attribution limit |
| --- | --- | --- |
| 272 closing1 | Original159 reports daemon1/direct2 in the complete sweep log, before operation/full-form diagnostics were added. | No per-route windows or exact document join; later isolated2/2 and closing passes do not establish this cause. |
| 272 diagnostic-native1/case1 | Isolated2/2: daemon PID45130 at1.290–2.289s (812ms internal), direct PID45152 at2.290–3.147s (837ms), status at3.149–3.190s reports live2. Both typed results have top BC11/process45088 and child BC8589934593/process45112, under connections5/6 respectively. | The full stderr and daemon reply forms are retained in the audit. Direct typed forms still lack window IDs; this passing sample neither proves an exact cross-route document join nor attributes the earlier failure. |
| 272 closing2/closing3 and281 closing1 | Each complete sweep log reports original159 `ok`. | Passing libtest bodies are suppressed; no per-operation window or exact document identity is inferred. |
| 281 closing2 daemon | Query PID63455 runs at2.381–3.338s from the test's observation start; internal enumeration809ms. All15 replies contain only top actor `server1.conn6.watcher2.process4//windowGlobalTarget20`, from watcher3; BC11, process63319, innerWindow8589934594, topInnerWindow8589934594. Readiness true, cumulative count2. | No child appears in any of these replies. This alone does not say when Firefox created or announced it. |
| 281 closing2 lifecycle | Daemon PID63345 logs initial about:blank top at07:22:19.969593Z (innerWindow8589934593), its switching destruction at07:22:20.647094Z, then the fixture top at07:22:20.647477Z. Exactly two available forms and one destruction; no child form in this retained handler log. | Cumulative count2 is fully accounted for by two successive tops, not two live frames. The missing child cannot be attributed to local snapshot pruning on this evidence. No upstream wire/emission trace establishes why it was not logged. |
| 281 closing2 direct | PID63491 runs at3.338–5.505s, internal enumeration1996ms. It returns top BC11/process63319 and child BC8589934593/process63424 at `https://example.com/`, with connection7 actor IDs. | The typed result lacks innerWindowId/topInnerWindowId. Same top BC/process/URL supports a tab/location join, not exact document identity across connections. No synchronized delivery cut. |
| 281 closing2 status | At5.505–5.638s: cumulative count2, live count1; dispatcher41/41, idle. | Consistent with the three logged lifecycle events. Dispatcher health is not proof that Firefox emitted the child, nor a complete cross-connection delivery join. |

`occurrence-audit.json` retains the full selected lifecycle forms, all15 reply
summaries and SHA256s of the complete authoritative raw logs. Those raw logs,
source/binary pins and all failed sweeps remain preserved. The native mismatch
cause is still unknown for both occurrences; mere differing windows is not
asserted to explain either. In particular the daemon is still top-only at the
later status, so a simple “child arrived during direct enumeration” explanation
is unsupported by the retained daemon records.

### Current contract and actual production-path control

Each route returns surviving forms at its own observed cut: the daemon's last
snapshot reply, or the direct subscription catch-up plus drain. Direct setup
precedes its settle window; equal settle durations neither synchronize cuts nor
promise future frame completeness. With the same current documents and caught-up
subscriptions, enumeration must agree. A form already observed as live cannot
be omitted; an observed destroyed/replaced document cannot be retained.

Within one connection, actor IDs key actor replacement/removal. Across connections
in one Firefox lifetime, exact raw-form document comparison requires compatible
BC/process/innerWindow/topInnerWindow identities, roles and lifecycle cuts. BC,
process, URL and equal counts alone cannot certify it. Missing raw window IDs
remain unknown. The source documentation now states this contract instead of
claiming unconditional identical results or observation of every future frame.
The unchanged original159 fixture continues to require sequential count parity;
an unexplained failure remains a failure, with its original order and bounds.

`unit_283_frame_parity_requires_current_documents_at_the_same_cut` calls the real
`dispatch_firefox_message`, daemon `frame-targets` response/replay and direct
`enumerate_frame_targets` over a bounded scripted TCP peer. It checks two
successive top documents in the same BC/process/URL, cumulative2/current1, then
retention of a delivered child. The direct handshake delivers known same-cut
forms under different connection-local actor names. The test joins typed direct
actors only to their own scripted raw forms; it does not invent missing native
window IDs. Both outputs must match the independent expected current identities.
A stale alternative passes count equality but fails document identity equality.
The peer observes client EOF and is actually joined; no sleep or extra native
window is used. Existing272 removal/replacement/readiness/different-cut controls
remain unchanged and are reused, not rewritten.

The initial focused control passed1/1 on current production behavior. A temporary
mutation dropping observed non-top-level forms inside the actual daemon handler
failed this control at its delivered-child assertion (0pass/1fail, exit101).
The original bytes were restored and hashed (`mutation.json`). This proves the
control detects a real local omission; it is not a before/after repair claim for
native159. No production behavior was changed because no occurrence-backed
product defect was established. The resolved question is the precise parity
contract and its production-path evidence. Independent review and283's own closing are still required; original AC
text stays intact.


### Non-live validation

Current stable was updated before the ordered final `cargo fmt`, strict workspace
all-targets Clippy and normal parallel workspace tests; all passed. The final
workspace has2567 outer passes,0 failures and421 ignored (raw summaries2568
include the successful nested child-fd test). The first workspace attempt failed
only the daemon-lock source-invariant check because the new test used
`.lock().unwrap()`; the test now follows the existing `.expect("test state lock")`
convention. That failed run and the required repeated final ordered gates are
retained separately. Plan validation and Hyalo HYALO005 also passed. There were
no native tests, Firefox launches, commits or remote actions in this delivery.
The final574 runtime inputs and five actual built/tested executables are frozen
with dep-info, embedded CLI paths and copy/hash verification in `freeze.json`.
Original159 source and original AC wording were verified unchanged.


### Integrated validation and failed closing — 2026-09-25

The three reviewed owned files were preserved while fast-forwarding to merged
main `1df6c864`. Ordered formatting, strict workspace Clippy and normal parallel
workspace tests passed2602/0/421 (one nested child pass excluded). No separate
repeat of the unchanged focused control or mutation was needed.

This iteration's first dual-gate closing executed all349 exact names across six
tiers:347passed/2failed, with zero skipped/preexisting/vanished/launch-timeout/
watchdog-timeout names and profiles leaked0/unattributed0. Original159 frame
parity passed; that does not explain either old daemon1/direct2 occurrence.
`live_159_network_default_source_is_watcher` failed its initial daemon navigate
to Wikipedia after30001ms waiting for document completion; network inspection
was not reached and the target/readiness state is not in this occurrence's log.
`live_navigate_elapsed_matches_wall` returned a correct successful document with
reported447ms versus wall1495ms (delta1048ms). Its trace records483.054ms through
connection and445.922ms from connection to dispatch; these are excluded from
the reported command result. The high contemporaneous load is an observation,
not proof of either failure's cause. Original assertions and bounds remain.

Both failures, all logs and final executable bytes are retained in private
`iter283/closing1/`; no unchanged repeat was admitted. Every344launch-ledger
entry was paired; all10added retained profiles were attributed and the256
preexisting evidence profiles conserved. Full run-root census after cleanup
found266profiles with no unexpected workload. The owned raw Firefox was
actually waited (signal15); protected desktop Firefox was unchanged. This does
not imply native worker return. Closing acceptance remains unmet, noPR exists,
and the exact pending work is separate attribution of these two failures.

## Single network159 attribution occurrence — 2026-09-25

The independently reviewed diagnostic wrapper was applied with formatting only.
Fresh compiler-artifact records identify the current checkout and actual CLI/live
producers. Compilation, layout validation and exact test enumeration passed;
root's first preparation script stopped before compilation on an incorrect test
entrypoint path, retained that failure, corrected the path and reused formatting.
No product behavior, original test assertion, command order or30000ms budget changed.

Root admitted exactly one original network159 invocation with both live gates,
private file-backed diagnostics and a180second outer watchdog. It passed in6.27s:
all six wrapped commands have matched start/spawn/actual-wait-output records,
and all original empty-buffer, watcher, performance-api and final acceptance
markers were reached. Both original navigations returned successfully. No missing
or invalid diagnostic rows were found. The test-child actual wait was0 and the
watchdog did not fire. Actual Firefox/daemon birth identities were observed;
nonchild disappearance remains identity evidence, not waitpid or worker return.
The final census conserved295profiles, added none and found no owned survivors;
575source inputs,110Firefox files and four real managed profiles stayed unchanged.

The one-use experiment is answered and stopped. This instrumented pass does not
reconstruct or explain the earlier network159 timeout and does not erase failed
closing1 or its separate timing-bound failure. Full closure remains unmet; no
unchanged closing sweep is admitted. Raw logs, all six command outputs, producer
pins and qualification are under `iter283/network159-attribution/` in the existing
private all-open run root. Original historical criteria remain unchanged.

## Integration after 281 and 284 — 2026-09-25

The code/document integration checkout is
`/Users/james/.cache/ff-rdp/all-open-20260925-283-integrated`, based on verified
GitHub main `b9efed9d9bcdd800c513a205572da6a5ca1ddc81`. It retains the 262 merge
`c761c1b1f62b3cd9f6bcf56301e6528b1e70fad2` and the merged 281 timing diagnostics
and 284 cancellation/joined-shutdown and launcher observation work. The old
`all-open-20260925-283` checkout at `1df6c864` and its four dirty files remain
untouched. Their exact patch, source archive and hashes are preserved in private
`iter283/integration-281-284-20260925/` alongside the integrated patch and receipt.

Only the three reviewed source-file deltas and this plan were carried forward.
The frame-target contract comments, same-cut control and opt-in original159
diagnostic wrapper retain their previous bytes; no production behavior repair
or assertion, command-order or timeout change was introduced. No unrelated
planning history was copied. The patch applied without textual conflicts.

There is a semantic integration boundary in `daemon/server.rs`: 284 made
`dispatch_firefox_message` a test wrapper over the real
`dispatch_firefox_message_from` primary-source path, with source-specific target
retention and publication. The unchanged 283 control now exercises that path
using the current `test_state` cancellation/queue initialization. Its queries
still use the real daemon reply and typed replay; its direct TCP enumeration
still uses the unchanged core enumeration implementation. It does not cover
optional-generation cancellation or joined shutdown, whose evidence belongs to
284. The 281/284 navigation, launcher, transport and lifecycle changes are kept
as merged, not overwritten by the older checkout.

The earlier same-cut pass, missing-child mutation failure/restoration and
stale equal-count identity control remain useful evidence of the unchanged
test oracle. They do not validate the changed production dependencies at this
new base. Old ordered 2602/0/421 and the failed 347/349 closing likewise remain
historical results. The sole network159 experiment remains consumed and answered:
24 complete diagnostic rows, six stages and six actual CLI waits reached its
original acceptance path in 6.27s. It must not be repeated to infer the old
timeout's cause, which remains unknown.

This integration ran no Cargo, build, tests, Firefox or capture, and made no
commit, push or PR. Fresh independent review of the integrated delta and its
dispatcher/source-publication overlap is needed. Root owns fresh ordered gates,
current xtask checks and this iteration's own admitted dual-gate closing with
exact-name/profile reconciliation; this is final integration validation, not a
renewed discovery schedule. Any new failure requires bounded attribution before
another run. This current integration boundary supersedes the preceding
unchanged-source closing restriction only for validation of the newly merged
dependencies. All earlier failed occurrences, attribution limits and original
tasks/acceptance criteria remain intact and unticked.

## Completed contract delivery — 2026-09-25

Tasks and original acceptance criteria are complete without changing their
wording. The delivered correction is the documented parity/observation-window
contract and its production-path regression coverage. No occurrence-backed
product repair was established or shipped here. The retained missing-child
mutation failure and restored pass establish the control’s sensitivity; they
do not explain either historical daemon1/direct2 mismatch. Both mismatch
causes and the old network159 timeout remain unknown. The single network159
experiment remains consumed; no further isolated discovery run was made.

Fresh independent integration review accepted the exact four-file candidate
with zero actionable findings, including284’s primary-source dispatcher and
publication dependencies and conservation of the original159 assertions/order/
bounds. Existing272 and283 focused evidence was reused only for unchanged
contracts. On verified merged main b9efed9d, ordered formatting, strict
workspace/all-targets Clippy and normal parallel workspace tests passed
2647/0/423 across37 outer targets. One nested282 child pass is separately
recorded. Formatting changed no source bytes.

The required own integrated closing2 passed349/349 across all six exact tiers:
CLI338, frame-targets1, registry3,61u3, Firefox2 and watcher-protocol2. Both
live environment gates were enabled, with the original six-job schedule,
watchdog, timeouts and assertions. Summary: executed349, skipped0, preexisting0,
vanished0, launch_timeout0, timed_out0, total349; profiles leaked0/unattributed0.
Original159 frame parity and network coverage passed. This pass neither
reconstructs suppressed earlier bodies nor attributes their failures.

All582 source inputs,110 installed Firefox files and four real profile
identities/markers remained exact. Twelve frozen workspace producers and
eight separately qualified standalone runtime producers retain actual Cargo
JSON, current-checkout paths, dependency bindings and successful child waits.
All eight actual sweep executable identities matched the qualified binaries.
The355 baseline evidence profiles were conserved; all10 added retained profiles
were attributed, with342 launch attempts paired and zero unpaired records.
The sweep child actually returned0; the owned raw Firefox was waited with
signal15 after recorded owned-group cleanup. No owned survivor or unexpected
process remained, and protected desktop identities were unchanged. Nonchild
absence is not claimed as worker return.

Evidence is under the primary checkout’s private
.git/ralph-loop/20260924-all-open/iter283/integrated-gates1/, including
closing2/root-final-reconciliation.json, tier-reconciliation.json, conservation
and launch-ledger receipts. Sweep log SHA256:
161cdcbcb3e3f610415d72495186c124f322fbea1a5c29361a5fbda8717391aa;
raw-profile archive SHA256:
c5614718b1d64387f7fde14fc209ccf52f84107e0c129616f7d07453c390dfcf.
The earlier failed closing and all source/capture/reader failures remain
preserved. The offline cleanup-reader’s wrong PID-field lookup was corrected
against existing receipts without rerunning a workload.

Affected-plan reconciliation retains275’s separate consent contract and268’s
original attribution requirements.203’s existing terminal watch disposition
receives this qualifying sweep as evidence; it creates no extra sweep or
successor holder. No unrelated iteration’s acceptance is discharged here.
