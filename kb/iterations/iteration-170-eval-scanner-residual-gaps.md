---
title: "Iteration 170: eval's scanner still cannot see into ${} or past a }"
type: iteration
date: 2026-08-16
status: in-review
branch: iter-170/eval-scanner-residual-gaps
depends_on:
  - iteration-167-eval-statement-scanner-is-not-a-tokenizer
first_call_sites: []
dogfood_path: |
  # Carry-over from iter-167, which taught `top_level_statement_boundaries`
  # about regex literals, comments and backslash escapes and documented two
  # gaps it deliberately left open. NEITHER HAS BEEN RUN — like iter-167's own
  # premise, these are predictions from reading the code. Run them FIRST and
  # record the real output; close this iteration obsolete if neither
  # reproduces.
  ff-rdp launch --headless --debug-port 7502
  ff-rdp --port 7502 navigate https://example.com

  # Gap 1 — `${...}` inside a template literal is skipped as opaque text
  # rather than re-entered, so a quote or backtick inside an interpolation
  # can end the template early.
  ff-rdp --port 7502 eval --stringify 'const s = `a${"`"}b`; s'
  # → predicted: the inner backtick closes the template, the scanner sees a
  #   top-level `;` that is not one, and the wrap emits invalid JS.

  ff-rdp --port 7502 eval --stringify 'const s = `x${ ";" }y`; s'
  # → the `;` is inside an interpolation but also inside the template, which
  #   IS tracked — record whether this one is already fine.

  # Gap 2 — a `/` right after `}` is always read as division, because telling
  # an object literal from a block needs parser feedback.
  ff-rdp --port 7502 eval --stringify 'const n = 1; if (n) {} /a;b/.test("a;b")'
  # → predicted: the `/` after `}` is scanned as division, the regex is never
  #   entered, and its `;` is reported as a top-level boundary.
tags:
  - iteration
  - eval
---

# Iteration 170: `eval`'s scanner still cannot see into `${}` or past a `}`

Carry-over from [[iteration-167-eval-statement-scanner-is-not-a-tokenizer]], filed before that PR
merged.

iter-167 turned `top_level_statement_boundaries` from a quote-and-depth scanner into one that also
understands regex literals, `//` and `/* */` comments, and backslash escapes — five inputs that
emitted invalid JavaScript on `main` now evaluate correctly. It documented exactly two gaps it did
not close, both in the function's own doc comment and in `eval --help`:

1. **`${…}` interpolation** inside a template literal is skipped as opaque text rather than
   re-entered, so a quote, backtick or bracket inside an interpolation is not tracked.
2. **A `/` right after `}`** is always read as division. `{}` ends an object literal (division is
   right) and a block (a regex is right), and real tokenizers need parser feedback to tell them
   apart.

Both fail *safe* by construction: the worst outcome is a boundary the scanner should not have
reported, which costs at most a wrap — the same class of near-miss as the eleven `✓ (luck)` rows
in iter-167's measurement table, not a crash. That is precisely why they were left open, and it is
also why this plan starts by asking whether they are reachable at all from input a caller would
actually type.

## Themes

- **A — Measure, and be willing to close this obsolete.** Run the `dogfood_path` and record real
  output per input. iter-167's own premise had never been run before it was implemented, and its
  measurement widened the defect from one case to five. Run these before writing any code. If
  neither gap produces invalid JavaScript from input a caller would plausibly write, close this
  iteration obsolete and say so — the gaps are documented and fail safe, so "documented and
  unreachable" is a legitimate outcome.
- **B — Re-enter `${…}`.** If gap 1 reproduces: track interpolation as a nested scan (a depth
  counter entered at `${` and left at the matching `}`, with full string/regex/comment state
  inside) rather than skipping to the next backtick.
- **C — Decide `}` on evidence, or leave it.** If gap 2 reproduces, the only honest fixes are a
  heuristic (does the `{` that opened this `}` sit where a *statement* could start?) or nothing.
  Adding a heuristic that is wrong in the other direction — reading a division as a regex, which
  swallows text — would be worse than the current behaviour, because that failure is not safe.
  Record the decision either way.

## Theme A measurement (2026-08-17, live Firefox on port 7502, `main` @ `07a9c03`)

Every line of `dogfood_path` was run, plus five variants added to separate "reproduces" from
"reproduces only under `--stringify`" and to pin the behaviour that must NOT regress.

```text
$ ff-rdp --port 7502 eval --stringify 'const s = `a${"`"}b`; s'
  expected  a`b
  actual    {"results":{"type":"undefined"}}          <- GAP 1 REPRODUCES

$ ff-rdp --port 7502 eval 'const s = `a${"`"}b`; s'    # no --stringify
  expected  a`b
  actual    {"results":{"type":"undefined"}}          <- GAP 1 REPRODUCES

$ ff-rdp --port 7502 eval --stringify '`a${"`"}b`'     # bare expression
  expected  a`b
  actual    {"results":"a`b"}                          <- ok (luck: no declaration, no wrap)

$ ff-rdp --port 7502 eval --stringify 'const s = `x${ ";" }y`; s'
  expected  x;y
  actual    {"results":"x;y"}                          <- ok already

$ ff-rdp --port 7502 eval --stringify 'const t = `v=${JSON.stringify({a:";"})}`; t'
  expected  v={"a":";"}
  actual    {"results":"v={\"a\":\";\"}"}              <- ok already

$ ff-rdp --port 7502 eval --stringify 'const n = 1; if (n) {} /a;b/.test("a;b")'
  expected  true
  actual    {"error":"unterminated regular expression literal","error_type":"User"}
                                                       <- GAP 2 REPRODUCES

$ ff-rdp --port 7502 eval --stringify $'const n = 1; if (n) {}\n/a;b/.test("a;b")'
  expected  true
  actual    {"error":"unterminated regular expression literal","error_type":"User"}
                                                       <- GAP 2 REPRODUCES

$ ff-rdp --port 7502 eval --stringify 'const o = {v:8}; o.v / 2'
  expected  4
  actual    {"results":4}                              <- ok; must not regress
```

**Gap 1 reproduces.** The inner backtick closes the template, the following `"` opens double-quote
state, and the rest of the script — including the real top-level `;` — is swallowed as string
content. No boundary is reported, so `wrap_statements_in_iife` finds nothing to auto-return and
emits `(function(){ … })()` with no `return`. The script evaluates, but its value is lost.

**Gap 2 reproduces.** The `/` after `}` is scanned as division, so the regex is never entered and
its `;` is reported as a top-level boundary. The wrap splits the script into `… if (n) {} /a;` and
`b/.test("a;b")`, and the emitted JS is a SyntaxError. A newline before the regex does not help:
`/` is an [`is_continuation_start_char`], so the newline is not an ASI boundary either.

**Correction to this plan's premise.** The prose above says "Both fail *safe* by construction: the
worst outcome is a boundary the scanner should not have reported, which costs at most a wrap."
That is wrong, and the measurement is the evidence. Gap 1 costs the *value* — a silent
`{"type":"undefined"}`, which iter-142 Theme E named the worst failure mode of this whole wrap
("an agent gets `{"type":"undefined"}` with no indication anything went wrong"). Gap 2 costs a
user-visible SyntaxError, the exact symptom iter-167 set out to eliminate. Neither is fail-safe,
so closing this iteration obsolete was not available.

## Tasks

### A. Measure
- [x] Run every line of `dogfood_path` against a live Firefox and paste the actual output here
- [x] State explicitly, per gap, whether it reproduces

### B. Template interpolation
- [x] Re-enter `${…}` in `top_level_statement_boundaries` with full nested state
- [x] Unit tests: a backtick, a quote and a `;` inside an interpolation

### C. The `}` ambiguity
- [x] Decide: heuristic or leave as documented. Record the decision and its reasoning

## Theme C decision — heuristic, and one consequence the plan did not anticipate

**Decided: heuristic** (DEC-042). Each `{` is classified when it is opened, by
`brace_opens_block`, into `Block` / `ObjectLiteral` / `Interpolation` / `Unknown`, and the kind of
the most recently closed brace is handed to `slash_starts_regex`. The classifier commits *only*
where a statement can start and an object literal cannot — nothing before the `{`, or `;`, `{`,
`)`, or one of `do`/`else`/`try`/`finally` — and answers `ObjectLiteral` everywhere else, which
reproduces iter-167's unconditional "`}` divides" exactly. An arrow function's `{` body, a `class`
body and a labelled block are all deliberately left in that conservative bucket. So the heuristic
Theme C warned against — reading a division as a regex, which swallows text — is only reachable
from four positions in which JS admits no object literal at all.

**The consequence the plan did not anticipate.** Fixing the regex decision alone turned gap 2's
SyntaxError into a *silent* `{"type":"undefined"}`, because `if (n) {} /a;b/.test("a;b")` was still
scanned as a single statement beginning with `if`, and an `if` is not something the wrap can
auto-return. Trading a loud wrong answer for a silent one is not an improvement, so a third change
went in: a top-level block's `}` **ends its own statement** (`block_boundary_after`), suppressed
after `(`, `[`, a backtick, any continuation character except `/`, a comment, and the
`else`/`catch`/`finally`/`while` clause keywords. This is only decidable *because* the braces are
now classified, which is why it belongs here and not in an earlier iteration.

## Verification — 36 scripts through both binaries, same live Firefox

`main` @ `07a9c03` and this branch, `eval --stringify`, identical browser (port 7502). Six rows
changed, every one from a wrong answer to the right one; thirty are byte-identical.

```text
for (const a of [1,2]) {} 7                  undefined -> 7
function f(){ return 5 } f()                 undefined -> 5
let a = 1; function f(){ return a+1 } f()    undefined -> 2
const s = `a${"`"}b`; s                      undefined -> a`b
const n = 1; if (n) {} /a;b/.test("a;b")     SyntaxError -> true
switch (1) { case 1: break; } 11             undefined -> 11
```

Unchanged, including the two rows the classification could most plausibly have broken:
`const o = {v:8}; o.v / 2` → 4 and `!function(){ return 1 }()` → false. Also unchanged:
`if (1) { 2 }`, `const x = 1; if (x) { 2 }` and `if (1) { const z = 2 }` all still yield
`undefined`, which is the contract `eval --help` states.

## Review fix (2026-08-17): a function *expression*'s body was misclassified as a statement block

PR review found a fifth position that reaches the unsafe direction Theme C's decision explicitly
warned against and believed was closed off: `)` precedes `{` identically for a function
*declaration* (`function f(){}`, a statement) and a function *expression* (`const f =
function(){}`, a value), and `brace_opens_block` classified both as `Block` uniformly. Live-tested:
`main` (pre-iter-170) evaluates `const f = function(){} / 2` to `undefined`; this branch, before
this fix, threw `unterminated regular expression literal` on the exact same script —
`eval --stringify 'const f = function(){} / 2'` — a plain division of a function value that had
never been broken before. That is precisely the "reading a division as a regex, which swallows
text" failure this plan's Theme C called worse than doing nothing.

**Fix**: `function_keyword_is_declaration` — reuses `brace_opens_block`'s statement-start character
classes on the token before a `function` keyword itself (walking back past a leading `async` so
`async function foo(){}` at true statement position is unaffected), and forces a function
expression's body to `ObjectLiteral` — the pre-170, safe answer — regardless of what
`brace_opens_block` would otherwise say from the `)` immediately before its `{`. Declarations are
untouched: `function f(){ return 5 } f()` and `async function f(){ return 5 } f()` still get the
self-terminating boundary and the regex-permitting `/`.

**New coverage**: `unit_170_function_expression_body_is_not_a_statement_block` (unit; covers plain,
named, `async`, and a callback-argument body) and three new cases in
`live_170_brace_kind_decides_regex_and_boundary` (`const fe1/fe2/fe3 = …function… / 2`), all
verified against the live Firefox on port 6000 before and after the fix.

**Not chased further**: the fix is scoped to the `function` keyword (plain, named, `async`,
generator via the word-match alone since `*` doesn't affect the depth tracking). Arrow-function
expression bodies (`() => {}`) are unaffected by this fix because they were never classified
`Block` in the first place (DEC-042's own Theme C already left them in the conservative
`ObjectLiteral` bucket — see iteration 171's brace-positions plan). A `function` keyword used only
as a bare word is detected by a forward word-boundary scan with one deliberate gap: if "function"
appears in expression position but is *not* actually followed by a matching `(...) {` (e.g. it was
mid-scanned incorrectly for some construct this fix's author did not anticipate), the pushed marker
is left on a depth-indexed stack and could, in principle, misclassify a later, unrelated `{}` at the
same depth. That failure direction is `ObjectLiteral` (the safe fallback), not `Block`, so it costs
at most a missed opportunity, not a new crash — consistent with this iteration's own safety
contract. No live or unit input has been found that reaches this residual case.

## Acceptance Criteria [3/3]

- [x] unit_170_interpolation_is_scanned: a `` ` `` or `;` inside `${…}` does not end the template
      literal or produce a boundary — **only if Theme A shows gap 1 reproduces** (it does; see the
      Theme A table). Live half: `live_170_interpolation_is_scanned_as_code`.
- [x] The `}`-ambiguity decision is recorded (here if it stays as-is, in `kb/decision-log.md` if
      the behaviour changes) — behaviour changed, so DEC-042, plus the summary above.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Live sweep (2026-08-17)

Gates: `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, plus a hand-started Firefox on port 6000
(profile `/tmp/ff-rdp-sweep-profile-6000`). Orphan check before the run: `pgrep -f 'ff-rdp/profiles'`
returned nothing.

```text
LIVE_SWEEP_SUMMARY executed=274 skipped=0 preexisting=0 total=274
  -> 271 passed / 3 failed;  ff-rdp-core tier 9/9
```

Baseline of record for this batch was `executed=270 ... 269 passed / 1 failed` on `main` @ `4d639e2`.
The corpus grew, it did not shrink: +2 from this branch's `live_170_*` and +2 from iter-169's
`live_169_*`, which merged into `main` between the baseline and this run. The baseline's single
failure — `live_166_navigate_reports_document_status` — **passed** here; it was iteration 169's
subject and iter-169 merged as #207.

All eval suites are green, including every regression suite this change could plausibly have
broken: `live_161_*` 8/8, `live_165_*` 3/3, `live_167_*` 3/3, and both new `live_170_*` tests.

## Carry-over

| # | item | disposition |
| --- | --- | --- |
| 1 | `live_128_network_output_fidelity::live_128_meta_route` — FAILED in the sweep: route `direct`, `parsing registry at ~/.ff-rdp/daemon.49263.json: EOF while parsing a value at line 1 column 0`. Passes in isolation. | **fold** — iteration 172's exact located cause (the registry writer locks the published path). Added to `iteration-172-daemon-registry-torn-read-on-autostart.md` as an observed instance, with the differing `daemon_fallback` wording flagged for its Theme A. |
| 2 | `live_134_meta_route_all_commands::live_134_meta_route_all_commands` — FAILED, same zero-byte-registry signature on `click`. Passes in isolation. | **fold** — same, into iteration 172. |
| 3 | `live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless` — FAILED: `never opened debug port 64638 within 30s`. Passes in isolation. A per-test 30 s launch budget spent during a 38-minute serial sweep. **Environmental is a diagnosis, not a disposition**: the sweep reporting an unstartable browser as a failing assertion is a defect of ours. | **fold** — into iteration 173, whose Theme B already owns "the sweep must not report an unmet precondition as a failure", with a note that this is a second, distinct precondition. |
| 4 | `live_166_navigate_reports_document_status` — the batch baseline's only failure. **Passed this run.** One green run is not proof, so it gets a row. | **closed elsewhere** — iteration 169's subject; merged into `main` as #207 (`c886507`) before this sweep ran. Not this PR's to close. |
| 5 | `brace_opens_block` does not commit on an arrow function's `{` body, a `class` body or a labelled block, so each is read as an object literal — a `/` after one divides and no boundary follows it. Unmeasured: no case forced the question here. | **file** — `kb/iterations/iteration-171-eval-scanner-brace-positions.md`, `check-iteration-plan: OK`. Its Theme A must measure before fixing, like this one did. |
| 6 | Gap-2's dogfood line returns `true` only because a top-level block's `}` now ends its own statement — a change beyond the plan's Theme C wording. | **closed in this PR** — `block_boundary_after`, `unit_170_block_close_ends_a_statement`, `live_170_brace_kind_decides_regex_and_boundary`, DEC-042. Recorded in the Theme C section above rather than reworded into the AC. |
| 7 | PR review found a fifth misclassified position — a function *expression*'s body (`const f = function(){}`) was `Block` identically to a declaration's, since both are `)`-preceded. Live-tested regression: `eval --stringify 'const f = function(){} / 2'` threw `unterminated regular expression literal` on a script that evaluates fine on `main`. | **closed in this PR** — `function_keyword_is_declaration`, `unit_170_function_expression_body_is_not_a_statement_block`, three new `live_170_brace_kind_decides_regex_and_boundary` cases, DEC-042 addendum. See "Review fix" section above. |
| 8 | The review fix's `function`-keyword detection has one acknowledged, unreached residual: a stale depth-indexed marker could in principle misclassify a later, unrelated `{}` at the same depth if `function` appears in expression position but is never followed by a matching `(...) {`. No live or unit input reaches it; the failure direction is the safe one (`ObjectLiteral`). | **fold** — into `iteration-171-eval-scanner-brace-positions.md`'s "Carried in from iter-170's PR review" section, `check-iteration-plan: OK`. Worth a Theme A line there if that iteration's own measurement turns up a matching case. |

Nothing external interfered with this sweep: no `ff-rdp/profiles` orphans before or after, and the
port-6000 browser was verified alive immediately before the run and executed all 9 core-tier tests.

## Design notes

The scanner must not become a JS parser — all code stays in Rust and this repo has no JS parser
dependency (`kb/decision-log.md`, DEC-039 and iter-167's design notes). Any case the scanner
cannot decide must fail safe: an unwrapped script evaluates as it always did, which is a working
behaviour, while a wrongly-consumed one is a SyntaxError.

**Lesson for the next iteration built on this scanner.** iter-167 asserted both of these gaps fail
safe, and both assertions were wrong. The pattern is that "fails safe" was reasoned about at the
level of the *boundary list* ("a spurious boundary only costs a wrap") without following it through
to what the wrap then emits — where a spurious boundary can land inside a literal (SyntaxError) and
a *missing* one can leave the wrap with nothing to auto-return (silent `undefined`). Any future
claim of this shape needs the two-binary comparison above, not an argument.

## Out of scope

- Replacing the wrap machinery with a real parser or a WASM JS tokenizer. This was out of scope in
  iter-167 for a policy reason that has not changed, and it remains out of scope here: it is the
  answer to "make the scanner perfect", and the repo has decided against paying that price.
- Re-litigating DEC-039. iter-167 Theme C re-examined the *trigger* asymmetry on live evidence and
  left it in place; the contract itself was out of scope there and stays out of scope here.

## References

- [[iteration-167-eval-statement-scanner-is-not-a-tokenizer]] — the fix that closed the other
  three gaps and documented these two
- [[iteration-171-eval-scanner-brace-positions]] — carry-over: the brace positions this iteration
  deliberately did not commit on
- [[decision-log]] — DEC-039, and DEC-042 which this iteration adds
