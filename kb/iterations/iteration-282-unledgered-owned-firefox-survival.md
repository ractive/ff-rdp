---
type: iteration
status: done
branch: iter-282/unledgered-owned-firefox-survival
date: 2026-09-24
depends_on: []
dogfood_path: >-
  Owned-child cleanup/output and launch-attempt regressions with sensitive
  mutations; Darwin inherited-descriptor child proof; reviewed native 175/96/158 and
  paired 153 ownership evidence; final dual-gate closing 348/348, ordered workspace
  2551/0/420. Earlier failed sweep and limited attribution phases remain preserved.
first_call_sites: []
---

# Iteration 282: unledgered owned Firefox survives a completed live test

## Retained occurrence

Iteration277's named direct-parity test returned1pass in11.81s during
2026-09-24 00:35:15–32CEST. Its successful launch ledger names39455/debug63162.
A broader preflight at02:04 found Firefox39345/debug63135, exact native birth
1790202922.085764, executable `/Applications/Firefox.app/Contents/MacOS/firefox`,
using the exclusive run profile under
`iter277/home-live1/ff-rdp/profiles/ff-rdp-profile-pAqKM8XF4kGV5yfp`.
That retained directory has neither `.ff-rdp-owner-pid` nor `.ff-rdp-owner-test`.
Helper39453 has native birth1790202925.732134 and argv naming39345.

The warm and final-checkpoint process tables already contained these processes,
but earlier summaries checked the successful launch PID and missed them.
The broad complete-cleanup and single-owned-browser-workload claims were false.
The warm test's actual pass remains measured, but its unconditional admitted-control
classification is withdrawn. The browser's effect on scheduling is unknown;
this does not explain277's historical blank-commit failure.

Root revalidated39345's exact native birth, executable and private-profile argv,
then sent only39345 SIGTERM at02:06:23CEST. Browser/helper and the recorded child
rows were absent afterward. Protected desktop14298/14301 retained their exact
native identities; the profile and all evidence remain. Absence after a signal
is not proof of worker return, graceful shutdown or successful joins.

Evidence under primary `.git/ralph-loop/20260923-remaining272-278/iter277/`:
`live1.meta`, `live1.log`, `live1-launches.log`, warm and final-checkpoint process
tables, and `late-owned-cleanup/{before.json,result.json,correction.md,review.md}`.
Preserve every original verdict and subsequent correction.

## Attribution boundary

The launch helper retries and can return on a later success without publishing
prior failed-attempt outputs; the success ledger is written only after successful
status, parsed JSON and a numeric PID. An earlier failed attempt followed by success
is consistent with the evidence, but no retained failure result ties39345 to a
specific branch or ordinal. The launch error, marker absence/removal actor,
missing cleanup action and worker-return state remain unproved.

This is separate from [[iteration-273-contended-launch-output-hang]]: the named
277 test completed rather than hanging. It is also separate from
[[iteration-280-preexisting-profile-disappearance]]: this private profile survives.
Neither shared terminology nor a static source path establishes a common cause.
Do not reopen262,268 or271 execution from this filing.

## Tasks [3/3]

- [x] Audit the retained launch, retry, profile-marker and process evidence in a
      finite source-first investigation, preserving contradictions and uncertainty.
- [x] If additional evidence is needed, declare a finite owned reproduction before
      execution and account for every launch attempt, including failed attempts,
      rather than only the final successful PID.
- [x] Repair only an established cleanup/accounting defect with meaningful regression
      proof, or record the bounded unresolved outcome and exact missing evidence.

## Acceptance Criteria [3/3]

- [x] The original orphan and erroneous cleanup summaries are retained; attribution
      has a concrete evidence chain or an explicit bounded unresolved disposition.
- [x] Any repair accounts for all affected owned launches and preserves private
      evidence and unrelated browsers; no broad kill, blind retry or inference of
      worker return from process death substitutes for actual observations.
- [x] Any implementation has repair-sensitive regression proof, ordered workspace
      gates and its required closing validation, with every failed/invalid result
      and cleanup limitation preserved.

## Original filing boundary (historical)

Planning only.282 is outside the authorized272–278 execution queue. No new live
capture, product/harness repair or allowance is authorized by this note.277 remains
incomplete at checkpoint62fda89da4d51d2ff277d11be94c777aad26dbc6 with originalAC0/3.

## Authorized implementation — 2026-09-24

The owner's subsequent all-open-work grant authorizes this implementation. The
original filing and retained277 evidence above remain unchanged as history.
Work is isolated on base00403995e299b736042d5b554772bbb8f8182308; no277 source or
280 plan is imported. Private evidence is under
`.git/ralph-loop/20260924-all-open/iter282/` in the primary checkout.

The finite audit read the original launch command/status/ledger, the warm and
final process rows, native before/after cleanup receipts, correction/review,
and current launch/profile/guard/retry source. The original39345 survivor and
incorrect broad cleanup summaries are established; its failed launch ordinal,
error and marker-removal actor remain unresolved. The missing observation is
the original failed attempt's command status/stdout/stderr plus its owned child
status and cleanup actions. Later source reproductions do not supply that
historical observation.280's disappeared baseline profiles are a separate issue.

Two concrete cleanup defects were reproduced with direct owned child processes:

- A child-status query error removed the managed profile without stopping or
  waiting for the still-running child.
- A real invalid jq expression failed output after the running child's ownership
  had been released, leaving that child behind a failed launch command.

The repair retains a cleanup guard until output succeeds, including unwinding
from an output panic. Status/probe/output failures
share scoped stop and actual child wait before profile cleanup. If stopping or
waiting fails, the error retains a warning and leaves the profile as evidence.
Only the child's verified independent process group is eligible for group
signalling; an injected spawner in the caller's group does not authorize that
group. A caller-supplied profile remains intact.

The harness records a start row before spawning and preserves every command
outcome, including exact stdout/stderr bytes, in a sibling
`live-launches.attempts.jsonl` file (derived from `FF_RDP_LIVE_LAUNCH_LOG` when
set). Failure to open/write the start row prevents spawning. An incomplete
start or outcome-write error is an explicit evidence gap, never cleanup proof.
The former unconditional three-attempt retry is removed: one launch failure
fails the test instead of being hidden behind a subsequent success. Successful
PID records remain compatible with the existing ledger. This is launch-command
accounting, not a claim that the ledger inventories descendants or proves
Firefox worker returns. Independent process/profile census remains necessary.

## Validation in progress

`gates/before.log` restores the two original failure branches with the new
owned-child tests: both fail with `waitpid(WNOHANG) == 0`, then test rescue
performs scoped kill and actual wait. `gates/after.log` passes three tests,
including a caller-profile and unrelated-child preservation control. `ECHILD`
after the product call verifies the direct child's status was collected; it
does not certify any Firefox worker epilogue or join. Two Firefox-free ledger
tests preserve failed/successful outputs and reject an unwritable ledger before
spawn. The final workspace also exercises the additional unwind regression.
The initial invalid `--lib` command, first strict-Clippy failures and the
stdout-evidence source-scan failure on the new byte-preservation assertion are
retained; the latter was corrected by including the complete stdout/stderr row
in that assertion's failure diagnostic. Ordered stable update, fmt, strict
workspace Clippy and normal parallel workspace tests passed in `gates/v3-*`:
2544 passed,0 failed,419 ignored across37 summaries. Native focused validation,
independent review and this implementation's closing dual-gate sweep remain
pending; no completion or merge is claimed. The finite two-test qualification
schedule and source/binary pins are in the private evidence root.

## Initial review correction

Initial independent review found no further actionable product defect, but
rejected native admission because the selected175 failure test used an
auto-deleting home outside the retained occurrence root, exempted live-owner
profiles, and discarded nominal-pass command output. No native occurrence ran.

The scoped repair keeps the test's actual home on every path. With a capture
ledger, its exclusive0700 home is created beside that ledger and printed for
the supervisor. Its direct launch now records exact raw output/status, including
the command's overridden home rather than only the parent environment. Every
surviving managed profile fails the test regardless of marker/liveness, and
assertion unwinding preserves the home and profile bytes. Browser-free controls
exercise live-owner, dead-owner and unmarked survivors, retained raw outcomes,
actual-home reporting, private mode and unwind preservation. Private `repair1/`
preserves the reviewed prior candidate, every repair failure and the revised
proof/gates. Source/binary/schedule pins require fresh scoped review before any
native admission; the original tasks/criteria and historical attribution boundary
remain unchanged.

Repair1's four focused282 harness controls pass. Restoring the live-owner
exclusion and parent-environment home reporting makes the two new controls fail
at those exact contracts (`repair1/mutation2.log`); the earlier draft/mutation
results are retained separately and are not sensitivity proof. Final ordered
stable update, fmt, strict workspace Clippy and normal parallel workspace tests
passed:2546/0/419 across37 summaries. Plan validation and Hyalo schema lint pass.
No native occurrence or closing sweep has run; fresh scoped review remains due.

## Focused native controls and failed closing — 2026-09-24

Fresh scoped repair review returned zero actionable findings. Root admitted and
ran the two scheduled controls exactly once: original175 passed1/1 in0.55s and
original96 passed1/1 in1.15s. Each runner collected actual exit0; direct launch
outcomes were the expected error1 and success0 respectively. Exact raw outputs,
actual homes and qualification receipts are retained in `native175/` and
`native96/`. Their browser lifetimes were too short for the external collector
to sample native birth before exit; the96 during snapshot was post-test. Neither
result is a Firefox worker-return claim. Baseline13 profiles were unchanged and
protected desktop identities were preserved. These two occurrences are consumed.

One required final-source closing dual-gate sweep then ran with the standard
six jobs,300-second stall watchdog and900-second build bound in an exclusive
private home. It failed:343 passed,3 failed and1 timed out of347 qualified tests.
The CLI tier supplied332 passes,3 failures and1 timeout of336; the five core tiers
passed1,3,3,2,2 respectively. The failures were
`live_153_replace_double_envelope::live_153_replace_emits_single_envelope`,
`live_153_replace_double_envelope::live_153_replace_reports_launched_pid` and
`live_153_replace_double_envelope::live_153_replace_reports_stopped_instance`.
`live_158_launch_lifecycle::live_158_launch_survives_contended_bind` produced no
verdict before the watchdog killed the CLI phase. Buffered153 panic diagnostics
were not published before that kill; their exact failing assertions remain
unknown. The retained158 sample places a worker in `Command::output` reading
output and the test thread in a join. It does not identify the open pipe writer,
collect an actual worker return, or establish the historical273 cause.

`closing1/sweep.log` records executed346,timed_out1,total347 and the post-watchdog
profile metric leaked0/unattributed0. The watchdog reports cleanup of four managed
Firefox PIDs35414–35417; this is not an actual child wait or worker-return receipt.
The supervisor separately collected sweep exit1 and, after scoped group TERM,
actual raw Firefox child wait status-15. Its positively identified raw browser
used an unmanaged retained profile and private process group. After cleanup the
independent census found no unexpected workload, preserved protected79047/79051
native births, and found all13 baseline profiles unchanged plus9 new retained
managed directories. Four new directories belong to the hung158 test; all nine
remain evidence. The unmanaged raw profile also remains, with a recovery archive.

Preflight verified568 source pins,4 binary pins,3 document pins and110 installed
Firefox package entries. Post-sweep all568 source pins still match. The sweep's
build/list phase replaced the CLI/live executables (mtime18:19:21CEST), so their
post-sweep hashes differ from the preflight binaries; both manifests are retained.
No claim of byte-identical focused-native and closing binaries is made. Product
and test source were unchanged throughout closing. `closing1/reconciled-outcome.json`,
`post-sweep-pins.json`, raw logs, watchdog samples and before/after census receipts
preserve the complete outcome and limitations.

Closing is unsuccessful and no further sweep, capture, source repair or completion
is claimed. Original criteria remain1/3. The remaining check-* and final ordered
commit gates have not been run after this failed closing; prior passing repair1
workspace gates remain historical validation of the frozen source. Next admission
must discriminate the153 raw failure and158 output-pipe owner, preserving actual
child waits and worker joins, before any focused repair or new closing validation.
Historical39345 attribution remains unresolved on the original evidence.

## Repair2: inherited Darwin descriptors

Root's finite closing1 observation and independent spawn audit additionally prove
that libtest and owned Firefox35414/35415 plus crash helpers35426/35427 held the
same extra pipe endpoints above standard streams. The saved test158 sample shows
an output read and a join wait. The exact sampled read descriptor, allocation race,
historical272/273 causes and three153 failure causes remain unproved. Evidence is
retained in `root-observation1/`, including the independent `spawn-fd-audit.md`.

The Firefox command now installs a macOS-only child descriptor preparation step:
post-fork `/dev/fd` enumeration marks every extra descriptor close-on-exec while
preserving configured0/1/2, the process group, Rust's exec-error pipe and actual
`Child`/`PendingLaunch` ownership. Enumeration and flag-setting errors abort spawn
with their OS error; no fixed fd ceiling or silent fallback is used. The callback
uses stack storage and syscalls, without allocation or locks. This changes the
macOS standard-library spawn path to fork/exec and requires usable `/dev/fd` and
Darwin getdirentries64. Other platforms and daemon spawn are unchanged. The pinned
close_fds0.3.2 implementation was inspected and rejected because its fallback caps
fd enumeration and discards flag-setting errors; no dependency was added.

A finite isolated subprocess proof creates inheritable fd1100, then lowers its
soft fd limit to64. After a positive post-exec control handshake, the production
helper yields immediate pipe EOF while the child still answers ping and remains
in its own group. The child exits on explicit completion and its actual status0
is collected; intended stderr is preserved. A missing executable still yields
ENOENT through spawn. Disabling only the descriptor helper yields no EOF while
the child remains responsive, then normal completion and actual wait0 precede the
specific failed EOF assertion. This establishes the missing descriptor-exclusion
contract without assigning every current or historical hang to that contract.

All draft outcomes are retained in `repair2/`: the initial high-fd fixture first
needed its isolated soft limit raised, and the initial mutation exposed matching
control/poll deadlines. The corrected protocol uses a zero-time EOF poll after
positive exec readiness; it does not widen a wait or use child death to force EOF.
`focused3.log` and `mutation2.log` are the repair-sensitive pair. The source and
actual binaries used for failed closing1 are preserved before this repair. Fresh
repair2 review and admitted native validation are still required; closing1 remains
failed and original acceptance criteria remain1/3. No new native occurrence ran.

## Repair3: preserve the four158 command outcomes

Fresh repair2 review found no actionable product defect, but blocked native158
admission because its four direct output calls bypassed the reviewed recorder.
Completed sibling outcomes stayed in memory until all joins; timeout could lose
even their exact bytes. No native158 occurrence was spent.

Each worker now uses the existing recorder with unchanged piped `Command::output`
semantics. Four separate sibling ledgers derived from the capture-ledger path,
`.158-0.attempts.jsonl` through `.158-3.attempts.jsonl`, provide one writer per file
without serializing launches or risking interleaved partial appends. Threads carry
the original test name; ports7101–7104, four concurrent launches, actual collective
joins, original assertions and timeout bounds are unchanged. Starts precede spawn;
exact status/stdout/stderr outcomes precede worker return and collective joins.
Unfinished calls retain unmatched starts, explicitly requiring supervisor recovery
and never implying worker return from process absence. Existing recorder regression
coverage applies unchanged; no test merely mirrors this call-site wiring.

`repair3/` preserves repair2's exact source and four actual binaries, the scoped
repair and its gate receipts. Root's native158 runner and the separate private153
attribution runner remain frozen. Fresh scoped review and native admission still
precede any occurrence; closing1 remains failed and original282 criteria remain1/3.

## Repair4: registry PID is not Firefox ownership

After fresh scoped reviews, root ran native158 once:1/1 passed in2.04s,
actual libtest wait0, four durable command outcomes and positive native browser
identities. All34 baseline profiles stayed unchanged, four new profiles remain,
and the independent census found no unexpected process. This does not identify
the exact closing1 read descriptor or retrospectively prove worker returns.

The single reviewed private153 diagnostic then failed at replacement, after
successful initial launch, tabs and eval. Its raw replacement CLI24650 exited1
and reported “stopped Firefox (pid24632)” while debug61053 remained listening.
Positive native records prove24632 was the ff-rdp proxy daemon and24607 was the
Firefox using the exclusive outer-home profile. The registry home contained the
proxy record and no launch record; the outer home retained the Firefox launch
record. Exact command outputs, records,26 cleanup actions and actual direct waits
remain in `attribution153/native1/`; root's complete census preserved protected
births and all38 baseline profiles plus the one newly retained profile. The
runner remained unqualified. This is current attributed failure evidence, not an
original153 test pass or the cause of every historical153/39345 occurrence.

The registry stop path could substitute `DaemonInfo.pid` for an unavailable
ownership-verified Firefox listener. That sent the proxy through Firefox's port
and tree escalation and mislabeled it in the error. The repair removes this
fallback: only a listener passing the existing ownership check can receive the
Firefox ladder; otherwise the unchanged bounded port check yields an explicit
ownership refusal. The separate proxy shutdown path, recycled-PID guard, launch
record preservation, actual launch-child cleanup and PR264/262 repairs remain.
A typed-registry regression with recording signal hooks passes; restoring the
old fallback makes it fail on forbidden proxy tree escalation. Stubbed signals
are not actual child waits or worker-return evidence.

Original153 run_raw commands now retain exact status/stdout/stderr through the
reviewed piped recorder before assertions, and each separate private registry
home survives success, failure and unwind. Initial outer-home launch/readiness,
two-home topology, eval/replacement order,10000ms command timeout and every
original assertion are unchanged. No native occurrence is run by this repair.

An unresolved prerequisite prevents claiming153 success: ownership_scan_roots
checks the current override and default roots, as iter188 intended. With both
homes private overrides, the inherited outer launch root is neither searched
root when replacement runs in the registry home. A proxy registry does not
transfer Firefox ownership. The corrected guard therefore still refuses this
topology; no other-home scan, marker transfer, default-root launch, assertion
weakening or topology change is used to force a pass. `repair4/native-proposal.md`
records a conditional one-per-original-case schedule with stop on first failure,
but no occurrence is admissible until root reconciles that ownership prerequisite.
Closing1 remains failed; original tasks/criteria remain1/3 and all earlier
failures/attribution limits are preserved.

Repair4 final-source ordered gates passed with stable1.98.1: fmt, strict workspace
Clippy and normal parallel workspace2548/0/419 across37 outer summaries. The
additional nested isolated-child summary is not double-counted. Existing registry
controls4/4, launch-record ownership controls6/6 and recorder/home controls4/4 pass.
Two initial focused selectors matched zero tests and are explicitly excluded from
proof; corrected selectors and the specific failing mutation remain retained.
The first Clippy rejected a two-arm match style; the equivalent if-let correction
passed a fresh ordered sequence. No failed outcome was discarded. This paragraph
records completed evidence only; no product/test behavior changed after those
gates, so no duplicate workspace run is charged solely for this status update.

## Repair5: prospective paired153 ownership contract

Root adopted the independent historical ownership audit and an explicit decision
under the owner's all-open implementation/refactoring grant. This supersedes
repair4's pending topology-decision note, not its historical failed results.
Default-root Firefox plus override registry remains deliberately supported by
iteration188/commit1798f119. The failed two-override topology lacks that same
ownership evidence. There is no universal cross-home prohibition, and no proxy
connection supplies authority to terminate an otherwise unproven browser.

The three active original153 test names and their original success assertions
remain: `live_153_replace_emits_single_envelope`,
`live_153_replace_reports_launched_pid`, and
`live_153_replace_reports_stopped_instance`, in the existing
`live_153_replace_double_envelope` module. Their prospective fixture now launches,
evaluates and replaces in one fresh private configured home. It archives the
exact fixture-generated per-port launch-record bytes and removes only that
active lookup to exercise registry fallback. Before replacement it requires a
matching native browser birth and real managed markers/start token in the trusted
configured root, an actual listener observation, no active launch record, a real
daemon eval route and a distinct native proxy identity. No marker or token is
changed. Every original full-buffer JSON/new live distinct PID/exact prior PID
assertion and timeout remains, with an additional prior-incarnation-ended check.
This models a missing launch record; it is not a pass of the old invocation.

A fourth ignored live case,
`live_153_replace_refuses_unproven_outer_override`, keeps two separate fresh
private override homes. It changes no ownership artifact and requires real
launch/eval setup, nonzero replacement status, a complete truthful ownership-error
envelope, and the same original Firefox birth/profile/listener plus byte-identical
record/markers before independently birth-validated fixture cleanup. Proxy and
browser cleanup authority are recorded separately. Always refusing fails the
positive cases; bypassing ownership fails the negative. The three historical
closing153 failures and native1 failure remain failed and archived.

All direct launch/tabs/eval/replace commands keep piped capture and retain exact
status/stdout/stderr before assertions, in one-writer ledgers inside each retained
home. Cleanup refuses unknown or changed incarnations; native query failure alone
is never death or worker-return proof. The new fixture guard has browser-free
mismatched- and matching-birth owned-child controls with actual child waits/joins.
Record archival checks refuse changed lookup bytes. The registry role regression
now covers both legacy and confirmed proxy identities; the old fallback mutation
fails forbidden proxy tree escalation. Existing153 full-buffer parser proof and
no-nested-print coverage remain applicable to the unchanged success assertions.

Private `repair5/` preserves repair4 source/binaries/docs and the accumulated
repair3-to-repair5 delta. Dated additions to DEC-045 and153/188/191/192 retain all
historical text and criteria. The explicit four-case native schedule requires
fresh independent review, pinned source/binaries/runner/runtime and complete
supervisor census, one occurrence per case and stop on first failed/unknown
result. No native or full sweep has run in repair5. Active live coverage adds one
negative case; no historical count is rewritten. Original282 criteria remain1/3;
a successful final-source closing sweep, outcome review and remote gates are
still owed before completion.

Repair5 final ordered stable1.98.1 update, fmt, strict workspace Clippy and normal
parallel workspace tests passed:2551/0/420 across37 outer summaries. The raw38th
summary is the isolated child-fd fixture already counted by its parent test.
All nine original positive assertion blocks and the full-buffer parser block
are byte-identical; browser-free enumeration finds all four active live names.
The initial private-helper compile failure, two Clippy style errors and the
launch-ownership source-scan naming failure remain retained with their exact
failed source/logs. The guard helper now uses the scanner's existing preferred
`guard_launched_firefox` name; no scanner exception or assertion change was added.
Every required repeated gate followed a concrete failure/correction; no native
retry or sweep ran. This status paragraph changes no executable/test contract.

## Reviewed native evidence and conservation corrections — 2026-09-24

This dated addition preserves the earlier plan body, original task/criterion
wording, every failed occurrence and each historical admission limit. It updates
the subsequent evidence; it does not retroactively complete a refused phase.
Private paths below are relative to the primary repository's
`.git/ralph-loop/20260924-all-open/iter282/` evidence directory.

Repair 5's independent review returned zero actionable findings. The production
repair retains the distinction between a proxy registry PID and an
ownership-verified Firefox listener. The three positive 153 fixtures now exercise
genuine owned registry fallback in one configured private home, preserving the
original success assertions and command bounds. The separate negative fixture
keeps two distinct override homes and requires truthful refusal plus unchanged
original-browser survival. Supported default-root Firefox plus override registry
remains supported; this is not a universal cross-home prohibition. The historical
two-override failures, original assertions and ownership audit remain intact.
See [replace-contract-decision.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/replace-contract-decision.md), [replace-ownership-contract-audit.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/replace-ownership-contract-audit.md) and
[review-repair5/report.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/review-repair5/report.md).

The admitted native 158 occurrence passed 1/1 in 2.04 seconds. Its actual libtest
wait was 0; four concurrent command outcomes were durable before the original
joins, and all four Firefox births, native images, private profiles and ports
were positively observed. [native158/qualification.json](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/native158/qualification.json) and its raw ledgers
retain that proof. It does not identify closing1's exact sampled descriptor or
prove the cause of every historical output hang or any Firefox worker return.

Independent evidence review then found that the original census read the unused
`.ff-rdp-owner-start-time` spelling and stripped PID/test text. Consequently ALL
earlier private-profile equality claims using that collector—including focused
175/96, closing1, native158 and the original153 diagnostic—establish only the
recorded paths/PID/test strings, not exact conservation of all actual marker
bytes. No evidence says a marker changed; missing bytes are not reconstructed
from current metadata. Separately captured exact real-profile bytes, native
process observations, command outcomes and actual waits remain independent.
[census-marker-conservation-limits.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/census-marker-conservation-limits.md) governs this correction.

The additive v2 census preserves the old observer and captures the actual
PID/test/start marker names, exact bytes/hashes, directory identity and explicit
absence/read-error states. Four browser-free controls passed; the wrong-spelling
mutation failed as required. Errors remain durable refusals. Independent reviews
of this collector and the later explicit evidence-scope decisions returned zero
actionable findings. No observer error was converted into a complete snapshot.
See [census-repair1/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/census-repair1), [review-census1/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/review-census1), [census-repair2/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/census-repair2) and [review-census2/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/review-census2).

The subsequent 153 results are separate occurrences and evidence scopes:

| Occurrence | Measured result | Qualification and retained limit |
| --- | --- | --- |
| Original private diagnostic | Replacement CLI exit1 after successful launch/tabs/eval; proxy24632 mislabeled as stopped Firefox while browser24607 retained the listener | Attributed wrong-PID selection, not an original153 libtest pass or universal historical cause. Exact outputs/identities and26 cleanup actions remain in [attribution153/native1/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/attribution153/native1). |
| First positive case1 phase | PASS 1/1,4.09s; actual libtest wait0; five CLI outcomes0 | Fixture native evidence satisfies the declared missed-sampling provision. Exact51-profile start-marker baseline was never captured; conservation remains LIMITED. Phase closed at1 spent/3 unspent. [review-case1-evidence/report.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/review-case1-evidence/report.md). |
| Corrected-collector phase2 case1 | PASS 1/1,2.86s; actual wait0;52 old profile rows exactly conserved,53 after | During4 retains PID971/PPID842/PGID842 but native birth/path0/ESRCH. Its individual incarnation/image/cause/return remain unknown. Strict phase closed at1 spent/3 unspent. [root-native153-paired/phase2-case1-outcome.json](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/root-native153-paired/phase2-case1-outcome.json). |
| Remaining case2: launched PID | PASS 1/1,2.93s; actual wait0; five matched CLI outcomes | All three named roots positively observed, seven recorded snapshots without errors,53 old profiles exactly conserved. Accepted at that finite scope, not continuous coverage. [root-native153-paired/remaining-case2-outcome.json](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/root-native153-paired/remaining-case2-outcome.json). |
| Remaining case3: stopped-instance metadata | PASS 1/1,2.89s; actual wait0; five matched CLI outcomes;54 old profile rows exactly conserved | During4 lost current identity for original49350,proxy49424,helper49352 and six members. No current root bracket; strict phase refused and closed at2 spent/1 unspent. [root-native153-paired/remaining-case3-outcome.json](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/root-native153-paired/remaining-case3-outcome.json). |
| Separate final negative case4 | PASS 1/1,10.32s; actual libtest66721 wait0; four successful setup outcomes and one expected ownership-refusal exit1 | Native original66737/birth1790278217.013715 survives before/after refusal on listener53949 with unchanged record/markers; distinct proxy66774/birth1790278218.025320 is in the other home. All55 old profiles exactly conserved,56 after. Accepted under the reviewed fixture-native scope. [root-native153-paired/negative-case4-outcome.json](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/root-native153-paired/negative-case4-outcome.json). |

Fresh independent review accepted reuse of case3's precise positive facts for
the original 282 obligations without qualifying its failed phase. Original/proxy
identities were observed before sample4; replacement49529 was born after4 began
and has positive fixture native birth/private ownership before cleanup. Its
external image/argv remains unobserved. It is not honest to claim every inherited
observation goal was already met at3; no exact evaluated stop predicate was
retained. The binding matrix and review are
[case3-evidence-reconciliation/evidence-matrix.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/case3-evidence-reconciliation/evidence-matrix.md) and
[review-negative-scope/report.md](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/review-negative-scope/report.md).

For case4, root adopted one occurrence with unchanged fixture/runner/CLI pipes,
assertions and lifecycle bounds, strict before/after v2 boundaries, and zero
optional global during censuses. Native image/argv and continuous internal-worker
coverage were NOT SCHEDULED; actual fixture native stage observations remained
mandatory. Birth-validated individual cleanup and the clean after boundary were
retained. These detached-process observations are not actual-child waits or
worker-return evidence. The first root reconciliation incorrectly required empty
outer stderr; its only contents were two expected retained-home announcements.
All direct CLI stderr was empty. The invalid check is preserved in
`negative-case4-root-check-draft-failure.json` under [root-native153-paired/](/Users/james/devel/ff-rdp/.git/ralph-loop/20260924-all-open/iter282/root-native153-paired);
no test rerun, source assertion change or weakened result followed.

The four named 153 behavior contracts now have reviewed, attributable evidence
across these separate occurrences. This does not turn them into four consecutive
qualified runs or erase any old failure, unknown identity or missing baseline.
Historical 39345's failed ordinal, error, marker-removal actor and worker returns
remain unresolved. No later pass supplies those missing observations.

## Final-source closing — 2026-09-24

The required closing sweep ran once on the final reviewed source with both
`FF_RDP_LIVE_TESTS=1` and `FF_RDP_LIVE_NETWORK_TESTS=1`, default six CLI jobs,
300-second stall and 900-second build bounds. Actual sweep wait was 0; every
qualified name has one passing verdict:

| Target | Passed | Failed / timed out / skipped |
| --- | ---: | --- |
| ff-rdp-cli / live | 337 | 0 / 0 / 0 |
| ff-rdp-core / live_129_frame_targets | 1 | 0 / 0 / 0 |
| ff-rdp-core / live_61p_registry | 3 | 0 / 0 / 0 |
| ff-rdp-core / live_61u | 3 | 0 / 0 / 0 |
| ff-rdp-core / live_firefox_test | 2 | 0 / 0 / 0 |
| ff-rdp-core / live_watcher_protocol | 2 | 0 / 0 / 0 |

`LIVE_SWEEP_SUMMARY executed=348 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=348`

`LIVE_SWEEP_PROFILES leaked=0 unattributed=0`

All 56 preexisting private profile directory/marker observations compare exactly;
65 remained afterward, including nine new retained profiles. Eight match their
exact recorded command test/PID/profile outcomes. The ninth belongs to the
unchanged direct-launch target-destroyed test: retained product launch record
52002 matches PID97621, birth token1790278808.674080, exact private profile and
test marker. That older helper does not retain raw CLI output. This distinction
is explicit; a product record is not an invented command-output receipt or
external native-image observation. Across the instrumented helpers, 342 starts match 342 output records in 17 ledgers: 342 unique
(ledger path, identity) pairs, with 341 distinct bare identities. One bare
identity occurs in two separate files with matched outcomes. No extra test
diagnostics were exposed in the sweep log. Passing-test stderr is suppressed
by the runner, so this does not prove execution of the target-destroyed test’s
final navigation assertion. Its recorded pass and launch attribution are the
accepted facts; no missing raw output or later assertion coverage is invented. The first root reconciliation
incorrectly assumed every added profile used that recorder; its failed check is
preserved in `closing2/root-added-profile-check-draft-failure.json`.

The four real profiles and protected desktop identities were unchanged. The
owned raw Firefox 72608/birth1790278499.616711 received scoped group TERM and had
actual child wait -15; the recorded raw group was empty, with no unknown member.
Its exact profile and archive remain private (archive SHA256
`7b739caabb308ba78fcd58e59ea804d53ebd300eeca207aed0b0f2c26f489fb6`).
The final v2 boundary was complete, with no errors or unexpected process.
Detached process absence is not a worker-return or graceful-shutdown claim.

All 571 source/build and 13 then-frozen document inputs remained unchanged.
The sweep build replaced the CLI/live executables; their old/new hashes and
APFS-cloned swept binaries are retained in `closing2/post-build-pins.json` and
`closing2/post-sweep-binaries.json`. This is not a claim that the earlier focused
binary hashes remained unchanged. Complete receipts, all six named partitions,
profile attribution and conservation are in `closing2/tier-reconciliation.json`,
`command-ledger-counts.json`, `new-profile-attribution.json`,
`baseline-conservation.json`, `cleanup.json` and the root before/after v2 records.
Closing1 remains 343 passed, 3 failed and 1 timed out of 347. No prior failed or
limited result is erased by this passing closing.

Final nine xtask checks and HYALO005 passed; Firefox-reference and dogfood
checks had no configured references/script and supply no additional live proof.
The final stable toolchain check remained rustc 1.98.1. The final ordered commit
gates passed: fmt, strict workspace/all-targets Clippy, then normal parallel
workspace tests, with 2551 passed, 0 failed and 420 ignored across 37 outer
summaries. The raw log also contains one nested subprocess fixture result; its
single pass is excluded from the outer count. Source/build/test inputs still
match all 571 reviewed pins. Final documentation adoption changes only evidence
and completion records, so these unchanged-runtime gates remain applicable.

The closing-evidence review accepted the nine profile attributions and complete
closing result, with zero source/behavior or missing mandatory attribution
findings. Its one low wording finding is addressed above: command identity
uniqueness includes the ledger path. The passing-stderr coverage limit is also
explicit. See `review-closing-evidence/review.md` and `final-gates/`.

After the non-live workspace gates, a fresh complete v2 boundary found all 65
existing private profile rows exactly unchanged, no unexpected process and the
same protected native births. Four additional managed-profile directories exist
inside the isolated ordered-gate home (69 total); their owner marker names the
workspace process, and no individual creating test was recorded. They remain
preserved, not evidence of a Firefox launch or worker return. All four real
profile directory identities and owner-marker bytes also compare exactly.
`final-gates/conservation.json` records these separate observations.

## Carry-over

Every original criterion is satisfied at its stated scope: the historical orphan
has an explicit bounded unresolved disposition; established repairs have owned
launch accounting and preservation proof; repair-sensitive regressions, ordered
gates and final closing passed. Historical failures and incomplete observation
phases remain failed or limited in the dated records above.

| Item | Disposition and evidence / reopening condition |
| --- | --- |
| Original 39345 survivor, erroneous cleanup summaries and unknown failed ordinal/error/marker-removal actor | **No plan, with a stated reason:** the finite audit is complete and no retained observation identifies the missing historical action. Reopen on a new attributable survivor or recovered contemporaneous failed-attempt/cleanup evidence. The original occurrence and withdrawn warm-control claim remain intact. |
| Established child-status/output/unwind cleanup defects and hidden retry outcomes | **Closed in this PR:** `commands/launch.rs`, `tests/common/mod.rs` and `tests/e2e/harness_session.rs` retain child ownership through output, scope cleanup, collect actual waits and preserve every attempt. Owned-child failures, controls and mutations discriminate the repair; no blind retry remains. |
| Darwin inherited descriptors and native 158 output accounting | **Closed in this PR:** `util/child_fds.rs`, its subprocess tests and `live_158_launch_lifecycle.rs`; positive child handshake/EOF-while-alive and disabling-helper mutation, native 158 plus final closing. This does not identify the exact historical sampled read fd or every output hang. |
| Failed-launch fixture evidence deletion/exempted survivors | **Closed in this PR:** `live_175_failed_launch_profile.rs` retains the actual private home, fails on every managed survivor and records raw output; four browser-free controls and sensitive mutations plus native 175/96 and final closing. |
| Wrong proxy PID escalation and changed 153 ownership fixtures | **Closed in this PR:** `daemon/client.rs`, `live_153_replace_double_envelope.rs`, `live/support/replace_fixture.rs`; genuine owned fallback positives, typed controls/mutation, separately admitted native positives and two-home truthful-refusal survival proof. Original assertions and default-root plus override support remain. |
| Failed closing1 (343 pass / 3 fail / 1 timeout), lost 153 panic text, original private 153 diagnostic failure | **No plan, with a stated reason:** current measured cleanup/descriptor/proxy defects are repaired above; retained historical evidence cannot recover the old panic assertions or exact read fd. A new attributed failure of those contracts or new contemporaneous evidence reopens investigation. Closing1 and diagnostic exit1 remain failures. |
| v1 wrong marker spelling / stripped marker text and missing pre-case bytes | **Closed in this PR's evidence record:** the reviewed v2 collector and discriminating mutation repair future collection; all v1 conservation claims are explicitly limited. **No plan** for unavailable historical bytes: no reconstruction is possible from retained metadata; recovered original bytes would reopen reconciliation. Real-profile byte records were captured separately. |
| Strict 153 phases with ESRCH gaps, missing baseline, missed images and ambiguous case3 stopping predicate | **No plan, with a stated reason:** failed phases stay refused/limited; original repair obligations are met by independently reviewed positive facts and separately admitted negative proof. No remaining mandatory fact is supplied by inference. New contradictory attributable evidence or a requirement for the unscheduled continuous coverage would require a new finite plan. |
| Invalid root all-nine-raw-output and empty-outer-stderr checks; earlier compile, Clippy, source-scan and draft mutation failures | **Closed in this PR's evidence record:** original failed receipts remain; schema/contract corrections are explicit, correct final checks pass, and no native repetition manufactured completeness. These are supervisor/diagnostic corrections, not hidden test assertion changes. |
| Ninth closing profile and target-destroyed passing-stderr coverage limit | **No plan, with a stated reason:** the independently reviewed product record, exact markers and libtest pass supply required launch attribution. The older direct helper has no retained raw output, and later navigation assertion execution is unmeasured. No new navigation failure is established; an attributable failure or new explicit coverage requirement would justify a separate plan. |
| Four retained non-live gate profiles and all boundary-only cleanup observations | **No plan, with a stated reason:** exact preserved baseline, isolated gate home and clean native boundary establish preservation; profile existence alone establishes neither a launched Firefox nor a worker-return defect. An attributable live survivor or unexpected baseline change would reopen investigation. |
| 272/277 launch infrastructure context | **No plan, with a stated reason:** this is repaired infrastructure context, not a new auto-wait/navigation failure. Upcoming [[iteration-272-autowait-deadline-and-diagnostics]] and [277’s preserved plan](/Users/james/.cache/ff-rdp/remaining-20260923-277/kb/iterations/iteration-277-direct-navigate-committed-about-blank.md) retain their original criteria and causal limits. Their adaptation must retain the merged launch contracts. A newly attributed deadline/navigation defect would be reconciled into those plans; none is inferred here. |
| 273 historical output hang and 280 preexisting disappearance | **No plan, with a stated reason:** these already completed bounded outcomes retain their own unknown causes. This PR provides no new contemporaneous evidence to reopen either; 280 merged at 019c0ce22b73736de1c5ac331a678bc26b72d886 independently. |

No unfulfilled original 282 criterion is deferred into another plan. Publication,
exact-head CI and GitHub merge are recorded separately after this local closure.
