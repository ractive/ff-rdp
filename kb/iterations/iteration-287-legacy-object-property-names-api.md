---
title: "Iteration 287: Retire the unsupported object property-names API"
type: iteration
status: done
date: 2026-09-28
branch: iter-287/legacy-object-property-names
depends_on: []
tags:
  - iteration
  - carry-over
  - protocol
  - api
first_call_sites: []
dogfood_path: >-
  Recheck repository callers and installed object spec/server once. Prefer
  removal or deprecation of the unused unsupported API after checking public
  compatibility; if retaining behavior through a supported operation, declare one independently
  reviewed matched native object-property control with exact property semantics,
  package identity and owned cleanup. Do not launch Firefox merely to re-prove an
  absent method. Stop answered hypotheses; run required ordered gates and this
  iteration's own closing sweep for product changes, not discovery sweeps.
firefox_refs:
  - lines: 100-220
    path: devtools/shared/specs/object.js
    why: declared supported property operations
  - lines: 380-410
    path: devtools/server/actors/object.js
    why: prototypeAndProperties implementation
kb_refs:
  - kb/rdp/actors/object.md
  - kb/iterations/iteration-18-dogfooding-fixes.md
  - kb/iterations/iteration-10-object-inspect-and-native-actors.md
---
# Iteration 287: Retire the unsupported object property-names API

## Problem and boundary

`ObjectActor::own_property_names` sends `ownPropertyNames`, absent from installed
Firefox156.0.1's object spec/server. Repository search finds only its definition
at `crates/ff-rdp-core/src/actors/object.rs:150–155`; no current CLI caller uses it.
`commands/inspect.rs:71` and `commands/eval.rs:1561` use prototypeAndProperties.
Completed iteration18 already records Firefox149's rejection and the eval repair.
This is a remaining public API/maintenance defect, not a newly observed eval or
inspect failure. Possible external library consumers are unknown.

Keep this separate from259 reply ownership and266 daemon grip lifetimes. It is
queued after259 to preserve its frozen reviewed source;286 has no technical
dependency on287 and vice versa. Do not introduce a new consumer merely to
justify an otherwise unused public helper.

## Tasks [3/3]

- [x] Recheck callers, exported API expectations and package contract; choose
      removal/deprecation or an accurately specified supported implementation.
      Record compatibility consequences instead of silently changing semantics.
- [x] Apply the smallest justified API repair and reconcile stale current
      documentation. Preserve iteration18's completed fix and historical notes
      through dated corrections, not rewritten historical outcomes.
- [x] Add meaningful validation for the selected behavior, independent review,
      ordered workspace gates and required per-iteration closing validation.

## Acceptance Criteria [3/3]

- [x] The maintained API no longer issues unsupported ownPropertyNames to the
      qualified Firefox version. Removal/deprecation or supported replacement
      has an explicit compatibility decision and no invented production caller.
- [x] Existing eval/inspect property output remains correct. If a supported
      replacement is retained, own-property inclusion/order semantics are stated
      and demonstrated with a qualified native object and a repair-sensitive
      control; a renamed unsupported packet or fabricated fixture cannot pass.
- [x] Documentation distinguishes the old repaired CLI issue from the leftover
      API, all relevant checks and independent review pass, and product changes
      have their own reconciled dual-gate closing sweep with honest dispositions.

## Finite validation boundary

The missing method and absent in-repository caller are already source-evidenced;
one refreshed census answers whether that starting state changed. Do not spend
a native capture simply confirming the same unsupported request. If retaining
implemented behavior, first define the supported property operation and precise
oracle; use one finite baseline/current comparison and stop once answered.
Qualification failure ends the scheduled block. No blind retries, broad timeout
changes or full-sweep discovery. The required closing sweep validates the final
product change; it does not replace a specific API proof.

## Exact provenance

Reviewed259 source manifest:
`73fdcae5ebdd584ad64154cbeb39e81222b672355c1698aa3daa59ff1d4b5e58`.
Installed Firefox156.0.1, BuildID20260921121718, SourceStamp
`6f2c158dfc7e9693f880fad2510ceb51a158c069`.
Archive `/Applications/Firefox.app/Contents/Resources/browser/omni.ja` SHA256
`85f891cec3e54150027582ac74eb96fc3774cc0ab4bfd96f7192905149d05fff`.
Members have prefix `chrome/devtools/modules/devtools/`:
`shared/specs/object.js:100–220`, SHA256
`55c62975ad8b4e8bdb70efc432bc5d8aef5ffa84bd0d0267f126ff31038852a0`;
`server/actors/object.js` (prototypeAndProperties at386), SHA256
`85dc0b136dacd5b32c6c9ea4ad37608ddac207039daec6728f3b49d3a6104f5e`.
Hyalo search found historical claims in research/implementation-gap-analysis.md,
research/rdp-protocol-deep-dive.md and iteration10; iteration18 records the fix.

This plan records no implementation, test execution or new runtime failure.
All criteria remain pending. No new public primitive is proposed.

## Independent execution without259 or286 — 2026-09-28

Admitted on merged main964ee9aec54379e6393a6e26883e520516c480d7.
The259 dependency was serial scheduling only: this unused public object method
and its supported eval/inspect callers are unchanged on main.286 is preserved
unmerged after its unrelated closing failures; no286 or259 source is imported.
The owner’s all-open grant permits this independent next iteration while Windows259
and dependent266 remain deferred. Original tasks and acceptance text are unchanged.
The chosen scope is removal of the unsupported sender, an explicit breaking Rust
API change, with migration guidance and existing-caller regression coverage.
No replacement API or fresh native attribution experiment is proposed.

## Implementation decision — 2026-09-28

Removed the unused `ObjectActor::own_property_names` sender rather than retaining
an unsupported packet or silently substituting different semantics. Refreshed
main-based caller/API/package inputs match the preparation census: `ObjectActor`
is publicly reexported, `ff-rdp-core` is a published crate, external consumers are
unknown, and the existing `eval`/`inspect` product callers already use
`prototype_and_properties`.

This is a breaking Rust source API removal. The next published crate version
containing it must use a breaking pre-1.0 minor version (0.4.0 or a later
appropriate minor), not a compatible 0.3.x patch. No package version bump or
crate publication occurs in this iteration. The PR must identify the breaking
change. [[rdp/actors/object]] gives opt-in migration through the existing
descriptor map and records its string-key inclusion and lexicographic order;
no equivalence to a historical names-only operation is claimed.

The existing `eval_object_result` e2e test now uses the recorded supported
property fixture, asserts `propertyNames` and checks the actual request's
method/actor. Existing inspect e2e and core parser tests cover the unchanged
descriptor output. Dated notes correct the current reading of iteration 10
and the two research claims; iteration 18's completed outcome is unchanged.

Implementation is awaiting independent review, ordered workspace gates and
this iteration's own dual-gate closing validation. No test or native capture
is claimed here; original acceptance criteria remain pending.

## Verified validation and first closing result — 2026-09-28

The independent product review accepted the complete source and documentation
change with zero actionable findings. Ordered current-stable formatting, strict
workspace Clippy and workspace tests passed: 2719 reported passes, zero failures
and 429 ignored across 38 summaries (including nested helper output, not 2719
unique parent tests). Existing eval/inspect controls passed. All 590 relevant
source inputs remain unchanged; these checks are reused without repetition.

This iteration's first complete dual-gate closing sweep used the default six
self-launching workers. All six tiers were reconciled: 350 passed and one failed
of 351; no skipped, preexisting, vanished, launch-timeout or watchdog-timeout
coverage. CLI was 339/1 and the five core tiers supplied eleven passes.
Profile summaries were leaked=0 and unattributed=0. Actual outer tool21203 and
sweep exits were1; neither timed out. All 342 launch pairs across 17 ledgers
matched. All 526 pre-existing retained profiles were conserved; ten new fixture
profiles were attributed, giving536. Owned browser cleanup completed with an
actual raw-child wait, no owned survivors, and protected/real state unchanged.

The single failure was
`live_237_act_and_see_timing::live_237_cancelled_submit_does_not_wait_out_the_timeout`:
4893.203875ms exceeded its unchanged strict2000ms bound. Command success and
`navigated=false` passed. Reported load214.31/108.29/56.93 is context, not proof of
cause. Independent audit found no changed submission/timing path in287 and no
justified product repair from this occurrence. The original failure remains
failed even if later tests pass.

All nine enumerated static repository checks then passed, including two verified
Firefox references. Dogfood lint/execution explicitly skipped because the plan
has no script; this is not additional live proof. Final committed-head actor/KB
validation remains owed. Actual static tool36484 exited0.

A separately qualified single isolated unchanged237 control is being prepared.
Only an actual passing control with complete cleanup may admit one new full
serial dual-gate closing sweep. No result, causal explanation, successful closing
or original acceptance completion is claimed by that preparation.

Evidence is private under `.git/ralph-loop/20260924-all-open/iter287/`: product
review `review-product1/report.md`, ordered `nonlive1/`, complete failed
`closing-preparation1/`, exact failure `audit-closing1-timing1/report.md`, and
static `post-closing1-static1/`. Every earlier raw result remains preserved.

## Carry-over

| Observation | Disposition |
| --- | --- |
| Default-six closing1 cancelled-submit elapsed4893.203875ms exceeds2000ms; successful command/non-navigation, exact phase and scheduler contribution unknown. | File [[iteration-293-cancelled-submit-timing-attribution]]. Preserve this failure separately from any later isolated/serial result; no load waiver or claimed287 repair. |
| Removal breaks potential external Rust callers; no replacement is retained. | Closed in this PR's explicit API decision and object-actor migration guidance: next crate publication containing it requires a breaking pre-1.0 minor, not a0.3.x patch. No publication occurs here. |

259's Windows investigation remains owner-deferred,266 depends on merged259,
and268's diagnostic adaptation remains separate. No286 candidate source or
future288–292 execution is included in this iteration.

## Verified serial closing and local completion — 2026-09-28

The exact unchanged isolated237 test passed once at1467ms. Its diagnostic wrapper
remains failed: external tool39846exit1/controller2/test0. The post-test reader
compared serde's typed Unix OsString home to a string; strict offline decoding
and one positive/24negative controls resolved that private parser defect.
Missing later wrapper boundaries and its30second cleanup qualification remain
missing. The complete immediate snapshots and separate later root recovery
conserved536profiles, with protected/real state unchanged and no interventions.
No native test was repeated to repair the reader.

A fresh independent review accepted a narrower timing-observation contract for
one separate serial close. This supersedes only the prospective admission
sentence above; it does not qualify the failed wrapper or rewrite its outcomes.
C2 used both live gates, all six tiers, jobs1 and unchanged300/900second watchdogs.
Actual external tool61369, parent and sweep exited0; the parent took1252.629s.
All351names passed (CLI340, core1/3/3/2/2), with no skipped, preexisting, vanished,
launch-timeout or watchdog-timeout cases. All six profile summaries and the
aggregate show zero live-owned leaks/unattributed profiles. All341launch pairs
across17ledgers reconcile. The536prior profiles were conserved; six newly retained
fixtures are attributed, giving542. The owned raw browser's actual wait was-15
after its recorded cleanup TERM; no owned survivors remain. Protected desktop
Firefox and real profile state are unchanged. Mutable CLI bytes still match the
reviewed normal939976364e536505e2ef5212dfa0a3650ba0db187e05964cf69c89be0f910ff0.

This serial pass supplies287's own functional closing validation. It does not
explain C1's4893ms timing failure or prove default-six timing behavior. Plan293
retains that failure and its unknown cause; its tasks/AC remain0/3. Both original
224/240 scenarios passed in C2, without supplying268's missing attribution.
No conditional native replacement control applies because no replacement API
is retained. Original287 task/AC wording is unchanged, now3/3 locally satisfied.

Independent product review, ordered2719/0/429 workspace results and nine static
gates remain valid against unchanged590source inputs. Their detailed prior
receipts are reused, not rerun. Publication still requires final affected
plan/Hyalo/diff validation, committed-head actor/KB validation and exact-head CI;
no PR merge is claimed in this local completion record. Evidence is in private
`iter287/closing2-preparation1/`, including `tier-reconciliation.json`,
`launch-ledger-reconciliation.json` and `root-release.json`.

Affected-plan reconciliation:268's24file diagnostic candidate on provisional
964ee9ae is independently reviewed preparation only; after this iteration's
actual disposition its final base, inputs and producers must be qualified anew.
No unchanged mock replay is warranted.259 remains owner-deferred,266 dependent,
286 preserved unmerged, and288–293 filed/unexecuted. No broader plan status is
changed by this completion. The breaking-minor publication constraint above
remains binding; no crate version or release is changed.
