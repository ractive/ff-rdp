---
type: iteration
status: done
date: 2026-09-19
branch: iter-274/styles-applied-unattributed-recurrence
title: "Iteration 274: attribute the recurring applied-styles live failure"
depends_on: []
first_call_sites: []
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test
  live live_styles_applied::live_styles_applied_returns_real_rules --
  --include-ignored --exact --nocapture
tags:
  - iteration
  - carry-over
---
# Iteration 274: attribute the recurring applied-styles live failure

## Trigger and retained evidence

Iteration 203's `iteration_253_precheckpoint_2026_09_12` frontmatter requires a
scoped diagnostic follow-up when
`live_styles_applied::live_styles_applied_returns_real_rules` recurs in an unpaused
sweep or exact isolation. Iteration 261's first required closing sweep fired that
trigger on 2026-09-19. The result was `FAILED`, but the panic body was not emitted
before an independent contended-launch hang caused watchdog teardown. This plan
records the already-fired trigger; it does not defer filing until another recurrence.

Retained evidence: `.git/ralph-loop/20260919-queue/iter261/logs/live-sweep.log`.
The corrected full sweep in `iter261/logs/live-sweep-rerun.log` passed the same test.
That pass is a control, not a cause or repair. Missing evidence includes the panic,
URL, document identity/DOM, route and daemon diagnostics. No load, selector, network,
protocol or page-content cause has been established for this occurrence.

## Tasks [2/3]

- [x] Inspect the historical253 observation and this recurrence without assuming they
      share a mechanism; preserve exact output and source/runtime versions.
- [ ] Use a bounded local reproduction or independently required sweep to capture
      the failed test's panic, URL/document/DOM, route and relevant daemon state before
      teardown, with evidence retained even if a sibling test hangs.
- [x] Diagnose from a failed occurrence and apply a scoped repair only when justified;
      otherwise record the bounded attempts and missing evidence honestly.

## Acceptance Criteria [2/3]

- [x] The already-fired203 trigger and the missing261 failure diagnostics are explicitly
      preserved; later passing controls are not represented as repairs.
- [x] Failed-occurrence evidence identifies the mechanism, or a bounded investigation
      reports the unresolved evidence gap without claiming the failure fixed.
- [ ] Any repair has a meaningful regression, preserves real applied-style assertions,
      and records its required own closing sweep without weakening the test to pass.

## Execution boundary

This diagnostic follow-up is filed by261's closing review, not selected for the
September19 authorized implementation inventory. Leave203 parked and do not execute274
in that batch. The separate contended-launch hang belongs to
[[iteration-273-contended-launch-output-hang]].

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

At merged main c761c1b1 the named test still uses explicit --no-daemon through
base_args.262’s watched-daemon recovery therefore does not establish this test’s repair.
The historical panic remains absent: do not reduce the failure to a missing selector or
infer 276’s cause. Retain command stage, actual route, URL/document identity, DOM and
assertion evidence before teardown. A pure-readystate intermediate-document candidate is
distinct from 277’s same-document shortcut. First discriminate the actual caller with
finite scripted cases; declare a finite live schedule before capture. Preserve author-
rule/reset assertions and the original bounded-unresolved option.

## Capture preparation — 2026-09-24

Prepared at merged main `4e5820b78099e1555c55cb7e0ac7e1004c840c6b` with no
277 candidate imported. The original253 panic is retained: navigation succeeded,
then `styles p --applied` exited1 with `no element matching selector 'p'`;
its2.21s isolated pass on recorded Firefox155.0.1 is only a control. The261
recurrence reports `FAILED` but has no panic before the separate contended-bind
watchdog interruption. Its later named pass does not localize the missing failure.
Historical original test bytes equal the current base in both retained source
contexts; full failing-runtime binary attestation is not available from these
records. Do not equate the261 failure with the253 missing selector.

The relevant276 direct-readystate source cases are reused for the unchanged
navigation path, without rerunning them or treating cascade as styles evidence.
That native passing control also demonstrates that a cached getTarget form can
still name about:blank while the same console evaluates the new document. Cached
actor/innerWindow/url metadata is not immutable actual-document identity.

Two new browser-free styles capture-shape checks retain actual walker query
identity and success/missing-selector boundaries. The success arm reuses a
recorded CSS reply; neither arm executes a JS engine or supplies native CSS
evidence. A file-output qualifier preserves exact stdout/stderr on success and
failure with actual child waits. Initial mock greeting and test-only Clippy
defects were corrected; all earlier failures remain private. No product repair
has been selected.

Temporary instrumentation retains each original command/assertion stage, direct
route, raw protocol bytes, atomic successful readiness sample, adjacent styles
document/DOM samples, applied rules and dedupe/reset-filter decisions. Command
stdout/stderr stream directly to files before assertion unwind or teardown.
Original fixture, waits, commands, author-rule count and reset-stub exclusion
assertions remain intact. Adjacent DOM samples are not atomic with walker or
style calls; diagnostics can perturb timing. Private evidence and the proposed
single original named capture are under
`.git/ralph-loop/20260923-remaining272-278/iter274/` in the primary repository.
This checkpoint stops before Firefox pending independent review and admission.
Original tasks/AC remain0/3 pending the bounded investigation's outcome; no
historical repair, exhausted hypothesis set or closing sweep is claimed.

## Bounded investigation outcome — 2026-09-24

Exactly one independently reviewed/admitted original named occurrence ran. It
passed1/1 in2.36s on Firefox156.0.1, build20260921121718, with actual runner/test
exit0 waits and no interruption. Both original CLI commands reported direct
route to the owned browser. Navigation returned the requested fixture URL in
140ms. The first readiness evaluation returned false; the second atomic sample
reported the fixture href/documentURI, epoch1790210483125 versus the original
1790210482868 baseline, complete readiness and p=true. Cached target metadata
still named the earlier blank document; it is not contemporaneous document
identity and does not contradict that actual evaluation.

Styles acquired its own target and DOM root, then queried p and received a P
node. Adjacent before-query and after-applied samples both retained the complete
165-character documentElement HTML, fixture URL/epoch and p=true, with
truncated=false. These samples are not atomic with the protocol calls. The raw
getApplied reply contained both author rules and three references to the empty
reset stub. Parsed/filter diagnostics retained both nonempty author rules,
deduplicated the repeated reset actor, and classified the surviving empty reset
stub for exclusion. Final output contained exactly two p rules with nonempty
properties and no reset stub. Every original command/assertion stage executed;
neither assertion nor fixture/wait was weakened.

This is a current instrumented passing control, not a repair or a cause for
either historical occurrence. The253 missing-p error still lacks its failing
document/target/DOM trace. The261 recurrence still lacks even its panic/stage,
as well as attributable URL/document/DOM/route/rules evidence. Historical runtime
attestation remains incomplete; the recorded253 Firefox155.0.1 differs from this
156.0.1 control. Added diagnostics can perturb timing. The finite unchanged
navigation source experiments and this control do not exhaust possible causes.
No further unchanged attempt, product repair, before/after proof or sweep was
selected.

The bounded-unresolved alternative in original AC2 is fulfilled by this retained
control and precise evidence gap; AC1 is fulfilled by preserving the already-fired
trigger and both historical failures without representing later passes as fixes.
Task2 remains unchecked: no new failed occurrence was captured. Repair-conditional
AC3 remains unchecked and is not applicable to this no-repair disposition; no
regression repair or repair-specific closing sweep is claimed. Independent outcome review returned zero actionable findings and confirmed closure
as completion of the bounded investigation, not resolution of the product failure.
The review is retained at `iter274/review-outcome/report.md` under the evidence
root, SHA256 `27571d2ad6aa3515e0451ca89897d1c98f2d173089dabb089c660d073f782ef5`.
Original task/AC wording remains unchanged.

All temporary runtime/test diagnostics were reconstructed byte-for-byte from
the private frozen patch/module, then restored to merged main4e5820b7. The final
change is this documentation only. The iteration-close product-source sweep
requirement therefore does not apply to the submitted change: no product or test
behavior is shipped, and the one diagnostic control is not presented as a closing
sweep. The original live test and its assertions remain exactly at base. Private
source/binary archives, all failed qualifiers, raw capture files and source/runner
provenance remain under the evidence root above.

Root's full before/after process inventories and native identities found no
unprotected browser/workload after capture; protected desktop14298/helper14301
births were unchanged. No recovery signal occurred. One owned profile remnant
with marker13397 remains preserved, with that process absent. Actual child waits
are recorded separately; process absence is not worker-return proof. General
launcher cleanup limitations and unrelated277 evidence remain separate.

Final restored-source validation: stable1.98.1 unchanged; cargo fmt, strict
workspace/all-target Clippy and normal-parallel workspace tests passed in that
order,2538passed/0failed/419ignored across37 summaries. All nine enumerated
xtask gates exited0; check-firefox-refs has no refs, and check-dogfood-script
reported SKIP because this plan has no script. The live env flag was supplied
only to that already-verified nonexecuting checker path, with no force override
or browser execution. Hyalo HYALO005 checked477files with no issues. The final
CLI was freshly rebuilt from restored W274 source; its SHA and exact Cargo
provenance are retained privately. These checks are not live-sweep or CI results.

## Carry-over

| Observation | Disposition |
|---|---|
| Historical253 missing-p error;261 missing panic/document/route/rules; unchecked failed-occurrence capture task | No additional plan now: original274 explicitly permits a bounded unresolved outcome. On another attributable recurrence in an independently required sweep or separately justified scoped capture, reopen this investigation or file a scoped follow-up preserving the exact panic/stage, command bytes, URL/document/DOM, route, target and rules/filter trace before teardown. No unchanged blind retry is implied. |
| No product repair or repair-specific regression/sweep; AC3 unchecked | Not applicable to this no-repair documentation outcome. Any future demonstrated repair must meet this unchanged requirement. |
| Reused direct-readystate source characterization and cached-target identity limitation | Retain privately as limited source/capture evidence; not historical attribution, a navigation repair or proof of shared cause with276/277. |
| Mock greeting and test-only Clippy qualifier failures | Closed within private diagnostic preparation, with failed logs/source preserved and corrected browser-free checks passing; no product defect claim. |
