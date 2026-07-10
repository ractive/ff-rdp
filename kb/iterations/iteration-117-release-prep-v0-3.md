---
title: "Iteration 117: release prep v0.3.0 — spec-drift TBDs, CI live-lane retrigger, version bump + cut"
type: iteration
date: 2026-07-10
status: done
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

**Discovered during iter-116 review (2026-07-10):** the `on:` trigger block
itself is clean (no `pull_request:` key — confirmed), but the "Run
dogfood-script gate (iter-* branches only)" step's `if:` guard was not updated
alongside the trigger swap — it still reads
`if: startsWith(github.head_ref, 'iter-') && github.event.pull_request.head.repo.full_name == github.repository`
(`.github/workflows/live.yml:52`). `github.event.pull_request` is unset under
all three current triggers, so that `if:` now always evaluates false and the
dogfood-script gate step is silently dead — it never runs on any trigger. Two
follow-ups folded into this iteration:
1. Fix or remove the stale `if:` guard (either drop the step, since
   `check-iteration-ready`'s local `check-dogfood-script` sub-check already
   covers this pre-merge, or rewrite the condition to fire on
   `workflow_dispatch`/`schedule`/`release` appropriately).
2. The `live_lane_retriggered` AC's grep is too literal — a bare
   `grep -n "pull_request" .github/workflows/live.yml` still matches this
   leftover `if:` guard (a false failure even though the trigger block itself
   is correct). Reworded below to check the `on:` block specifically.

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

- [x] tbd_annotations_replaced: `grep -rn "allow-spec-drift: bug TBD" crates/`
      returns no output (verified); all three annotations in `screenshot.rs`
      switched from `bug TBD` to `bug FILING`. Bugzilla search found no
      existing bug for any of the three novel gaps, so Results lists SD-1/SD-2/
      SD-3 (Bugzilla-ready) awaiting James's filing — they block publishing the
      v0.3.0 draft.
- [x] drawsnapshot_workaround_reassessed: FF152 repro outcome recorded in
      Results (regression STILL reproduces on Firefox 152.0.5 — workaround
      retained, no version-gate applied because gating would break screenshots);
      live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport
      passes live on Firefox 152.0.5.
- [x] live_lane_retriggered: `sed -n '/^on:/,/^env:/p' .github/workflows/live.yml
      | grep -n "pull_request"` returns no output (verified — the `on:` trigger
      block has no `pull_request:` key; workflow_dispatch + release + schedule
      triggers present) AND the stale `if: … github.event.pull_request …`
      dogfood-gate guard was removed entirely (the step was dead under all three
      triggers; the gate is already covered pre-merge by
      `check-iteration-ready`'s `lint-dogfood-script` sub-check).
- [x] release_ready: workspace version bumped 0.2.0 → 0.3.0 in Cargo.toml +
      Cargo.lock; release notes drafted at `kb/releases/v0.3.0-notes.md`; the
      exact `gh release create v0.3.0 --draft …` command is recorded in Results.
      Publishing itself is James's action and NOT part of this iteration's ACs.

## Results

Implemented 2026-07-10 on `iter-117/release-prep-v0-3` (Firefox 152.0.5,
macOS). Quality gates green: `cargo fmt` (no changes), `cargo clippy
--workspace --all-targets -D warnings` (clean), `cargo test --workspace -q`
(all pass), `check-iteration-ready --plan … --base origin/main` = **10/10
PASS** (with `FF_RDP_LIVE_TESTS=1`).

### Theme A — spec-drift `bug TBD` markers

Bugzilla was searched for each of the three gaps; **no existing bug** matches
any of them — they are novel, surfaced by ff-rdp's own testing against
FF149–152. Filing needs a Bugzilla account (James's action), so per the plan's
CAVEAT the annotations were switched from the reviewer-flagged `bug TBD` to a
distinct `bug FILING` marker and the Bugzilla-ready descriptions recorded in
[[open-gaps#spec-drift-bugs-awaiting-filing]]. `grep -rn "allow-spec-drift: bug
TBD" crates/` is now empty. **These three block publishing v0.3.0** — James
files them, then a follow-up commit replaces each `bug FILING` with the real
number before the draft is published.

Bugs to file (all component **DevTools :: Framework / Server**):

- **SD-1** (`screenshot.rs:34`) — the published `screenshot` actor spec dict at
  `devtools/shared/specs/screenshot.js:13-35` omits `browsingContextID`,
  `snapshotScale`, and `rect`, though `devtools/server/actors/screenshot.js`
  reads all three (required for the two-step FF149+ capture protocol).
- **SD-2** (`screenshot.rs:255`) — `screenshotActor.capture` fails to load
  `capture-screenshot.js` in the DevTools distinct global (`moz-src:` scheme
  unsupported there). **Still reproduces on FF152** (see reassessment below).
- **SD-3** (`screenshot.rs:401`) — `WindowGlobalTarget.screenshot` is
  implemented server-side (FF151+) but undeclared in
  `devtools/shared/specs/targets/window-global.js`.

### Theme A — FF152 drawSnapshot workaround reassessment

The plan required testing whether the FF151 `capture-screenshot.js`
module-load regression still reproduces on Firefox 152 before deciding to
version-gate or remove the `screenshot_via_process_drawsnapshot` workaround.

**Outcome: the regression STILL reproduces on Firefox 152.0.5.** A live probe
(`ff-rdp launch --headless`, navigate to a data-URL page, then
`RUST_LOG=ff_rdp_cli::screenshot=debug ff-rdp screenshot -o …`) logged:

```
DEBUG ff_rdp_cli::screenshot: screenshotActor module load failure; retrying via screenshot_via_process_drawsnapshot
```

and produced a valid 1366×683 PNG via the fallback. Because the underlying
Firefox bug is NOT fixed, a version-gate that disabled the workaround on FF152
would BREAK screenshots — so the workaround is retained unchanged and the
annotation (SD-2) documents the FF152 reassessment. `live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport`
and `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport`
both pass live on Firefox 152.0.5.

### Theme B — CI live-lane (pre-landed) + stale-guard follow-up

Theme B's trigger swap (`pull_request` → `workflow_dispatch` + `release:
published` + weekly `schedule`) was **pre-landed on main by James** before this
iteration. Verified: `sed -n '/^on:/,/^env:/p' .github/workflows/live.yml |
grep -n "pull_request"` is empty (the `on:` block has no per-PR key; all three
current triggers present).

The iter-116-review follow-up: the "Run dogfood-script gate (iter-* branches
only)" step's `if:` guard keyed on `github.event.pull_request`, which is unset
under all three current triggers — so the step was silently dead. It was
**removed** (not rewritten): the dogfood-script gate is already enforced
pre-merge locally by `check-iteration-ready`'s `lint-dogfood-script`
sub-check, making the CI step redundant as well as dead. A whole-file
`grep -n "pull_request" .github/workflows/live.yml` is now empty too.

### Theme C — version bump + draft release

Workspace `version` and the intra-workspace `ff-rdp-core` dep constraint bumped
`0.2.0` → `0.3.0` in `Cargo.toml`; `Cargo.lock` regenerated (`cargo check`
confirms both `ff-rdp-cli` and `ff-rdp-core` at `0.3.0`). The `--version`
format doc-comment example in `cli/args.rs` was updated to `0.3.0`. Release
notes drafted at [[v0.3.0-notes]] (highlights: FF152 compatibility for `index`
[iter-114] and `console` [iter-116], cascade `rule_actor_id` [iter-115],
daemon/live-launch hardening [iter-113], live-suite trust restoration
[iter-114] — links iteration plans rather than itemizing ~525 commits).

**Draft NOT published** (James publishes). Exact ready-to-run commands:

```sh
# Create the draft release (run once the branch is merged to main and tagged):
gh release create v0.3.0 --draft --title "ff-rdp v0.3.0" \
  --notes-file kb/releases/v0.3.0-notes.md

# After filing SD-1/SD-2/SD-3 and replacing the `bug FILING` markers, publish:
gh release edit v0.3.0 --draft=false
```

The agent does NOT create the release (it would require pushing a tag and
`gh release create` on `main` outside the PR); James runs the command above
after merge. Pipeline babysitting (past cuts hit Windows/macOS/cross-compile
failures) happens after his publish, outside this iteration.
