---
title: "Iteration 280: Attribute the disappearance of four preexisting profiles"
type: iteration
date: 2026-09-23
status: planned
branch: iter-280/preexisting-profile-disappearance
# NN must be free: `ls kb/iterations/` before filing. check-iteration-plan fails when
# another plan already claims it. Letter suffixes (61b, 162a) are distinct numbers.
depends_on: []
# If this iteration introduces new pub items, list each one with its first call site.
# Leave empty ([]) if no new pub items are introduced.
# Required by cargo xtask check-iteration-plan when the body mentions pub symbols.
first_call_sites: []
# Describe how to manually exercise this iteration's output end-to-end.
# Required by cargo xtask check-iteration-plan.
dogfood_path: >-
  Planning only. Audit the finite retained iteration 273 baseline, launch,
  process, archive and scoped-removal records. Report an evidenced ownership/removal
  explanation or a bounded unattributed outcome. No live command is authorized; any
  future capture requires a fresh finite schedule and explicit authorization.
tags: [iteration]
# Add `skill-edit` if this iteration modifies files under ~/.claude/skills/.
# Skill-edit iterations cannot run through ralph-loop (the cmux child workspace
# has no write access to ~/.claude/skills/). Drive them by hand in a regular
# Claude session. See iter-61z for the canonical example.
---

# Iteration 280: Attribute the disappearance of four preexisting profiles

**Planning only.** This carry-over tracks iteration 273 final-review finding F1.
It is outside the authorized 272–278 execution queue; neither filing this plan nor
completing 273 authorizes its audit, implementation or a live capture. No product
removal defect is established. The observation itself remains to be explained.

## Retained observation

Iteration273's live1 preflight recorded these four existing directories. Each
contained owner-PID marker `74584` and had no readable `.ff-rdp-owner-test` marker:

| Exact baseline path | Owner PID | Owner-test marker |
| --- | --- | --- |
| `/Users/james/Library/Application Support/ff-rdp/profiles/ff-rdp-profile-84TYilR2JoskwjlJ` | 74584 | absent |
| `/Users/james/Library/Application Support/ff-rdp/profiles/ff-rdp-profile-LgtOtfK56dPK5eVW` | 74584 | absent |
| `/Users/james/Library/Application Support/ff-rdp/profiles/ff-rdp-profile-OWIDLu6InaOW0Ai3` | 74584 | absent |
| `/Users/james/Library/Application Support/ff-rdp/profiles/ff-rdp-profile-pFY1RurZaPEVvUIr` | 74584 | absent |

At the later archive/removal post-check all four were absent, so the broader
preexisting-preservation assertion failed. The scoped cleanup removed only the
four newly created live1 paths ending `FcaTZ0UYDgiPM17E`, `kYOu5t84i3J6iezX`,
`1iHARMaQyixAt0au` and `0XxxqYlrzWBenDAS`; that deletion list is disjoint from the
baseline. Those new profiles had independently verified ownership, root/group
absence, and a recoverable archive before removal. The failed broader post-check
must not be converted into proof that the older profiles were preserved.

The timing and actor of the older disappearance are unknown. PID 74584 in a marker
is not a birth-qualified ownership record. A static stale-pruning call in launch.rs
is a possible code path, not attribution for these exact deletions. The separate
historical launch hangs and273's passing diagnostic attempt establish no deletion
cause. The four missing baseline profiles are not claimed recoverable from the
archive of the four different live1 profiles.

Evidence root in the primary checkout:
`.git/ralph-loop/20260923-remaining272-278/iter273/`.

- `live1-preflight/profiles.json`: exact baseline paths and marker reads.
- `live1-preflight/report.json`, `live1-slot.json`, `live1/events.tsv`,
  `live1/supervisor.tsv`, `live1/launch-*.json`: actual run/identity boundaries.
- `closure/finish-profile-archive.py`, `closure/profile-cleanup-final.log` and
  `closure/profile-archive2/removal-receipt.json`: scoped deletion list, failed
  broader post-check and honest final reconciliation.
- `closure/profile-archive2/original-metadata.json`, recovery archive/receipts,
  `closure/final-profile-root.json`, retained process snapshots and manifests:
  what was preserved, removed, present or unavailable at the recorded times.
- `review-final/report.md`: why this measured anomaly requires a tracked plan.

## Tasks [0/3]

- [ ] After a future explicit 280 launch, perform one finite audit of the retained
      records above, with at most 30 active minutes. Freeze the input list and
      hashes first; include any already-retained ownership/cleanup receipts for
      these exact paths, without broad historical discovery. Build an interval
      ledger of observed presence/absence and actual actions. Stop once the
      explanation is evidenced or the declared records are exhausted.
- [ ] For each baseline path, record an evidenced expected/owned-removal
      explanation, or an explicit bounded unattributed outcome with the missing
      observation needed to distinguish ownership and removal actors. Do not
      infer deletion from PID absence, a marker alone, a later pass or static code.
- [ ] If the audit selects a concrete product or cleanup defect, propose a scoped
      repair and meaningful regression for independent review; otherwise close
      the bounded audit honestly without claiming resolution. Any further live
      work requires a fresh finite schedule and explicit authorization first.

## Acceptance Criteria [0/3]

- [ ] Every exact baseline path is traceable through the retained marker/presence
      records, failed broader post-check and disjoint scoped deletion list;
      unavailable timestamps, ownership and actor evidence remain explicit.
- [ ] The finite record audit either attributes these exact removals to evidenced
      actions/ownership or records a bounded unattributed outcome and the precise
      evidence gap, without asserting a product deletion or resolved anomaly.
- [ ] Any proposed repair follows the established mechanism with a regression
      that fails for it and independently reviewed ownership guarantees. If no
      repair is justified, leave this conditional item unticked and state that
      no repair or live validation is claimed.

## Execution and reopening boundary

The initial audit is read-only and uses existing records only. Do not launch
Firefox, run tests/captures, remove profiles, signal processes, change source,
or perform remote actions under this planning entry. A future live schedule
must declare the hypothesis, positive/negative controls, finite attempt count,
readiness/execution/cleanup bounds, process birth/ownership and filesystem-action
observations, preserved defaults and stop conditions before separate authorization.
No blind retries or full-sweep discovery. A bounded unattributed audit may close;
reopen it only for newly available relevant records or a separately authorized
recurrence that can fill the named gap. No budget or capture allowance transfers
from 273, and no original 273/272 criteria or statuses are weakened.

## References

- [[iteration-273-contended-launch-output-hang]] — original bounded hang outcome
  remains complete without attributing this separate profile observation.
