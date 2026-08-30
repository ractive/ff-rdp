---
title: "Iteration 223: --with-page cannot tell a fast outgoing-document answer from the real destination"
type: iteration
date: 2026-08-31
status: planned
branch: iter-223/with-page-outgoing-doc-race-detection
depends_on: [220]
first_call_sites:
  - primitive: (none yet — investigation first; see Themes)
    site: crates/ff-rdp-cli/src/commands/page_view.rs (collect_settled / settle_after_navigation)
dogfood_path: |
  ff-rdp launch --headless
  # Needs a route that answers a ~10-50KB collection eval in well under the
  # target-destroyed-form latency (observed 55ms on Wikipedia) while a
  # navigation is in flight. No such fixture exists yet — Theme A is building
  # one (a `/race` route serving a tiny, fast document as the outgoing page,
  # linking to a slow-committing destination).
  ff-rdp navigate <fixture>/race --with-page
  ff-rdp click --ref <link-to-slow-dest> --with-page --jq '.results.page.headings[0].text'
  # expected AFTER this iteration: either "slow-dest" (the real fix), or a
  # documented, tested failure mode if Theme A concludes detection is not
  # affordable — see Themes B.
tags: [iteration, act-and-see, page-view, carry-over, defect]
---

# Iteration 223: `--with-page` cannot tell a fast outgoing-document answer from the real destination

## Why

Carry-over from [[iteration-220-with-page-after-navigating-click]]'s closing sweep. iter-220
fixed the case where the outgoing document's collection eval is still in flight when Firefox
tears its docshell down — `set_target_guard` now catches that and retries.

It does **not** cover the narrower case iter-220's Outcome names as a residual, deliberately
left unfixed:

> When a navigation starts *and* the outgoing document answers the whole collection before its
> docshell is torn down, the view describes the outgoing page and nothing detects it.

Concretely: `click --ref <link>` fires `tabNavigated{state:"start"}`, and `collect_settled`'s
`settle_after_navigation` polls `getTarget` for up to 3s waiting for the `innerWindowId` (or
URL) to change. If Firefox answers the full collection eval against the **outgoing** docshell
before the settle loop's *next* poll observes the change — a small/cached outgoing page,
racing a `target-destroyed-form` that has not arrived yet — `collect_settled` returns a page
view of the page the action left, with no error and no signal that anything is wrong.

iter-220's own AC-2 evidence produced exactly this failure shape (`headings[0] == "Ada
Lovelace"`, the page the click left) when the fix was reverted — so the shape is real and
reachable; iter-220 fixed the reachable trajectory (Wikipedia: destination too slow for the
outgoing page to still be answerable) but not the adjacent one (outgoing page answers fast
enough to beat detection).

iter-220 judged the window "small" and chose not to file this at the time; filing it now
because the closing-sweep rule for this run requires every residual finding to get its own
plan rather than a comment.

## Themes

- **A — Reproduce it on purpose.** Build a fixture trajectory where the outgoing document is
  small enough to answer a full collection (headings + interactive scan + Readability run)
  before `target-destroyed-form` or the settle loop's own poll would catch it. If this cannot
  be reproduced against a realistic collection payload (the Readability pass alone is tens of
  KB of JS to inject and run — Theme A should measure whether that alone pushes every real
  collection past the observed 55ms teardown latency), say so with numbers and downgrade this
  iteration's scope to detection-only (Theme C) rather than a race fix.
- **B — If reproducible: close the window.** The cheapest fix is *not* "wait for every
  announced navigation to resolve before collecting" (iter-220 rejected that — it makes
  `--with-page` hostage to a redirect that never lands). Candidates worth costing out instead:
  re-check `getTarget`'s `innerWindowId`/`url` *after* collection completes, before returning,
  and retry if it still names the pre-navigation document; or have the collection eval itself
  report the `innerWindowId` it ran against (already visible to content-process JS) so the CLI
  can compare without a second round-trip.
- **C — If not affordably reproducible: detect and label it instead.** Failing that, at minimum
  make the returned page view distinguishable — e.g. compare the post-collection
  `innerWindowId`/`url` against what a navigation was announced for, and set
  `meta.page_ready = false` (or a new field) rather than reporting a confident wrong answer
  silently, matching how `page_ready` already reports the unrelated "collection wait timed
  out" case.

## Tasks

### A. Reproduce or bound the window [0/2]
- [ ] Fixture route(s) that isolate the race (fast outgoing page, slow-to-commit destination
      with a `target-destroyed-form` delay) and a live test that fails on `main`
      (post-iter-220) if the race is real
- [ ] If unreproducible against a realistic collection payload, measure and record why
      (timing numbers), and note the theme this narrows the iteration to

### B or C. Close the window or label it [0/1]
- [ ] Implement whichever of Theme B or Theme C the Task A finding points at, with a live test

## Acceptance Criteria [0/2]

- [ ] Either: the reproduction test from Task A fails on `main` and passes after the fix — OR:
      Task A's write-up in Outcome explains, with measured numbers, why no such test exists,
      and Task B instead ships detection/labeling with its own live test
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- Everything iter-220 already fixed (the in-flight-collection race, the navigation-latch, the
  target guard). This iteration is only the narrower "outgoing page answers before teardown is
  observed" gap iter-220's Outcome left as residual.

## References

- [[iteration-220-with-page-after-navigating-click]] — Outcome § "Residual, deliberately not
  fixed here"; PR #234's `## Carry-over` table
- `crates/ff-rdp-cli/src/commands/page_view.rs` — `collect_settled`, `settle_after_navigation`
- `crates/ff-rdp-core/src/transport.rs` — `set_target_guard`, `take_navigation_started`
