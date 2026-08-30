---
title: "Iteration 220: --with-page hangs after a click that navigates a heavy page"
type: iteration
date: 2026-08-30
status: planned
branch: iter-220/with-page-after-navigating-click
depends_on: []
first_call_sites:
  - primitive: ff_rdp_cli::commands::page_view::attach
    site: crates/ff-rdp-cli/src/commands/page_view.rs (already the sole `--with-page` entry point; this iteration changes what it does before evaluating, and adds no new pub item)
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page \
    --jq '[.results.page.interactive[] | select(.name == "Charles Babbage")][0].ref'
  # expected: a ref, e.g. "e19" — works today
  time ff-rdp click --ref e19 --with-page --jq '.results.page.headings[0].text'
  # TODAY: {"error":"operation timed out after 20000ms (phase: recv)"} after the full
  #        --timeout budget, reproducible 3 runs out of 3
  # expected AFTER this iteration: "Charles Babbage", in about a second
  ff-rdp click --ref e19 --jq '.results.clicked'
  ff-rdp scroll top --with-page --jq '.results.page.headings[0].text'
  # the same two steps as separate processes already work — which is the clue
tags: [iteration, act-and-see, page-view, defect, carry-over]
---

# Iteration 220: `--with-page` hangs after a click that navigates a heavy page

## Why

`click --ref <link> --with-page` — the two-command trajectory
[[iteration-210-act-and-see]] exists to enable and [[iteration-219-reader-view-page]]
made worth taking — **times out on Wikipedia**. It has never worked there.

Reproduced on 2026-08-30 while closing iteration 219, on `en.wikipedia.org/wiki/Ada_Lovelace`:

| binary | command | result |
|---|---|---|
| iter-219 branch | `click --ref e19 --with-page` | recv timeout at 10 s, at 20 s, at 30 s — 3/3 |
| iter-219 branch | `click --ref e19` (no flag) | `clicked: true` in 0.22 s, lands on the Babbage article |
| iter-219 branch | `scroll top --with-page` (separate process, after the click) | full reader view in ~0.3 s |
| **`main`** (built from `0a87d1d`) | `click 'a[href*="Charles_Babbage"]' --with-page` | **recv timeout at 30 s** |

The `main` row is the important one: the hang predates the Readability injection and is
**not** an iteration-219 regression. It is an iter-210 defect that the local
two-page fixture in `live_210_act_and_see` and `live_219_reader_view` cannot reach, because
those documents commit before the collector runs.

## Diagnosis so far

`page_view::attach` calls `ctx.refresh_target()` and then evaluates on
`ctx.target.console_actor`. On a page whose navigation is still in flight, `TabActor::
get_target` hands back the **outgoing** docshell's actors; that console actor is destroyed a
moment later, Firefox sends no reply to the eval it was given, and the client sits on the
socket until `--timeout` expires (`phase: recv`).

Evidence for that reading, all from the runs above:

- `--no-wait` does not help, so the stall is inside `attach`, not in `click`'s own wait.
- A **fresh connection** opened after the click collects the same page in ~0.3 s. That is
  exactly the escape `navigate --auto-consent --with-page` already takes: `run` defers
  collection to a second `connect_and_get_target` because the consent click invalidates the
  first connection's actors (see `navigate.rs`, `defer_with_page`).
- `navigate --with-page` is unaffected — it waits for the document to commit before the
  collector ever runs.

## Themes

- **A — Do not evaluate against a docshell that is going away.** Either wait for the
  navigation the action started to commit before refreshing the target, or collect on a fresh
  connection the way `defer_with_page` does. Prefer whichever can be shown to work without
  spending a timeout budget first: a retry keyed on `AppError::Timeout` is correct but costs
  the caller the whole `--timeout` before it helps, which on the default 10 s is worse than
  the bug for anyone who scripts it.
- **B — A live test on a document that does not commit instantly.** The current fixtures are
  two-element pages served from an in-process HTTP server; they commit before the collector
  can race them, which is why 3 live suites and 2 iterations missed this. The fixture needs a
  destination that is slow to commit — a route that sleeps before its first byte, or one that
  serves a few hundred KB — so the race is the *normal* case in the test rather than the
  unlucky one.
- **C — Say something better than `phase: recv`.** A timeout whose real cause is "the actor
  you were talking to was destroyed by a navigation" should say so. `AppError::Timeout`
  carrying the phase is right; the message is not actionable.

## Tasks

### A. Fix the collection path [0/3]
- [ ] Reproduce in a test before changing anything (Theme B's fixture)
- [ ] Make `attach` collect against actors that belong to the document the action produced
- [ ] Verify on Wikipedia by hand: `click --ref … --with-page` returns the destination view

### B. Regression cover [0/2]
- [ ] `live_220_*`: a click that navigates to a slow-committing route, with `--with-page`
- [ ] The same for `type --submit --with-page`, which takes the identical path

### C. Error message [0/1]
- [ ] A recv timeout on a page-view eval names the navigation as the likely cause

## Acceptance Criteria [0/4]

- [ ] `ff-rdp click --ref <link> --with-page` on `en.wikipedia.org/wiki/Ada_Lovelace` returns
      the destination page's view in under 5 s, 3 runs out of 3 (recorded in Outcome)
- [ ] A live test fails on `main`'s collection path and passes after the fix
- [ ] `type --submit --with-page` covered by the same test shape
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Out of scope

- Anything about *what* the page view contains — that is [[iteration-219-reader-view-page]].
- The benchmark re-measurement, which is [[iteration-213-act-and-see-benchmark-rerun]]
  Theme A's harness. Note it cannot produce a fair `wikipedia_link_follow` number until this
  is fixed: the trajectory it measures is exactly the one that times out.

## References

- [[iteration-210-act-and-see]] — introduced `--with-page` and `page_view::attach`
- [[iteration-219-reader-view-page]] — found this while closing; its live tests pass because
  its fixtures commit instantly
- [[iteration-213-act-and-see-benchmark-rerun]] — blocked by this for the click-through tasks
- `crates/ff-rdp-cli/src/commands/navigate.rs` — `defer_with_page`, the existing fresh-connection
  escape for the same class of problem
