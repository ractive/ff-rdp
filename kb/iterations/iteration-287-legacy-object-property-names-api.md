---
title: "Iteration 287: Retire the unsupported object property-names API"
type: iteration
status: planned
date: 2026-09-28
branch: iter-287/legacy-object-property-names-api
depends_on:
  - "259"
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
  - path: devtools/shared/specs/object.js
    lines: "100-220"
    why: declared supported property operations
  - path: devtools/server/actors/object.js
    lines: "380-410"
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

## Tasks [0/3]

- [ ] Recheck callers, exported API expectations and package contract; choose
      removal/deprecation or an accurately specified supported implementation.
      Record compatibility consequences instead of silently changing semantics.
- [ ] Apply the smallest justified API repair and reconcile stale current
      documentation. Preserve iteration18's completed fix and historical notes
      through dated corrections, not rewritten historical outcomes.
- [ ] Add meaningful validation for the selected behavior, independent review,
      ordered workspace gates and required per-iteration closing validation.

## Acceptance Criteria [0/3]

- [ ] The maintained API no longer issues unsupported ownPropertyNames to the
      qualified Firefox version. Removal/deprecation or supported replacement
      has an explicit compatibility decision and no invented production caller.
- [ ] Existing eval/inspect property output remains correct. If a supported
      replacement is retained, own-property inclusion/order semantics are stated
      and demonstrated with a qualified native object and a repair-sensitive
      control; a renamed unsupported packet or fabricated fixture cannot pass.
- [ ] Documentation distinguishes the old repaired CLI issue from the leftover
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
