---
title: "Iteration 215: type --submit under-reports navigated after a slow requestSubmit()"
type: iteration
date: 2026-08-29
status: planned
branch: iter-215/submit-navigation-grace-period
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
tags: [iteration, cli, agent-ergonomics, refs, bugfix]
---

# Iteration 215: `type --submit` under-reports `navigated` after a slow `requestSubmit()`

Found while manually verifying [[iteration-210-act-and-see]]'s PR #230 review fixes live against
Wikipedia. `type --ref <search box> --text "Turing Award" --submit --with-page` correctly submits
the form (synthetic Enter does nothing on Wikipedia's search box, so the `requestSubmit()`
fallback fires) and correctly returns the *destination* page under `results.page` — the iter-210
fix for stale-actor reuse after a navigating submit is working. But `results.navigated` reports
`false`, even though `results.page.headings[0]` visibly changed from "Main Page" to "Turing
Award" in the same response — the two fields of one envelope disagree with each other.

## Root cause

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

## Themes

- **A — Give the post-`requestSubmit()` poll room to observe a real navigation.** Either a longer,
  separate grace period for this call site than the post-Enter one, or reuse the command's own
  `--timeout`/auto-wait budget instead of a small fixed constant.

## Tasks

### A. Fix the second `navigated_away` call [0/2]
- [ ] Give `press_enter_and_submit`'s post-`requestSubmit()` `navigated_away` call its own
      constant (e.g. `REQUEST_SUBMIT_NAVIGATION_GRACE_MS`) sized for a real network round-trip —
      or thread `wait_timeout_ms` through so it honours `--timeout` like the rest of the command —
      rather than reusing `ENTER_NAVIGATION_GRACE_MS`
- [ ] Document, at the call site, why the two `navigated_away` calls in `press_enter_and_submit`
      need different budgets (post-Enter: "did the untrusted keydown do anything, fast local
      check" vs. post-`requestSubmit`: "did the network round-trip land")

## Acceptance Criteria [0/2]

- [ ] `live_type_submit_navigates_search_form` (or a new live test) asserts `results.navigated ==
      true` when `results.page`'s heading demonstrably changed — the two fields must agree
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Don't just raise `ENTER_NAVIGATION_GRACE_MS` globally.** The post-Enter call benefits from
  staying short — it is the local-only check that decides whether to bother calling
  `requestSubmit()` at all, and a slow local check just adds latency to the (usual, per the
  isTrusted ceiling) no-op case. Only the post-submit call needs the longer budget.
- **`results.navigated` disagreeing with `results.page` is the actual bug**, not "false" being
  wrong in isolation — a caller reading only `results.navigated` (no `--with-page`) has no way to
  catch this today.

## Out of scope

- Anything about `--with-page`'s own correctness after a navigating submit — already fixed and
  live-verified in [[iteration-210-act-and-see]]'s review-fix pass.

## References

- [[iteration-210-act-and-see]] — carry-over row that filed this plan; the review-fix pass that
  found it live
- `crates/ff-rdp-cli/src/commands/type_text.rs` — `press_enter_and_submit`, `navigated_away`,
  `ENTER_NAVIGATION_GRACE_MS`
