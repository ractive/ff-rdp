---
title: "Iteration 237: act-and-see timing: type --submit under-reports navigated; click not-found waits the full --timeout"
type: iteration
date: 2026-08-29
status: in-progress
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

#### A. Fix the second `navigated_away` call [2/2]
- [x] Give `press_enter_and_submit`'s post-`requestSubmit()` `navigated_away` call its own
      constant (e.g. `REQUEST_SUBMIT_NAVIGATION_GRACE_MS`) sized for a real network round-trip —
      or thread `wait_timeout_ms` through so it honours `--timeout` like the rest of the command —
      rather than reusing `ENTER_NAVIGATION_GRACE_MS`
      [2026-09-07: `REQUEST_SUBMIT_NAVIGATION_GRACE_MS = 3_000`, capped by `--timeout` via
      `request_submit_grace_ms`. **This alone did not fix the defect** — see "What the plan got
      wrong" below.]
- [x] Document, at the call site, why the two `navigated_away` calls in `press_enter_and_submit`
      need different budgets (post-Enter: "did the untrusted keydown do anything, fast local
      check" vs. post-`requestSubmit`: "did the network round-trip land")

### What the plan got wrong (recorded 2026-09-07, rather than reworded away)

The plan's root cause — "600 ms is frequently not enough for a network round-trip" — is true but
is **not** what produces `navigated: false`. Measured against the plan's own `dogfood_path`
reproduction with `REQUEST_SUBMIT_NAVIGATION_GRACE_MS` already at 3 s:

```
{"submitted":true,"navigated":false,"method":"request_submit","heading":"Turing Award"}
```

Unchanged from `main`. While Firefox commits the new document it stops answering
`evaluateJSAsync` on the pre-submit console actor altogether, so `navigated_away`'s *first* poll
iteration blocks on the socket read for the **transport's** deadline (`--timeout`, 10 s by
default). That single read outlives any grace period shorter than it: the loop wakes with
`ProtocolError::Timeout`, finds its own deadline long past, and returns "no navigation" having
never completed one probe. Widening the budget moves the boundary and not the outcome — which is
why the plan's Theme A ("a longer, separate grace period *or* thread `--timeout` through") would
have failed either way.

A second thing the plan did not anticipate, found by the `/slower` live fixture rather than by
Wikipedia: a *single* re-read after the grace period is also too early. A destination that sends
its first byte 4.5 s in has not committed when the grace period expires, so one look at
`location.href` sees the origin URL and is wrong for the same reason.

The fix that actually closes it is a third option the plan did not consider: `navigated_after_refresh`
asks the question where it can still be answered — drop the torn-down target, re-resolve the tab's
fronts, and *poll* `location.href` on the actor that now exists for whatever is left of the
caller's `--timeout`. That is the same recovery `--with-page` already performs, which is exactly
why `results.page` was right about the destination while `results.navigated` was wrong about
reaching it. The grace-period change is kept because it is independently correct (a submission that
commits inside 3 s is answered by the cheap poll, with no extra target round-trip), but it is the
belt, not the braces.

### The latency the fix would otherwise have cost, and the gate that prevents it

Polling for the remaining `--timeout` is right for a load that is genuinely in flight and badly
wrong for a form that will never navigate: every AJAX form (a `submit` handler calling
`preventDefault()`) would have gone from ~600 ms to the full 10 s — a worse regression than the
bug being fixed. So `build_request_submit_js` now records `e.defaultPrevented` from a one-shot
`submit` listener and reports it as `cancelled`, and `press_enter_and_submit` enters the extended
poll only when the submission was *not* cancelled. An uncancelled submit guarantees a
cross-document load, so waiting for it is warranted; a cancelled one returns immediately.
`live_237_cancelled_submit_does_not_wait_out_the_timeout` is the regression guard — it fails on
exactly the 10 s stall that removing the gate produces.

Measured after (same command, same page as the reproduction above):

```
{"submitted":true,"navigated":true,"method":"request_submit","heading":"Turing Award"}
```

### Acceptance Criteria [1/1]

- [x] `live_type_submit_navigates_search_form` (or a new live test) asserts `results.navigated ==
      true` when `results.page`'s heading demonstrably changed — the two fields must agree
      [2026-09-07: two new live tests, both in
      `crates/ff-rdp-cli/tests/live/live_237_act_and_see_timing.rs`.
      `live_237_submit_navigated_agrees_with_the_page_it_reports` uses a destination that commits
      after 1.2 s (inside the widened grace period);
      `live_237_submit_navigated_survives_a_destination_slower_than_the_grace` uses one that
      commits after 4.5 s, past the grace period entirely, so only the refreshed-target re-check
      can answer it. The second test exists because the first would have passed on the grace
      period alone and would therefore not have caught the real defect. A third,
      `live_237_cancelled_submit_does_not_wait_out_the_timeout`, guards the latency the fix would
      otherwise have cost every AJAX form.]

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

#### A. Stable-and-empty short-circuit [3/3]
- [x] Define "stable" precisely (reuse whatever `settle_page`/network-idle signal `click` already
      computes for `--wait-for-network`, rather than inventing a second notion of idle)
      firstly check `crates/ff-rdp-cli/src/commands/click.rs` for the existing signal before adding
      one
      [2026-09-07: the signal lives in `js_helpers::settle_page`, not `click.rs`. Its two inline JS
      strings were extracted to `SETTLE_INJECT_JS` (XHR/fetch counters + `MutationObserver`,
      `window.__ffrdpSettleInit`-guarded) and `SETTLE_IDLE_CHECK_JS` (nothing in flight for 500 ms
      **and** no DOM mutation for 200 ms) so `settle_page` and the short-circuit share one
      definition of idle rather than two. The short-circuit adds `document.readyState ===
      'complete'` on top, because a document still parsing grows DOM the observer has not recorded
      yet.]
- [x] Wire the short-circuit into `autowait_element`'s poll loop, gated so it only fires once the
      page is stable, never before
      [2026-09-07: the probe is installed lazily, on the first poll that *misses* — the happy path
      pays no extra eval and no page instrumentation. Two further gates: an observation floor
      (`not_found_min_observation_ms` — `--timeout`/5, minimum 500 ms) so a probe installed into a
      document an earlier command already instrumented cannot answer on its first poll, and
      `SettleProbe::Unavailable` (CSP refused the injection, or the eval failed), which falls back
      to the pre-iter-237 behaviour of polling the whole budget.]
- [x] Unit test: a selector that appears after 2 polls still resolves (retry case unregressed);
      a selector that never appears on a page that goes stable at poll N reports not-found at
      poll N+1, not at the full timeout
      [2026-09-07: four unit tests in `js_helpers.rs` driven by a scripted in-process console
      server — `unit_237_observation_floor_scales_with_the_budget`,
      `unit_237_late_selector_still_resolves_when_the_page_is_not_idle`,
      `unit_237_absent_selector_on_an_idle_page_reports_before_the_timeout`,
      `unit_237_csp_blocked_probe_falls_back_to_the_full_budget`.]

#### B. Live coverage [2/2]
- [x] Live Firefox test: guessed selector against a static page (immediately stable) resolves
      not-found in well under `--timeout`
      [2026-09-07: `live_237_absent_selector_reports_well_under_the_timeout`.]
- [x] Live Firefox test: selector that appears after a deliberate delay (dynamically inserted via
      `eval`) still resolves successfully within the existing timeout budget — the regression case
      Theme B exists to prevent
      [2026-09-07: `live_237_late_selector_behind_a_request_still_clicks`. Inserted by a `fetch`
      response rather than by `eval`: the request is what keeps the page non-idle, which is the
      exact discrimination the short-circuit has to make. A bare `setTimeout` insertion leaves no
      signal to observe and is covered by the observation floor instead.]

#### C. Measure [1/1]
- [x] Re-run this plan's `dogfood_path` reproduction before and after; record both wall-clock
      numbers here

Three runs each of `time ff-rdp click --selector '#definitely-not-on-the-page'` against
`https://example.com` (default `--timeout`, headless Firefox, same machine, same session),
`main` @ `f8d278e` built into a separate worktree versus this branch:

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| before (`main` f8d278e) | 10.98 s | 10.97 s | 10.95 s |
| after (iter-237) | 2.95 s | 2.99 s | 2.94 s |

~11.0 s → ~2.96 s, a 3.7× reduction. The residual ~2.9 s is the observation floor
(`--timeout`/5 = 2 s) plus process start, daemon round-trip and the failure diagnostic — i.e. the
budget the fix deliberately keeps, not slack left on the table.

### Acceptance Criteria [3/4]

- [x] A selector that matches nothing on a page that has gone stable reports not-found in
      measurably less than `--timeout` (record the before/after numbers, not just "faster")
      [2026-09-07: ~11.0 s → ~2.96 s against `https://example.com` at the default `--timeout`;
      three runs each side, table under Task C above.]
- [x] Every existing live test covering `autowait_element`'s retry behavior (a selector that
      appears late) still passes unmodified
      [2026-09-07: no existing live test was edited — the whole diff to
      `crates/ff-rdp-cli/tests/` is the one new `live_237_act_and_see_timing.rs` module and its
      one-line registration in `tests/live/main.rs`. See the live-sweep line in the PR body for
      the corpus-wide result.]
- [ ] `cargo run -p xtask -- live-sweep` clean with both env gates set
      [2026-09-07: **not met, and not reworded to match what happened.** The run:
      `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` →
      `LIVE_SWEEP_SUMMARY executed=325 skipped=0 preexisting=0 vanished=0 launch_timeout=0
      timed_out=0 total=325`, CLI tier `307 passed; 9 failed` (307 + 9 + the 9 across the four
      non-CLI tiers = 325, so the record reconciles). All five `live_237_*` tests are green.
      The nine reds are two pre-existing clusters, neither reachable from this diff, each with a
      row in the PR's `## Carry-over` table and an already-`planned` owner: seven `--full-page`
      screenshot failures on Firefox 155's `drawSnapshot` signature change
      ([[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]), and
      `live_137_consent_accept_via_daemon` + `live_140_frame_error_bounded`, both the daemon's
      `live_target_count: 0` under sweep load
      ([[iteration-246-sweep-load-misclassification]] Part D, which absorbed
      [[iteration-251-live-tests-red-only-under-concurrency]]). The AC asked for a *clean* sweep;
      the sweep was not clean, so the box stays empty.]
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean. (covers both parts)
      [2026-09-07: all three exit 0 on `stable`; CI's own run is the authority per CLAUDE.md's
      toolchain-skew note.]

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
