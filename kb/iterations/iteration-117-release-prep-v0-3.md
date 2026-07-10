---
title: "Iteration 117: release prep v0.3.0 — spec-drift TBDs, CI live-lane retrigger, version bump + cut"
type: iteration
date: 2026-07-10
status: planned
branch: iter-117/release-prep-v0-3
depends_on:
  - kb/iterations/iteration-116-console-cache-start-listeners.md
firefox_refs: []
kb_refs:
  - kb/decision-log.md
first_call_sites: []
dogfood_path: |
  grep -rn "allow-spec-drift: bug TBD" crates/
  # expected: no output (every drift annotation carries a real Bugzilla number)
  grep -n "pull_request" .github/workflows/live.yml
  # expected: no output (lane runs on workflow_dispatch/release/schedule only)
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_92_screenshot -- --include-ignored
  gh release view v0.3.0 --json tagName,isDraft
  # expected: tag v0.3.0, isDraft TRUE (James publishes; agent never publishes)
tags:
  - iteration
  - release
  - ci
---

# Iteration 117: release prep v0.3.0

v0.2.0 is ~500 commits behind main; everything from roughly iter-66 onward is
unreleased, and users on Firefox 152 (current stable) have two broken commands
in the released binary (`index` — fixed in iter-114; `console` — fixed in
iter-116, hence the dependency). This iteration clears the release blockers
required by CLAUDE.md discipline, de-noises CI, bumps the version, and cuts
v0.3.0.

## Theme A — replace the three `allow-spec-drift: bug TBD` annotations

All three are in `crates/ff-rdp-core/src/actors/screenshot.rs` (CLAUDE.md:
`TBD` must become a real Bugzilla number before the next release cut; the
rdp-spec-reviewer agent flags any survivor):

1. `screenshot.rs:34` — `screenshot.args` spec dict
   (devtools/shared/specs/screenshot.js) omits
   `browsingContextID`/`snapshotScale`/`rect` though the server reads all
   three (tracked "via iter-78").
2. `screenshot.rs:255` — `BrowsingContext.drawSnapshot` parent-process-eval
   workaround for the FF151 capture-screenshot.js regression. FIRST test
   whether the regression still reproduces on Firefox 152: if fixed upstream,
   gate the workaround behind a version check (or remove it) as the
   annotation itself demands — a bug number alone is the wrong fix here.
3. `screenshot.rs:401` — `WindowGlobalTarget.screenshot` implemented
   server-side but undeclared in
   devtools/shared/specs/targets/window-global.js.

Method: search Mozilla Bugzilla for existing bugs covering each gap before
filing; file only genuinely novel ones. CAVEAT (James): filing needs a
Bugzilla account — if the implementing session cannot file, it annotates with
found existing bug numbers and lists the remaining novel gaps in Results for
James to file, replacing those TBDs in a follow-up commit before the cut.

## Theme B — retrigger the CI live lane (per DEC-022 context) [PRE-LANDED]

**Pre-landed on main 2026-07-10 (per James)** before this iteration's launch,
because the per-PR lane would otherwise stop the ralph loop's review agent on
this very iteration's PR (see [[iteration-115-cascade-rule-actor-id]]'s manual
finish and the project_ralph_advisory_lane_gotcha memory note). The change:
`live.yml` triggers are now `workflow_dispatch` + `release: {types: [published]}`
+ weekly `schedule` cron instead of `pull_request`; `continue-on-error: true`
kept. The implementing agent VERIFIES the AC grep below and records the
pre-landing in Results — no further Theme B work needed. Making a curated
runner-green subset blocking on release stays deferred until the first
release-triggered run shows what is environmental vs real.

## Theme C — version bump + cut v0.3.0

- Workspace `version = "0.2.0"` → `"0.3.0"` in the root Cargo.toml (verify
  the `0.2` intra-workspace dep constraint line still resolves).
- Pre-cut ritual: full serial `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
  cargo test-live` green except explicitly justified reds; TBD grep clean.
- **Prepare but do NOT publish** (per James, 2026-07-10): draft the release
  notes as highlights — FF152 compatibility (`index`, `console`), cascade
  `rule_actor_id`, daemon hardening, live-suite trust restoration — linking
  kb iteration plans rather than itemizing ~500 commits. Save the draft to
  `kb/releases/v0.3.0-notes.md` and create the release as a **draft**
  (`gh release create v0.3.0 --draft --notes-file …`) or record the exact
  ready-to-run publish command in Results. James reviews and publishes;
  pipeline babysitting (past cuts hit Windows/macOS/cross-compile failures)
  happens after his publish, outside this iteration.

## Out of scope

- Making the live lane blocking (deferred; needs curated subset data).
- Live-suite parallel-safety (DEC-022 revisit condition — only if serial
  sweep wall-clock starts to hurt).

## Acceptance criteria

- [ ] tbd_annotations_replaced: `grep -rn "allow-spec-drift: bug TBD" crates/`
      returns no output; each annotation carries a real Bugzilla number (or
      Results lists the ones awaiting James's filing, and those block the cut).
- [ ] drawsnapshot_workaround_reassessed: FF152 repro outcome recorded in
      Results; live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport
      passes live after any version-gate change.
- [ ] live_lane_retriggered: `grep -n "pull_request" .github/workflows/live.yml`
      returns no output; workflow_dispatch + release + schedule triggers
      present.
- [ ] release_ready: workspace version 0.3.0 merged; release notes drafted;
      v0.3.0 exists as an UNPUBLISHED draft release (or the exact publish
      command is recorded in Results); publishing itself is James's action
      and NOT part of this iteration's ACs.

## Results

(to be filled by the implementing iteration)
