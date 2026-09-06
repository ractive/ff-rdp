---
title: "Iteration 237: act-and-see timing: type --submit under-reports navigated; click not-found waits the full --timeout"
type: iteration
date: 2026-08-29
status: planned
branch: iter-237/submit-navigation-grace-period
depends_on: [210]
first_call_sites:
  - primitive: ff_rdp_cli::commands::type_text::navigated_away
    site: crates/ff-rdp-cli/src/commands/type_text.rs (press_enter_and_submit's second navigated_away
      call, after form.requestSubmit())
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Main_Page
  ff-rdp dom "input[name=search]" --jq '.results[0].ref'
  ff-rdp type --ref <REF> --text "Turing Award" --submit --with-page \
    --jq '{submitted: .results.submitted, navigated: .results.navigated, method: .results.method, heading: .results.page.headings[0]}'
  # expected today: {"submitted":true,"navigated":false,"method":"request_submit",
  #   "heading":{"level":1,"text":"Turing Award"}} — navigated is false despite the heading
  #   proving a real cross-document navigation happened
  # --- from iteration 238 ---
  ff-rdp launch --headless
  ff-rdp navigate 'https://example.com'
  time ff-rdp click --selector '#definitely-not-on-the-page'
  # TODAY: ~10s wall clock (the default --timeout) before "0 elements matched (not found)".
  # AFTER: the poll still gives a late-appearing element its full budget, but a selector that
  # never matches anything reachable in the DOM is reported well under the timeout.
  ff-rdp daemon stop
tags: [iteration, cli, agent-ergonomics, refs, bugfix, carry-over, click]
---

# Iteration 237: act-and-see timing: `type --submit` under-reports `navigated`; `click` not-found waits the full `--timeout`

> **Renumbered 215 → 237 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 215.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-238-click-not-found-poll-timeout]] as Part B. One branch, one PR, one carry-over sweep for both parts.

## Part A: `type --submit` under-reports `navigated` after a slow `requestSubmit()`

Found while manually verifying [[iteration-210-act-and-see]]'s PR #230 review fixes live against
Wikipedia. `type --ref <search box> --text "Turing Award" --submit --with-page` correctly submits
the form (synthetic Enter does nothing on Wikipedia's search box, so the `requestSubmit()`
fallback fires) and correctly returns the *destination* page under `results.page` — the iter-210
fix for stale-actor reuse after a navigating submit is working. But `results.navigated` reports
`false`, even though `results.page.headings[0]` visibly changed from "Main Page" to "Turing
Award" in the same response — the two fields of one envelope disagree with each other.

### Root cause

`press_enter_and_submit` (`crates/ff-rdp-cli/src/commands/type_text.rs`) calls
`navigated_away(ctx, console_actor, &url_before, ENTER_NAVIGATION_GRACE_MS)` after
`form.requestSubmit()`, where `ENTER_NAVIGATION_GRACE_MS = 600`. That constant was sized for the
*first* call (right after the synthetic Enter, where the question is "did the untrusted keydown
alone do anything" — it usually didn't, and 600ms was enough to be sure). The *second* call,
after a real `requestSubmit()` against a remote origin, is answering a different question — "did
the network round-trip complete" — and 600ms is frequently not enough for that over a real
connection. `navigated_away` (as of iter-210's review-fix pass) now correctly reads a hard
`noSuchActor`/`EvalNavigatedDuringEval` protocol error as "navigated"; it still reads a plain
timeout as "not navigated" per its own doc comment ("a timeout means no navigation observed...
not an error"), and on a slow-but-successful requestSubmit that is the wrong read: the docshell
had not yet been torn down when the 600ms grace period expired, so the poll saw neither the
`Ok(true)` from a settled `location.href` nor a hard protocol error — it just ran out of time.

Not a regression from the iter-210 review-fix commit: this constant, and this second call site,
predate that commit. The review fix changed what `Err(_)` means; it did not touch the grace
period or `Ok(_)`'s timeout path.

### Themes

- **A — Give the post-`requestSubmit()` poll room to observe a real navigation.** Either a longer,
  separate grace period for this call site than the post-Enter one, or reuse the command's own
  `--timeout`/auto-wait budget instead of a small fixed constant.

### Tasks

#### A. Fix the second `navigated_away` call [0/2]
- [ ] Give `press_enter_and_submit`'s post-`requestSubmit()` `navigated_away` call its own
      constant (e.g. `REQUEST_SUBMIT_NAVIGATION_GRACE_MS`) sized for a real network round-trip —
      or thread `wait_timeout_ms` through so it honours `--timeout` like the rest of the command —
      rather than reusing `ENTER_NAVIGATION_GRACE_MS`
- [ ] Document, at the call site, why the two `navigated_away` calls in `press_enter_and_submit`
      need different budgets (post-Enter: "did the untrusted keydown do anything, fast local
      check" vs. post-`requestSubmit`: "did the network round-trip land")

### Acceptance Criteria [0/1]

- [ ] `live_type_submit_navigates_search_form` (or a new live test) asserts `results.navigated ==
      true` when `results.page`'s heading demonstrably changed — the two fields must agree

### Design notes

- **Don't just raise `ENTER_NAVIGATION_GRACE_MS` globally.** The post-Enter call benefits from
  staying short — it is the local-only check that decides whether to bother calling
  `requestSubmit()` at all, and a slow local check just adds latency to the (usual, per the
  isTrusted ceiling) no-op case. Only the post-submit call needs the longer budget.
- **`results.navigated` disagreeing with `results.page` is the actual bug**, not "false" being
  wrong in isolation — a caller reading only `results.navigated` (no `--with-page`) has no way to
  catch this today.

### Out of scope

- Anything about `--with-page`'s own correctness after a navigating submit — already fixed and
  live-verified in [[iteration-210-act-and-see]]'s review-fix pass.

### References

- [[iteration-210-act-and-see]] — carry-over row that filed this plan; the review-fix pass that
  found it live
- `crates/ff-rdp-cli/src/commands/type_text.rs` — `press_enter_and_submit`, `navigated_away`,
  `ENTER_NAVIGATION_GRACE_MS`

## Part B: `click`'s not-found poll costs a full timeout on a guessed selector (absorbed from iteration 238)

> **Renumbered 232 → 238 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 232.

### Why

Carried unfixed across two iterations without ever getting its own plan:

- [[iteration-228-two-task-benchmark-after-facts]] first observed it: `link_follow` runs 1 and 2
  both guessed `a[href="/wiki/Charles_Babbage"]`, which really does match zero elements (Wikipedia
  writes that attribute as a relative URL, not absolute), and each guess cost the full 10s
  `--timeout` before `click` reported `0 elements matched (not found)`. Filed as "observed,
  deliberately not acted on" — no turn cost, so out of scope there.
- [[iteration-230-quickstart-navigate-with-page]]'s carry-over table repeated the same disposition
  — "no plan, with reason" — because the defect did not fire in any of that iteration's six
  re-measurement runs (all six clicked by `--ref`, not a guessed selector), and named the
  condition under which it would need a plan: "if a run loses turns to it... it needs its own
  plan."
- [[iteration-255-infobox-facts-refs-and-query-matching]] repeated the "out of scope" note a third
  time without changing the disposition.

That condition — turn cost, not just wall-clock cost — has not yet fired in a measured benchmark
run. But wall-clock cost is a real cost on its own (a 10s stall reads as a hang to anything
watching the process, and CI/dogfood scripts pay it directly), and three iterations citing the same
unfiled defect is itself the thing the carry-over sweep exists to catch. This plan is the fix, not
a fourth deferral.

### Root cause (to confirm, not assumed)

`crates/ff-rdp-cli/src/commands/click.rs`'s `autowait_element` polls the top-level document for a
selector match up to `wait_timeout_ms` (defaulting to `cli.timeout`, 10s). The poll exists because
a selector that has not rendered *yet* — content behind a pending XHR, a route transition — needs
to be retried, and that is the correct behavior for a selector that will eventually match. The
defect is that the same budget is spent on a selector that provably never will: nothing in the DOM
resembles it and nothing is still loading. The fix has to distinguish those two cases, not just
shrink the timeout (which would make the legitimate "not rendered yet" case flaky).

### Themes

- **A — detect the stable-and-empty case early.** If the page has reached a stable/idle state
  (no pending network activity `click` already tracks for `--wait-for-network`, no DOM mutations
  in the last poll interval) and the selector still matches zero elements, there is nothing left
  to wait for — report not-found without spending the rest of the budget. A selector that matches
  late because of an in-flight fetch must still get its full timeout; only the case where the page
  is provably done changing should short-circuit.
- **B — do not regress the retry case.** Every existing live test that relies on `autowait_element`
  retrying a selector that appears after a delay must still pass unmodified — this is a
  short-circuit on top of the existing poll, not a replacement for it.
- **C — measure the actual saving.** Time the dogfood reproduction above before and after; record
  both numbers in the plan rather than asserting the fix worked from the diff alone.

### Tasks

#### A. Stable-and-empty short-circuit [0/3]
- [ ] Define "stable" precisely (reuse whatever `settle_page`/network-idle signal `click` already
      computes for `--wait-for-network`, rather than inventing a second notion of idle)
      firstly check `crates/ff-rdp-cli/src/commands/click.rs` for the existing signal before adding
      one
- [ ] Wire the short-circuit into `autowait_element`'s poll loop, gated so it only fires once the
      page is stable, never before
- [ ] Unit test: a selector that appears after 2 polls still resolves (retry case unregressed);
      a selector that never appears on a page that goes stable at poll N reports not-found at
      poll N+1, not at the full timeout

#### B. Live coverage [0/2]
- [ ] Live Firefox test: guessed selector against a static page (immediately stable) resolves
      not-found in well under `--timeout`
- [ ] Live Firefox test: selector that appears after a deliberate delay (dynamically inserted via
      `eval`) still resolves successfully within the existing timeout budget — the regression case
      Theme B exists to prevent

#### C. Measure [0/1]
- [ ] Re-run this plan's `dogfood_path` reproduction before and after; record both wall-clock
      numbers here

### Acceptance Criteria [0/4]

- [ ] A selector that matches nothing on a page that has gone stable reports not-found in
      measurably less than `--timeout` (record the before/after numbers, not just "faster")
- [ ] Every existing live test covering `autowait_element`'s retry behavior (a selector that
      appears late) still passes unmodified
- [ ] `cargo run -p xtask -- live-sweep` clean with both env gates set
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean. (covers both parts)

### Out of scope

- Lowering the default `--timeout`. That trades away the legitimate retry case for every caller,
  not just the guessed-selector case this plan targets.
- Any change to how `click --ref` resolves (it does not poll a selector at all — refs are already
  a stable page-view handle, so this defect does not apply to the act-and-see idiom that
  [[iteration-230-quickstart-navigate-with-page]] pushed adoption toward).

### References

- [[iteration-228-two-task-benchmark-after-facts]] — first observation, "deliberately not acted on"
- [[iteration-230-quickstart-navigate-with-page]] — carry-over row repeating the disposition
- [[iteration-255-infobox-facts-refs-and-query-matching]] — carry-over row repeating it a third time
