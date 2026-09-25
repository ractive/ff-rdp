---
title: "Iteration 277: Reject ambiguous blank navigation commits"
type: iteration
date: 2026-09-19
status: done
branch: iter-277/navigation-guard-final-20260925
# NN must be free: `ls kb/iterations/` before filing. check-iteration-plan fails when
# another plan already claims it. Letter suffixes (61b, 162a) are distinct numbers.
depends_on:
  - "279"
  - "282"
# If this iteration introduces new pub items, list each one with its first call site.
# Leave empty ([]) if no new pub items are introduced.
# Required by cargo xtask check-iteration-plan when the body mentions pub symbols.
first_call_sites: []
# Describe how to manually exercise this iteration's output end-to-end.
# Required by cargo xtask check-iteration-plan.
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test
  live live_166_navigate_document_status::live_166_navigate_status_direct_parity
  -- --include-ignored --exact --nocapture --test-threads=1
tags: [iteration]
# Add `skill-edit` if this iteration modifies files under ~/.claude/skills/.
# Skill-edit iterations cannot run through ralph-loop (the cmux child workspace
# has no write access to ~/.claude/skills/). Drive them by hand in a regular
# Claude session. See iter-61z for the canonical example.
---

# Iteration 277: Reject ambiguous blank navigation commits

Filed from iteration271's closing dual-gate sweep on baseline
`2e604688d226f4e77303468c158f0d059a59cb44`, Firefox156.0. This was filed as carry-over
only; no277 implementation or isolated rerun had been performed at filing.

## Measured failure

`live_166_navigate_document_status::live_166_navigate_status_direct_parity`
failed its committed-URL assertion at line283. The direct-route envelope was:

```json
{"navigated":"https://example.com?ff-rdp-cache-bust=1789824374155530000-1","status":200,"status_reason":null,"committed_url":"about:blank","ready_state":"complete","elapsed_ms":695}
```

Expected committed URL was
`https://example.com/?ff-rdp-cache-bust=1789824374155530000-1`.
The test uses a fresh cache-busting URL; this is not the historical304
assertion shape. The sweep had six CLI workers and was the only workflow
Cargo/Firefox workload; the user's desktop Firefox was preserved. There was
no watchdog expiry and no live-owned profile leak. Those facts
do not prove a contention cause or rule out a product defect. No raw RDP
event sequence or page snapshot was retained for this occurrence.

Evidence: primary checkout `.git/ralph-loop/20260919-queue/iter271/sweep.log`
and its `.meta` sidecar, `sweep-launches.log`, `actual-verdicts.tsv`.
Keep this distinct from BBC consent and daemon target-lifecycle findings
unless attributable evidence connects them.

## Tasks [0/3]

- [ ] Reproduce on an owned browser with the direct route and retain the
      requested URL, document identity/readiness, network status and navigation
      event ordering at the failed occurrence; retain passing controls.
- [ ] Identify why a successful status accompanies a stale committed URL;
      repair only the demonstrated cause without widening the assertion.
- [ ] Add a bounded regression that fails without the correction and passes
      with it, then run real-Firefox verification and the ordered gates/sweep.

## Acceptance Criteria [0/3]

- [ ] An attributable occurrence explains the `about:blank` committed URL;
      a passing isolated rerun alone does not count as an explanation.
- [ ] `live_166_navigate_status_direct_parity` still requires the canonical
      destination URL and correct HTTP status, with a regression demonstrating
      sensitivity to the repaired behavior.
- [ ] Ordered fmt/clippy/workspace-test gates and a final-source dual-gate
      sweep are recorded with all non-green outcomes explicitly dispositioned.

## Out of scope

Executing this plan as part of271; changing BBC consent, daemon handshake,
unrelated frame-target behavior, or accepting `about:blank` to turn a failure green.

## References

- [[iteration-271-bbc-consent-no-cmp-recurrence]]

## Later passing control — 2026-09-19

The required final-source sweep after271's bounded-diagnostic repair passed
`live_166_navigate_status_direct_parity`. Evidence:
`.git/ralph-loop/20260919-queue/iter271/repair1/sweep.log`. No navigation code
changed, no277 investigation or isolated rerun was performed, and the original
HTTP200/committed-`about:blank` failure and all ACs remain open. This pass is
not a causal explanation or a fix.

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

The original plan is recovered verbatim from commit
0e9f978b67f6de74bbd276e04dec4704b5554a61 in the preserved 271 checkout; no 271
implementation or unrelated planning history is imported. Rebase investigation on merged
main c761c1b1 and retain 262’s repairs. The unchanged 166 direct-parity test still
requires canonical committed destination and status 200. A current same-document
shortcut can accept changed complete about:blank while requested-resource fallback
supplies status 200, but it requires nonblank pre_href. First discriminate that
precondition with finite actual-caller scripted cases and capture it explicitly in live
attempts. A first-navigation blank baseline cannot be assigned this mechanism without
evidence. This is distinct from 274/276’s pure-readystate candidate. Original AC 1
remains unticked without attributable cause.

## Focused execution checkpoint — 2026-09-24

Actual execution base is merged main `4e5820b78099e1555c55cb7e0ac7e1004c840c6b`
(PR265/273 and PR266/278 retained). The original historical occurrence remains
unattributed; no original task or AC is checked by the following control.

A finite four-case scripted test of the actual `wait_for_doc_complete` caller
confirmed the precise mechanism: nonblank `pre_href`, complete `about:blank`
shortcut result, and the requested document's resource/status produce a blank
commit with HTTP200. A blank baseline produces a null shortcut and waits for
canonical destination events instead. Hash and pushState-shaped changed results
remain positive controls. Each case sent exactly one navigation action; no
navigation retry was admitted. These are declared protocol scripts, not recorded
Firefox fixtures and not the original occurrence's missing trace.

The candidate repair rejects a blank shortcut when a different requested URL is
known. It preserves explicitly requested blank and unknown-destination history
behavior. The regression failed before the guard with actual blank versus
expected destination, and passed after it. Two additional boundary controls cover
explicit blank and unknown destination. This addresses a demonstrated caller
mechanism; it does not assign that mechanism to the untraced historical failure.

One named real-Firefox direct-parity test passed (both existing legs, with the
canonical destination and status200 assertions unchanged). Its first leg's raw
RDP responses recorded `pre_href=about:blank` and epoch1790202927268. The shortcut
returned null. Thus this passing first-navigation control did not exercise the
nonblank prerequisite, and does not explain the historical failure. The existing
transport trace retained request/reply and document/network order plus actor and
inner-window forms. The capture filter mistakenly named `ff_rdp_cli` instead of
the binary tracing module, so the new explicit baseline/completion-branch debug
records were absent; raw protocol baseline evidence is present. This diagnostic
limitation is retained, not promoted to complete failing-occurrence attribution.
The owned launch PID39455 was absent after harness teardown; protected desktop
Firefox identities remained present. No browser was killed outside the harness.

Private evidence: primary `.git/ralph-loop/20260923-remaining272-278/iter277/`,
including `schedule1.md`, `schedule2.md`, `matrix1.log`,
`regression-before.log`, `regression-after.log`, `live1.log`, command sidecars,
and `progress.log`. A first Clippy pass rejected a needless-by-value test helper;
its log remains and the helper now borrows. Original AC1 still needs an
attributable live occurrence; the source mechanism and passing control are not
substitutes. At that checkpoint the candidate still required independent review and its
final-source closing sweep before any publication. No 277 PR or merge is claimed.

Independent initial review found one explicit-blank boundary defect: the request
scheme may be `ABOUT:` while Firefox returns canonical `about:blank`. The shared
ambiguity helper now compares only the scheme case-insensitively and preserves
case-sensitive scheme content. The actual-wait matrix retains its six cases and
adds `ABOUT:blank` acceptance plus `ABOUT:Blank` rejection. The mixed-case-scheme
case failed before this correction and the eight-case matrix passed afterward.
This source-boundary repair is not a new live attribution claim.

A fixed warm local cross-site observation test and exact before/corrected build
packet are prepared privately for fresh review. They preserve one measured
navigation, original waits and canonical/status200 assertions, with warm setup
recorded separately. No new browser occurrence has been authorized or executed
at this checkpoint. The original attributable-occurrence and final-source sweep
requirements remain open; no completion status is asserted.

## Reviewed repair and spent warm observation — 2026-09-24

Independent review approved the product guard and scheme-boundary correction.
The eight-case actual-wait regression includes preserved hash/pushState, blank
baseline, explicit blank, unknown destination, mixed-case scheme and
case-sensitive content boundaries. Its before failures and after passes remain
private evidence; they establish the controlled scripted mechanism, not the
historical occurrence's missing live cause. Capture-policy reviews also passed
after narrowing the fixture marker to a returned Drop and rejecting additional,
unknown or ambiguous panic diagnostics. No worker-join claim follows from that
marker.

The subsequently authorized single warm cross-site **before** observation used
Firefox156.0.1, build20260921121718, distinct from historical Firefox156.0. Only
the new shortcut guard was removed; the repaired scheme helper, diagnostic
logging and all other reviewed code remained. Warm setup navigated to an owned
127.0.0.1 fixture and returned its canonical URL/status200 in240ms. The sole
measured action navigated to localhost on the same fixture port and returned
canonical destination/status200 in221ms. The measured baseline was the warm URL,
epoch1790205702418, process19180/innerWindow12884901889. The replacement document
was process19214/innerWindow19327352833 with the destination URL. Requested
document resource12884901891 supplied200; destination dom-complete selected
completion. No post-navigation same-document probe ran. This answers only that
fixed scenario: it cannot say what a probe would have observed or whether an
unobserved transient blank existed. It does not attribute the historical failure
or exhaust future hypotheses. No after, retry or extra arm ran.

The exact test passed1/1 with complete outputs, zero panic diagnostics and
non-interrupted actual child wait. Owned Firefox and fixture Drops returned;
owned process absence was verified and the protected desktop preserved. The
private home/profile and full trace remain retained. Both favicon404 responses
were non-document resources and did not replace either selected document200;
no new product defect follows from those expected missing fixture resources.
Corrected source/CLI were restored byte-for-byte on completion. The temporary
observation function/comment were archived and removed before final-source
validation so a later sweep cannot silently repeat the spent discovery case.
The useful opt-in command-output diagnostic helper and original166 assertions
remain.

Evidence is under the same private iteration directory: `repair2/`,
`review-initial/`, `review-repair-capture/`, `review-panic-policy/`, immutable
`admission2/` and `admission3/`, and `warm-observation1/outcome.md` with raw
outputs, actual argv/env/wait, identities, restoration and hashes.
`final-checkpoint/` retains the exact observation-removal delta, final source
and ordered validation for the reviewed candidate. Earlier failed checks,
setup errors and incomplete capture logging remain preserved, not discarded.

All original tasks and ACs remain0/3. AC1 lacks an attributable real-Firefox
failing occurrence. AC2 retains its original canonical/status assertions and
has the scripted repair-sensitive regression plus passing named real-Firefox
control, but these do not complete the original causal/live proof. AC3 still
lacks the candidate's required final-source dual-gate sweep. No sweep is run
at this checkpoint, and no iteration completion, PR or merge is claimed.
The ordered final-source workspace gates are checkpoint validation only; their
actual verdicts and any failures are retained privately. No scope/criterion
is weakened to fit the passing controls.

## Late cleanup correction — 2026-09-24

A broader preflight at02:04CEST found Firefox39345/debug63135 using this run's
exclusive live1 profile, with native birth1790202922.085764 inside the original
00:35:15–32 command window. The successful launch ledger records only39455/debug63162.
The39345 profile has neither owner marker. Retained warm and checkpoint process
tables already contain39345 and helper39453: complete owned cleanup and the single
owned-browser workload claim were contradicted, not merely unverified. The prior
reports and source checkpoint remain immutable historical evidence.

After verifying exact current birth, executable and private-profile argv, root
signaled only39345 with SIGTERM at02:06:23CEST. It and helper39453 were absent
afterward; protected desktop14298/14301 retained their exact native identities.
Profile/evidence were preserved. External process absence does not establish
worker return, successful joins or the original launch/marker failure cause.

The warm occurrence remains spent and its actual1/0 result, canonical HTTP200,
221ms destination dom-complete and absence of a same-document probe remain measured
facts. Withdraw its unconditional valid admitted-control classification: prior
owned-browser cleanup was incomplete and the single-workload condition was violated.
The effect on scheduling is unknown. Workspace2539/0/419 and unchanged scripted
proofs remain recorded functional results with this corrected context; they are
not clean-workload evidence or a closing sweep. No replacement capture is implied.

Private correction/recovery/review: primary
`.git/ralph-loop/20260923-remaining272-278/iter277/late-owned-cleanup/`.
The source checkpoint62fda89da4d51d2ff277d11be94c777aad26dbc6 remains incomplete,
with original tasks/AC0/3. [[iteration-282-unledgered-owned-firefox-survival]]
tracks the distinct accounting/cleanup gap as planning only; no common cause with
273 or280 is established and no282 execution is authorized.

## Prospective scope adaptation — 2026-09-25

This iteration delivers the demonstrated same-document-shortcut correction.
A known nonblank destination must not complete through a changed complete
about:blank shortcut. Explicit blank requests, scheme case, hash/pushState
and unknown-destination history retain their supported behavior.

The historical HTTP200/about:blank occurrence remains unattributed because
its baseline and event sequence were not retained. Original AC1 remains
unfulfilled; the withdrawn warm control remains withdrawn. Reopen historical
attribution only on recovered contemporaneous evidence or a new attributable
occurrence, not another unchanged passing sample.

### Delivery acceptance [3/3]

- [x] The reviewed actual-caller regression fails without the guard and passes
      with it, including its eight existing boundary controls.
- [x] Final-source Firefox verification preserves the original canonical
      committed destination and HTTP-status assertions.
- [x] Current ordered gates and this iteration's own dual-gate closing sweep
      pass, retaining 279's refresh-consumer behavior and 282's launch ownership.

The owner’s September25 all-open adaptation request supplies the prospective
delivery scope above. Completion under this scope must say so explicitly and
retain every unfulfilled historical AC; it must not report the original
attribution criterion as passed. Earlier investigation schedules and restrictions
remain historical records, not renewable capture allowances. Use only the
finite controls needed for the new contract and the required closing validation.


## Current prospective candidate — 2026-09-25

The recovered product patch is adapted onto main
`c25a80c5c0011e6b895d81137c848422e16b9512`, retaining 279's watched-history
refresh consumer, 272's deadlines/frame handling and 282's launch ownership.
The patch applied without removing or rewriting those changes. The original
166 test ordering, canonical destination and HTTP-status assertions are unchanged;
its only addition is the previously reviewed opt-in output diagnostic.

Current-input proof was repeated because the caller's refresh behavior changed
since the old evidence. One guard-only mutation failed with actual committed
`about:blank` versus the expected destination, while reporting HTTP200.
The restored actual-wait test passed all eight cases with one navigation action
per case. Earlier proof remains historical supporting evidence, not a substitute
for this current-input run. No Firefox, warm observation or native discovery was
executed by the implementer. The withdrawn warm control remains withdrawn.

Private commands, raw outputs, actual exits, source and binary attestations are in
primary `.git/ralph-loop/20260924-all-open/iter277/current/`.
Independent review, final-source Firefox verification, this iteration's own
closing dual-gate sweep and closing dispositions are still required. Original
AC0/3 and delivery0/3 remain unticked at this implementation checkpoint.
The historical failure remains unattributed; this candidate makes only the
prospective delivery claim above.

## Prospective delivery completed — 2026-09-25

The reviewed guard was integrated onto main `c25a80c5`, preserving279’s
refresh-consumer behavior and272/282’s repairs. On these current inputs, removing
only the guard failed the actual wait caller with blank/HTTP200; exact restoration
passed all eight boundary controls. The ordered formatting, strict all-targets
workspace Clippy and normal parallel workspace tests passed once:2567/0/421
(282’s separately emitted nested child is excluded). Independent local review
accepted the frozen candidate with zero findings.

This iteration’s own final-source dual-gate sweep passed349/349 across all six
exact tiers (CLI338; core1/3/3/2/2), including original166’s canonical destination
and HTTP-status assertions. It reported zero leaked or unattributed profiles.
All343 launch attempts paired; all192 pre-existing profile directories/markers
were conserved and10 new retained profiles were attributed. The owned raw browser
had an actual child wait; protected desktop identities remained unchanged. No
worker-return inference is made from other process absence. Source574 inputs,
five binaries, Firefox156.0.1 package110 pins and four real profiles matched
before and after.

Private evidence: `.git/ralph-loop/20260924-all-open/iter277/current/`
(frozen candidate, failed mutation, restored control and ordered gates),
`iter277/review-final/report.json`, and `iter277/closing1/` (raw results,
exact-tier and launch/profile reconciliation). Sweep log SHA256
`0d0cceba801fbffe835632907850ae88f40931ca2a825f0839666340371ecb4e`.
An earlier reviewer refused filesystem isolation and returned no verdict; that
record remains separate from the completed instruction-enforced read-only review.

Completion is under the prospective delivery scope above. Original historical
tasks/AC0/3 and the withdrawn warm observation remain unchanged; neither this
sweep nor the scripted repair assigns the untraced historical failure’s cause.
Separate281 frame, initial-navigation timeout and timing failures remain preserved
and are not attributed to277.276 remains the next dependent readiness-sampling
repair; it must retain this guard and its supported navigation boundaries.
