---
title: "Iteration 220: --with-page hangs after a click that navigates a heavy page"
type: iteration
date: 2026-08-30
status: done
branch: iter-220/with-page-after-navigating-click
depends_on: []
first_call_sites:
  - primitive: ff_rdp_cli::commands::page_view::attach
    site: >-
      crates/ff-rdp-cli/src/commands/page_view.rs (already the sole `--with-page`
      entry point; this iteration changes what it does before evaluating, and adds no new
      pub item)
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
tags:
  - iteration
  - act-and-see
  - page-view
  - defect
  - carry-over
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

### A. Fix the collection path [3/3]
- [x] Reproduce in a test before changing anything (Theme B's fixture)
- [x] Make `attach` collect against actors that belong to the document the action produced
- [x] Verify on Wikipedia by hand: `click --ref … --with-page` returns the destination view

### B. Regression cover [2/2]
- [x] `live_220_*`: a click that navigates to a slow-committing route, with `--with-page`
- [x] The same for `type --submit --with-page`, which takes the identical path

### C. Error message [1/1]
- [x] A recv timeout on a page-view eval names the navigation as the likely cause

## Acceptance Criteria [4/4]

- [x] `ff-rdp click --ref <link> --with-page` on `en.wikipedia.org/wiki/Ada_Lovelace` returns
      the destination page's view in under 5 s, 3 runs out of 3 (recorded in Outcome)
- [x] A live test fails on `main`'s collection path and passes after the fix
- [x] `type --submit --with-page` covered by the same test shape
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Outcome

### What the wire actually said

The plan's diagnosis was right about the symptom and half right about the mechanism.
Traces (`--log-level trace`) of the failing call on both routes:

- `refresh_target()` **does** return, and it returns a *fresh* actor prefix
  (`child81/` → `child83/`) — which is why the existing comment's "refresh first" reasoning
  looked sound. But the frame it returns carries the **same `innerWindowId` and the same
  `url`**: `15032385539`, `…/Ada_Lovelace`. Firefox re-forwards the **outgoing** docshell
  under a new prefix. There is no escaping the doomed document by refreshing.
- Daemon route: the 12 KB collection eval is accepted, then `target-destroyed-form`
  (`innerWindowId 15032385539`) arrives 55 ms later and the `evaluationResult` never comes.
- Direct route (`--no-daemon`): **no `target-destroyed-form` at all** — nothing subscribed
  that connection to target watching. There the collection *succeeds* against the outgoing
  document, ships a 251 KB view as a chunked LongString, and hangs on the 52 KB Readability
  re-injection eval instead. Same defect, one step later.
- `tabNavigated {state: "start", url: <destination>}` arrives on **both** routes, before the
  action's own `evaluationResult`. It is the only signal common to both, and it is the one
  the fix is built on.

### The fix

1. `RdpTransport::recv` — the single choke point every packet passes — latches the
   destination URL of any `tabNavigated{state:"start"}` / `willNavigate`
   (`take_navigation_started`).
2. `page_view::collect_settled` (new, replaces the bare `refresh_target()` in `attach`):
   when a navigation was announced, poll `getTarget` at 50 ms until the target reports a
   different `innerWindowId` **or** the announced URL, capped at 3 s; then collect. Nothing
   announced → collect immediately, as before.
3. `RdpTransport::set_target_guard(Some(innerWindowId))`, armed **only** around the
   collection: a `target-destroyed-form` for that document, or a navigation starting
   mid-collection, becomes `ProtocolError::EvalTargetDestroyed` → `AppError::RdpActorDestroyed`
   in tens of milliseconds instead of a `--timeout`-long stall. `collect_settled` re-settles
   and collects again, up to 3 attempts.
4. Theme C: `AppError::RdpTimeout` gained a `hint` field and `AppError::with_timeout_hint`;
   a recv timeout out of the page-view path now names the navigation as the likely cause.

`TargetInfo` gained `inner_window_id` and `url` (both already in the `getTarget` frame,
both previously discarded), which is what makes the settle test possible at all.

### Measured, 2026-08-30, `en.wikipedia.org/wiki/Ada_Lovelace` → `Charles_Babbage`

| route | before | after |
|---|---|---|
| daemon, `click --ref e19 --with-page` | recv timeout 3/3 (8 s, 12 s, 15 s budgets) | **0.61 / 0.62 / 0.65 s**, `"Charles Babbage"` 3/3 |
| direct, `click 'a[href$="/wiki/Charles_Babbage"]' --with-page --no-daemon` | recv timeout 3/3 (8 s, 12 s, 45 s budgets) | **0.54 / 0.51 / 0.49 s**, `"Charles Babbage"` 3/3 |
| `navigate --with-page` (control) | 0.41 s | 0.41 s |

### AC 2 evidence

`live_220_navigating_action_with_page` — 5 tests, all green after the fix. With
`collect_settled` swapped back for `main`'s `refresh_target(); collect(…)` (temporary local
edit, reverted), **3 of the 5 fail**, each reporting `headings[0] == "Ada Lovelace"` —
the page the action left — not a timeout. The fixture's `/slow` route sleeps 700 ms before
its first byte and then serves 400 links, which turns the race main loses by luck into the
one it loses every time.

The two remaining tests are the cost side and pass on both: a non-navigating click and a
`#fragment` click must return well under the 3 s settle budget. The fragment case is why
the settle loop has a URL exit as well as an `innerWindowId` exit.

### Live sweep

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=313 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=313
  CLI tier   301 passed / 3 failed
  core tiers   1 + 3 + 3 + 2 passed / 0 failed
```

Reconciles: 301 + 3 + 1 + 3 + 3 + 2 = 313 = `executed`. Port 6000 carried a raw
`firefox -no-remote --start-debugger-server 6000 --headless` (never `ff-rdp launch`), which is
why `preexisting=0`.

All three reds are carried over — see the PR's `## Carry-over` table.
[[iteration-221-live-166-cached-example-com]] and
[[iteration-222-live-123-daemon-autostart-under-load]] are filed.

### Residual, deliberately not fixed here

When a navigation starts *and* the outgoing document answers the whole collection before its
docshell is torn down, the view describes the outgoing page and nothing detects it. The
window is small (the guard closes it the moment Firefox says anything) and closing it fully
would mean refusing to collect until every announced navigation resolves — which would make
a `--with-page` call hostage to a background redirect that never lands. Filed as a note
here rather than a carry-over iteration: no observed trajectory hits it.

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
