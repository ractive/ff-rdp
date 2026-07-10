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
  # expected: tag v0.3.0, isDraft false, release.yml run green
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

## Theme B — retrigger the CI live lane (per DEC-022 context)

`live.yml` currently runs on every `pull_request`: ~27 min/PR,
`continue-on-error`, permanently red from environmental runner failures —
cost without signal (the real live gate is local, enforced per-iteration).
Change triggers to `workflow_dispatch` + `release: {types: [published]}` +
weekly `schedule` cron; keep `continue-on-error: true` for now. Making a
curated runner-green subset blocking on release is explicitly deferred until
the first release-triggered run shows what is environmental vs real.

## Theme C — version bump + cut v0.3.0

- Workspace `version = "0.2.0"` → `"0.3.0"` in the root Cargo.toml (verify
  the `0.2` intra-workspace dep constraint line still resolves).
- Pre-cut ritual: full serial `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
  cargo test-live` green except explicitly justified reds; TBD grep clean.
- Publish GitHub release `v0.3.0` (release.yml triggers on publish; includes
  provenance attestation per iter-75). Release notes as highlights — FF152
  compatibility (`index`, `console`), cascade `rule_actor_id`, daemon
  hardening, live-suite trust restoration — linking kb iteration plans rather
  than itemizing ~500 commits.
- Babysit the pipeline: past cuts hit Windows/macOS/cross-compile failures
  (see kb/research + memory); budget a fix-forward pass rather than assuming
  fire-and-forget.

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
- [ ] release_cut: workspace version 0.3.0; GitHub release v0.3.0 published;
      release.yml run green with artifact attestation (run URL recorded in
      Results).

## Results

(to be filled by the implementing iteration)
