---
title: "Iteration 258: navigated_away's poll budget is advisory — one blocked read spends the whole --timeout"
type: iteration
date: 2026-09-07
status: planned
branch: iter-258/navigated-away-poll-budget-not-enforced
depends_on: [237]
first_call_sites: []
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Main_Page
  ff-rdp dom "input[name=search]" --jq '.results[0].ref'
  time ff-rdp type --ref <REF> --text "Turing Award" --submit \
    --jq '{navigated: .results.navigated, method: .results.method}'
  # TODAY (post-iter-237): {"navigated":true,"method":"request_submit"} in ~12.4s wall clock,
  #   against a REQUEST_SUBMIT_NAVIGATION_GRACE_MS of 3s. The answer is correct; the time it
  #   took to reach it is not bounded by any budget the code states.
  # AFTER: the same correct answer, with the poll actually honouring its own deadline.
  ff-rdp daemon stop
tags: [iteration, cli, bugfix, carry-over, latency, transport]
---

# Iteration 258: `navigated_away`'s poll budget is advisory — one blocked read spends the whole `--timeout`

> Filed by [[iteration-237-submit-navigation-grace-period]]'s carry-over sweep, from a measurement
> taken while fixing Part A. Not a regression from 237 — 237 made the *answer* right and left the
> *latency* exactly as it found it.

## Why

`navigated_away` (`crates/ff-rdp-cli/src/commands/type_text.rs`) is written as a poll with a
deadline: a 50 ms sleep between iterations, `if started.elapsed() >= deadline { return false }` at
the bottom. Every budget passed to it — `ENTER_NAVIGATION_GRACE_MS` (600 ms),
`REQUEST_SUBMIT_NAVIGATION_GRACE_MS` (3 s) — reads as a bound on how long that call can take.

None of them is. The deadline is only consulted *between* iterations, and the work inside one
iteration is a `WebConsoleActor::evaluate_js_async` whose read timeout is the CLI's global
`--timeout` (10 s by default; see `crate::error::socket_timeout_ms`). The one case where the
budget matters most is precisely the case where Firefox stops answering: while it commits a new
document it does not reply on the pre-submit console actor at all. So the first iteration blocks
for 10 s, the loop wakes, sees its 3 s deadline long gone, and returns — having completed zero
probes and spent 3.3× the budget it was given.

Measured on 2026-09-07 with iter-237 merged into the working tree:

```
$ time ff-rdp type --ref e1 --text "Turing Award" --submit --jq '{n:.results.navigated,m:.results.method}'
{"n":true,"m":"request_submit"}
type --submit wall clock: 12.37s
```

`navigated: true` is the right answer (that is iter-237's fix, via `navigated_after_refresh`), but
12.4 s for a search-box submit reads as a hang to anything watching the process — the same
complaint that made iter-237 Part B worth doing for `click`.

This also silently defeats the design iter-237 Part A documented at length: the whole point of
giving the post-Enter check 600 ms and the post-`requestSubmit()` check 3 s was that the two are
answering different questions and deserve different budgets. When a page goes quiet, both take
10 s regardless, and the constants are decoration.

## Root cause (confirmed, not assumed)

Two independent deadlines with no relationship between them:

- the poll's own, in `navigated_away`, checked only after a probe returns;
- the transport's, set once per process from `--timeout` and applied to every socket read.

Nothing clamps the second to what is left of the first.

## Themes

- **A — make the poll's deadline bound its own reads.** The transport already carries a read
  timeout; the poll needs the read it is about to issue to expire no later than its own deadline.
  Check whether `RdpTransport` can take a per-call read deadline (or be given one temporarily,
  the way `set_target_guard` is armed and disarmed around a section) before adding a new mechanism.
- **B — do not turn a fast negative into a wrong negative.** Shortening the read must not make a
  slow-but-successful probe report "no navigation" where it previously reported "yes". The
  existing `Ok(_) | Err(ProtocolError::Timeout) => {}` arm already treats a read timeout as "keep
  polling", so a shorter read should mean *more* iterations inside the same budget, not an earlier
  false answer.
- **C — check the other poll loops for the same shape.** `poll_js_condition`,
  `wait_for_predicates` and `autowait_element` all pair a stated budget with reads bounded only by
  `--timeout`. Establish whether they have the same defect before deciding this is one function's
  bug. `autowait_element`'s Part B short-circuit (iter-237) is a partial mitigation for one of
  them, not a fix for the shape.

## Tasks

### A. Bound the read by the poll's deadline [0/3]
- [ ] Establish what per-read deadline control `RdpTransport` already exposes; add one only if it
      genuinely has none
- [ ] Clamp `navigated_away`'s `evaluate_js_async` read to the remainder of its own deadline
- [ ] Unit test with a scripted console that never answers: `navigated_away(.., 600)` must return
      inside ~1 s, not inside `--timeout`

### B. Survey the sibling loops [0/1]
- [ ] Determine whether `poll_js_condition` / `wait_for_predicates` / `autowait_element` share the
      defect; fix them here if the mechanism from A applies unchanged, otherwise record which do
      and file separately

### C. Measure [0/1]
- [ ] Re-run the `dogfood_path` reproduction before and after; record both wall-clock numbers here

## Acceptance Criteria [0/4]

- [ ] `type --submit` against a real navigating search form still reports `navigated: true`, in
      measurably less wall-clock time than the 12.37 s recorded above (record the after number)
- [ ] A unit test proves `navigated_away` returns within its stated budget when the console never
      answers
- [ ] `cargo run -p xtask -- live-sweep` clean with both env gates set
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

## Out of scope

- Lowering the default `--timeout`. It is the right budget for a whole command; the defect is that
  a sub-second sub-step inherits it.
- Re-litigating `navigated_after_refresh` (iter-237). It is what makes the answer correct, and it
  is bounded by `--timeout` on purpose — a load genuinely in flight deserves the command's stated
  budget. Only the *grace-period* poll is claiming a bound it does not enforce.

## References

- [[iteration-237-submit-navigation-grace-period]] — Part A, where the measurement was taken
- `crates/ff-rdp-cli/src/commands/type_text.rs` — `navigated_away`, `navigated_after_refresh`
- `crates/ff-rdp-cli/src/error.rs` — `socket_timeout_ms`, the per-process read timeout
