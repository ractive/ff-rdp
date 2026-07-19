---
title: "Iteration 122: default navigate burns ~7s on FF152 — dom-complete never fires, elapsed_ms + committed_url wrong"
type: iteration
date: 2026-07-18
status: planned
branch: iter-122/navigate-dom-complete-ff152
depends_on: []
firefox_refs: []
kb_refs:
  - kb/rdp/actors/watcher.md
first_call_sites: []
dogfood_path: |
  # Default navigate on a simple static page must NOT burn the full events budget
  # waiting for a dom-complete document-event that never fires on FF152:
  /usr/bin/time -p ff-rdp --port <p> navigate https://example.com
  # expected: wall time < 2s (was ~7.2s); result.elapsed_ms within ~500ms of wall time
  ff-rdp --port <p> navigate https://www.comparis.ch --jq '.results.committed_url'
  # expected: "https://www.comparis.ch/" (NOT "about:blank") on an SPA that starts at about:blank
tags:
  - iteration
  - navigate
  - watcher
  - rdp
  - firefox-152
  - dogfood-61
---

# Iteration 122: default navigate burns ~7s on FF152

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152), **CONFIRMED on a clean
single instance**: default `navigate` costs ~7s on simple static pages (example.com 7.26s, HN
7.14s) while `--no-wait` returns in 0.06s with the page already loaded. Root cause chain in
`crates/ff-rdp-cli/src/commands/navigate.rs`:

1. Default `--wait-strategy both` (`WaitStrategy::Both`, `navigate.rs:51-63`) runs the
   **events** phase first. `split_wait_budget` (`navigate.rs:413-417`) gives events ~70% of the
   timeout (7s of the default 10s).
2. `wait_for_doc_complete` (`navigate.rs:139-283`) subscribes to the `document-event` resource
   and blocks for the `dom-complete` event. On FF152 that event **does not fire** for these
   pages, so the full ~7s events budget is consumed before falling back to
   `wait_for_readystate_complete` (`navigate.rs:331-384`), which then succeeds almost instantly.
3. Two derived defects surface in the fallback path:
   - **`elapsed_ms` is off by ~7000×** — it reports only the readystate-poll duration (~1ms),
     not the wall-clock across both phases (`elapsed_ms` came from `poll_js_condition`, not from
     the navigate-start `Instant`).
   - **`committed_url` is `about:blank` on SPAs** — when no `dom-loading` event fires, `commit_url`
     stays `None` and `unwrap_or_default()` (`navigate.rs:244-254`) yields an empty string
     rendered as `about:blank`, even though `location.href` confirms the real URL landed. Repro'd
     on 4 comparis routes; `navigated` + `ready_state:complete` were correct.

Note this is FF152-/page-specific: comparis's document *did* fire `dom-complete` quickly (0.69s),
so the fix must speed up the pages that don't fire it without regressing the ones that do.

## Themes

- **A — Short-circuit the events wait when readyState is already complete.** Poll
  `document.readyState` (with a fresh `navigationStart`) concurrently with / interleaved into the
  events wait, and return as soon as it reaches `complete`, instead of blocking the full events
  budget for a `dom-complete` event that may never arrive on FF152.
- **B — Report honest `elapsed_ms` and a real `committed_url`.** Measure `elapsed_ms` from the
  single navigate-start `Instant` across both phases; when the event carries no URL, fall back to
  an `eval location.href` for `committed_url` (as the readystate path already does) rather than
  emitting `about:blank`.

## Tasks

### A. Fast-path readystate

- [ ] In the `Both` strategy, interleave a lightweight `document.readyState` poll into the events
      wait loop (`wait_for_doc_complete` drain loop, `navigate.rs:160-282`) so a page that is
      already `complete` returns without waiting out the events budget. Preserve the freshness
      guard (`navigationStart > pre_epoch`, iter-92) to avoid returning on a stale document.
- [ ] Re-tune / justify `split_wait_budget` given the interleaved poll (the 70/30 split loses its
      rationale if readystate is checked continuously).

### B. Honest timing + URL

- [ ] Thread the navigate-start `Instant` through both phases so `CommitInfo.elapsed_ms`
      (`navigate.rs:101-110`) reflects total wall-clock, not just the readystate poll.
- [ ] When the committing event carries no `url`, resolve `committed_url` via `eval
      window.location.href` before falling back to empty (`navigate.rs:244-254`).

## Acceptance Criteria [0/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_navigate_default_fast: default `ff-rdp navigate` to a static page (example.com class)
      returns in wall-clock `< timeout/2` on FF152 — no full events-budget burn when the page is
      already `complete`.
- [ ] live_navigate_elapsed_matches_wall: `result.elapsed_ms` is within a tolerance (±750ms) of
      externally-measured wall-clock for a default navigate, across the events→readystate fallback.
- [ ] live_navigate_spa_committed_url: navigating an SPA that starts at `about:blank` yields
      `committed_url == location.href` (a real URL), never `"about:blank"`, when the real URL has
      committed.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Prefer fixing the `Both` strategy over changing the default to `readystate` — `readystate`
  loses the richer document-event signal (e.g. `about:neterror` early-exit at `navigate.rs:188-200`)
  that events provides on pages that do fire it.
- `--no-wait` (0.06s) stays the escape hatch but skips commit verification; it is not the fix.

## Out of scope

- `navigate --with-network` JSON shape inconsistency (object vs array) — filed separately from
  dogfood-61; related but distinct serialization bug.
- The persistent-daemon autostart failure (dogfood-61 bug 8) — separate infra issue.

## References

- [[dogfooding-session-61]]
- [[iteration-92-full-page-and-navigate-parity]]
- [[watcher]]
