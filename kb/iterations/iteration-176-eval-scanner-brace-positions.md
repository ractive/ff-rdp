---
title: "Iteration 176: the eval scanner refuses to judge three brace positions"
type: iteration
date: 2026-08-17
status: done
branch: iter-176/eval-scanner-brace-positions
depends_on:
  - iteration-170-eval-scanner-residual-gaps
first_call_sites: []
dogfood_path: |
  # Carry-over from iter-170, filed before that PR merged. iter-170 taught
  # `top_level_statement_boundaries` to classify each `{` it opens, but
  # `brace_opens_block` deliberately commits only where a statement can start
  # AND an object literal cannot: nothing before the `{`, or `;`, `{`, `)`, or
  # one of do/else/try/finally. Three positions are left in the conservative
  # `ObjectLiteral` bucket even though they are really blocks:
  #
  #   1. an arrow function body   `const f = () => { … }`
  #   2. a class body             `class K { … }`
  #   3. a labelled block         `outer: { … }`
  #
  # For each, a `/` after the closing `}` reads as division and the `}` does
  # not end a statement. THESE ARE PREDICTIONS FROM READING THE CODE — none has
  # been run. Run them FIRST and record the real output. iter-170's own premise
  # (that iter-167's two gaps failed safe) was disproved by exactly this step,
  # and iter-164 and iter-166 were both closed obsolete the same way.
  ff-rdp launch --headless --debug-port 7503
  ff-rdp --port 7503 navigate https://example.com
  
  # 1 — arrow body. Predicted: the `/` reads as division, the regex is never
  #     entered, its `;` becomes a top-level boundary, and the wrap splits
  #     inside the literal (the iter-170 gap-2 symptom, one form along).
  ff-rdp --port 7503 eval --stringify 'const f = () => {}; f() /a;b/.test("a;b")'
  ff-rdp --port 7503 eval --stringify 'const g = () => {} /a;b/.test("a;b")'
  
  # 2 — class body. Predicted: no boundary after `}`, so the trailing
  #     expression is not auto-returned and the value is silently undefined.
  ff-rdp --port 7503 eval --stringify 'class K { m(){ return 9 } } new K().m()'
  # Compare: with an explicit `;` after the class this already works today —
  ff-rdp --port 7503 eval --stringify 'class K { m(){ return 9 } } ; new K().m()'
  
  # 3 — labelled block.
  ff-rdp --port 7503 eval --stringify 'const n = 1; outer: { break outer } n'
  
  # Must NOT regress — the divisions the conservative bucket protects:
  ff-rdp --port 7503 eval --stringify 'const o = {v:8}; o.v / 2'
  ff-rdp --port 7503 eval --stringify 'const r = !function(){ return 1 }(); r'
tags:
  - iteration
  - eval
---

# Iteration 176: the `eval` scanner refuses to judge three brace positions

Carry-over from [[iteration-170-eval-scanner-residual-gaps]], filed before that PR merged.

iter-170 gave `top_level_statement_boundaries` a `BraceKind` stack (`Block` / `ObjectLiteral` /
`Interpolation` / `Unknown`) and used it for three answers the scanner previously guessed at: it
re-enters `${…}` as real code, it decides `/`-after-`}` from what the matching `{` opened, and it
treats a top-level block's `}` as the end of its own statement. See DEC-042.

`brace_opens_block` is deliberately narrow. It answers `Block` only for positions where a statement
can start **and** an object literal cannot — nothing before the `{`, or `;`, `{`, `)`, or one of
`do`/`else`/`try`/`finally` — and `ObjectLiteral` for everything else, which reproduces iter-167's
unconditional "`}` divides" exactly. That leaves three positions judged wrong-but-safely: an arrow
function's `{` body (preceded by `>`), a `class` body (preceded by an identifier), and a labelled
block (preceded by `:`).

Whether any of them is *reachable* from input a caller would type is unmeasured, which is the whole
point of Theme A.

## Themes

- **A — Measure, and be willing to close this obsolete.** Run the `dogfood_path` and record real
  output per input. This is the third iteration in a row on this scanner whose premise was written
  from code reading; iter-170's measurement disproved iter-167's "both gaps fail safe" claim, and
  iter-164 and iter-166 were both closed obsolete when measurement disproved theirs. If none of the
  three positions produces a wrong value or invalid JavaScript, close this iteration obsolete and
  say so.
- **B — Arrow bodies.** `=>` is unambiguous: the `{` after it is always a block. The reason
  iter-170 did not commit is that it had no case forcing the question, not that the question is
  hard. If Theme A shows it matters, `brace_opens_block` gains a two-character lookback for `=>`.
- **C — Class bodies and labelled blocks.** Both are preceded by an identifier-ish token, so both
  need the same word-lookback `brace_opens_block` already does for `do`/`else`/`try`/`finally` —
  `class`/`extends` for one, and for a label, "an identifier followed by `:` at statement
  position", which is the one genuinely ambiguous case (`{a: 1}` looks the same from the right).
  A label may be the case to leave alone; record the decision either way.

## Theme A measurement (2026-08-23, live Firefox, headless, port 7503)

Binary: `cargo run -p ff-rdp-cli --` at `iter-176/eval-scanner-brace-positions` base (= `main`
7d457af). **Ground truth** column is Firefox's own answer for the same source, obtained by handing
the raw source to an indirect `eval` inside a single wrap-proof expression, so the wrap cannot
influence it:

```
ff-rdp --port 7503 eval --stringify '(function(s){try{return "OK "+String(eval(s))}catch(e){return "ERR "+e.message}})("<source>")'
```

### The plan's predicted lines, as written

| # | source | Firefox (ground truth) | ff-rdp on `main` | verdict |
|---|--------|------------------------|------------------|---------|
| 1a | `const f = () => {}; f() /a;b/.test("a;b")` | `ERR expected expression, got '.'` | `error: expected expression, got '.'` | **not a defect** — the source is invalid JS (a `/` after `)` is division), and ff-rdp reproduces Firefox's error verbatim. The plan predicted a wrap split; it does not happen. |
| 1b | `const g = () => {} /a;b/.test("a;b")` | `ERR unexpected token: regular expression literal` | `error: unterminated regular expression literal` | **not a defect on valid input** — also invalid JS (an ArrowFunction is not a division operand and ASI needs a line terminator). Only the SyntaxError *text* differs. |
| 2a | `class K { m(){ return 9 } } new K().m()` | `OK 9` | `{"type":"undefined"}` | **REPRODUCES — silent wrong value.** No boundary after the class `}`, so `new K().m()` is not the last statement and nothing is auto-returned. |
| 2b | `class K { m(){ return 9 } } ; new K().m()` | — | `9` | works today, as the plan predicted |
| 3 | `const n = 1; outer: { break outer } n` | `OK 1` | `error: missing ) in parenthetical` | **REPRODUCES — valid JS turned into a SyntaxError.** |
| R1 | `const o = {v:8}; o.v / 2` | `OK 4` | `4` | must-not-regress baseline |
| R2 | `const r = !function(){ return 1 }(); r` | — | `false` | must-not-regress baseline |

### The line the plan did not think to write

Positions 1–3 all reach the *same* defect once a newline (i.e. real ASI) is put where the plan put
a bare space. All three of these are valid JS that Firefox evaluates to `true`:

| # | source (`\n` is a real newline) | Firefox | ff-rdp on `main` |
|---|--------|---------|------------------|
| 1c | `const g = () => {}\n/a;b/.test("a;b")` | `OK true` | `error: unterminated regular expression literal` |
| 2c | `class K { m(){ return 9 } }\n/a;b/.test("a;b")` | `OK true` | `error: unterminated regular expression literal` |
| 3c | `const n = 1; outer: { break outer }\n/a;b/.test("a;b")` | `OK true` | `error: unterminated regular expression literal` |

In each the `}` is classified `ObjectLiteral`, so the `/` reads as division, the `;` *inside* the
regex becomes a top-level boundary, and the wrap splits mid-literal — iter-170's gap-2 symptom
exactly, one position along.

### Per-position conclusion

- **Arrow body — reproduces (1c).** Not via the plan's own lines (1a/1b are invalid JS), but via the
  ASI form. Fixed under Theme B.
- **Class body — reproduces twice (2a silent `undefined`, 2c SyntaxError).** The strongest of the
  three: 2a is a wrong *value*, not an error. Fixed under Theme C.
- **Labelled block — reproduces twice (3 and 3c).** Valid JS, hard SyntaxError. Fixed under Theme C;
  see "Why the label is judgeable after all" below.

Not closed obsolete: every position produces a wrong value or turns valid JavaScript into invalid
JavaScript on a live browser.

## Tasks

### A. Measure
- [x] Run every line of `dogfood_path` against a live Firefox and paste the actual output here
- [x] State explicitly, per position, whether it reproduces and with what symptom

### B. Arrow bodies
- [x] If it reproduces: `brace_opens_block` recognizes `=>` before the `{`
- [x] Unit test: a regex after an arrow body, and a division after an object literal, in one script
      — `unit_176_arrow_body_is_a_block`, on the single script
      `const g = () => {}\n/a;b/.test("a;b"); const o = {v:8}; o.v / 2`

### C. Class bodies and labelled blocks
- [x] If they reproduce: extend the word lookback, or record why a label stays unjudged — both are
      now judged. See "Why the label is judgeable after all" below for the label decision, which the
      plan explicitly left open.

## Why the label is judgeable after all

iter-170's reason for leaving `:` alone was that "`{a: 1}` looks the same from the right", and read
from the `:` alone it does. It stops looking the same **one token further left**. A label sits where
a *statement* can start — nothing before it, a `;`, or a **block**'s `}` — and nothing that puts a
`{` after a `:` can occupy that position:

| what puts a `{` after a `:` | what precedes the `:`-identifier | judged |
|---|---|---|
| labelled block, `outer: { … }` | nothing / `;` / a block's `}` | **Block** |
| object key, `{a: {b:1}}` | `{` | ObjectLiteral |
| later object key, `{a:1, b:{c:2}}` | `,` | ObjectLiteral |
| ternary alternative, `c ? x : {a:1}` | `?` | ObjectLiteral |
| `case 1: {…}` | the word `case` (and a leading digit is rejected outright) | ObjectLiteral |

`default: {…}` inside a `switch` is accepted, and is genuinely a block. The cost is that a label
nested inside a non-block brace keeps the pre-176 answer — a missing boundary, never a spurious
one, i.e. the fail-safe direction DEC-042 already chose.

## Accepted divergence (recorded, not fixed)

`const g = () => {} /re/.test(s)` — with **no** line terminator — is rejected by Firefox (an
ArrowFunction is not a division operand, and ASI needs a newline) but is now accepted by the
scanner, which reads the arrow body's `}` as self-terminating the way a block statement's is. That
same rule is what makes the newline form (case 1c above), which *is* valid JavaScript, work. The
divergence only ever accepts input Firefox would reject; it never changes the value of a valid
script. Recorded in the DEC-042 addendum and in `top_level_statement_boundaries`' doc comment
rather than papered over.

## Local review fix (2026-08-23)

`/review-pr` (local-only) traced the new `class`/`extends` lookback in `brace_opens_block` by hand
and found the PR's own headline bug reproduced for a **namespaced superclass**:
`class K extends Foo.Bar {}` — the `class C extends React.Component {}` /
`extends stream.Writable {}` shape, not an exotic one. The dotted-property guard iter-170 added to
correctly exclude `obj.try {` fired on `Bar`'s preceding `.` before the class/`extends` lookback
this iteration added ever ran, so the class body stayed `ObjectLiteral` — reproducing the exact
silent-`undefined` defect this iteration exists to fix. Every test at the first landing (unit, live,
Theme A) only ever exercised a bare superclass identifier (`extends Object`); none covered a dotted
one.

Confirmed by hand before fixing: a temporary case added to `unit_176_class_declaration_body_is_a_block`
(`"class K extends Foo.Bar {} K.name"`) failed with `boundaries: []` (expected one boundary) on the
pre-fix code.

**Fix**: `brace_opens_block` now walks the whole `ident(.ident)*` chain back from the `{` (handling
`class K extends Foo.Bar.Baz {}` too) before checking whether `class`/`extends` precedes it, instead
of stopping at the first dot. The `obj.try {` guard itself is unchanged in effect — it still gates
the bare `do`/`else`/`try`/`finally`/`class` keyword check — but no longer short-circuits the
chain-walk. Regression-tested (`obj.try {}`, `obj.class {}`, `ns.obj.try {}` must still stay
unjudged) in the same unit test.

Two more, smaller findings from the same review, not fixed (fail-safe, documented instead):

- A labelled block as the very first statement of a real (non-object-literal) block —
  `if (1) { outer: { break outer } } /a;b/…` — is also unjudged. This is *not* the object-literal
  case "Why the label is judgeable after all" names; it is the same conservative answer for a
  different reason (the currently-open enclosing brace's kind is never handed to
  `label_precedes_block`, only the last *closed* one). Traced by hand: fails safe — the nested
  label's own misclassified `{}` never reaches a depth-0 boundary check, so it does not corrupt the
  enclosing block's own boundary. Documented on `label_precedes_block`.
- The bare `class {}` anonymous-declaration branch in `brace_opens_block` is believed unreachable on
  valid input through this eval path (a nameless class declaration needs `export default`, which a
  non-module script never has). Harmless if ever reached — `true` is the correct answer either way.
  Documented at the call site.

## Acceptance Criteria [3/3]

- [x] Each of the three positions is either fixed with a live test, or left as-is with the reason
      recorded here — **no position is silently dropped**. All three are fixed:
      `live_176_arrow_class_and_label_bodies_are_blocks` covers arrow (2 cases), class (3) and
      label (3).
- [x] `const o = {v:8}; o.v / 2` → 4 and `const r = !function(){ return 1 }(); r` → false still
      hold on a live browser — both are the first two cases of
      `live_176_object_literals_and_expressions_still_divide`, alongside 14 more `}`-then-`/`
      divisions (object keys, ternary branches, class *expressions*, function expressions, an
      arrow body followed by `,`).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Closing live sweep (2026-08-23)

Gates: `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`

```
LIVE_SWEEP_SUMMARY executed=275 skipped=0 preexisting=9 vanished=0 launch_timeout=0 total=284
ff-rdp-cli --test live: 274 passed / 1 failed, 762.99s
```

Both new tests executed and passed:

```
test live_176_eval_scanner_brace_positions::live_176_arrow_class_and_label_bodies_are_blocks ... ok
test live_176_eval_scanner_brace_positions::live_176_object_literals_and_expressions_still_divide ... ok
```

The single failure — `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running`
— is a pre-existing leaked Firefox, not sweep load and not this diff. See the carry-over table.

**An earlier attempt at this sweep was killed by the agent harness at test ~140** (not by a person,
and not mid-Firefox by choice). Its numbers are discarded, not quoted; the run above is a complete
re-run from a clean start.

## Carry-over

| # | item | disposition |
|---|------|-------------|
| 1 | Sweep failure `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` — precondition violated by pid 79010, an ff-rdp-managed profile owned by a live process attributed to `live_160_envelope_honesty::live_160_selector_diagnostics_survive`, started ~5 h before the sweep. Failed identically in isolation, so it is the orphan, not load. `kill 79010` then re-run → `live_96_profile_cleanup`: 3 passed / 0 failed. | **fold** — folded into [[iteration-190-live-sweep-only-failures]] as a third mechanism for the same test, with the two follow-ups it implies (does that live test really leak, and should a sweep name a pre-dating orphan in one line). Validated with `check-iteration-plan`. |
| 2 | First sweep attempt killed by the harness at ~test 140, leaving orphaned browsers behind. | **closed in this PR** — numbers discarded rather than quoted, sweep re-run from scratch, and the one orphan it left was identified by PID and terminated before the re-run's failure was diagnosed. |
| 3 | Accepted divergence: `const g = () => {} /re/.test(s)` with no line terminator is now accepted where Firefox rejects it. | **no plan, with a stated reason** — nothing measured is left to act on: the divergence only ever accepts input Firefox rejects, and removing it would break the valid newline form. It is recorded in the DEC-042 addendum, in `top_level_statement_boundaries`' doc comment, and in "Accepted divergence" above. If a caller ever reports a *valid* script whose value changed because of it, that needs its own plan. |
| 4 | Stale-marker residual on `expr_body_depths`: a `class` keyword in expression position never followed by a `{` at the same depth could misclassify a later `{}` — the same unreached residual `function_keyword_is_declaration` already carries. | **no plan, with a stated reason** — unreached by any live or unit input, and in the safe (pre-170) direction. Documented on `top_level_statement_boundaries`. If a measurement ever produces a matching input, it needs its own plan. |
| 5 | Labels nested inside a non-block brace (`{ outer: {…} }`) stay unjudged. | **no plan, with a stated reason** — deliberate, in the fail-safe direction (a missing boundary, never a spurious one), and stated in "Why the label is judgeable after all". A measured case that produces a wrong value would change that. |
| 6 | Plan's dogfood lines 1a/1b predicted an arrow-body defect on input that is invalid JavaScript in Firefox itself. | **closed in this PR** — recorded in the Theme A table as *not* defects, with Firefox's own error for each, rather than being quietly reused as evidence. The arrow position was confirmed reachable by a different line (1c) that the plan did not write. |
| 7 | Local review (2026-08-23): a labelled block as the first statement of a real enclosing block (`if (1) { outer: {…} } ...`) also stays unjudged — a different mechanism than item 5. | **no plan, with a stated reason** — fail-safe by hand-trace (does not corrupt the enclosing block's own boundary; see "Local review fix"). Documented on `label_precedes_block`. |
| 8 | Local review (2026-08-23): the bare `class {}` anonymous-declaration branch in `brace_opens_block` is believed unreachable through this eval path. | **no plan, with a stated reason** — harmless if ever reached (`true` is the correct answer). Documented at the call site. |

## Design notes

The scanner must not become a JS parser (DEC-039, iter-167 and iter-170 design notes). The rule
iter-170 arrived at is the one to keep: commit only where JS admits no object literal at all, and
leave everything else in the bucket that reproduces the previous behaviour.

iter-170's lesson applies directly here — "fails safe" reasoned about at the level of the boundary
list is not evidence. A spurious boundary can land inside a literal (SyntaxError) and a missing one
can leave the wrap with nothing to auto-return (silent `undefined`). Any claim of that shape needs
the two-binary A/B comparison, not an argument.

## Carried in from iter-170's PR review (2026-08-17)

Review of iter-170's PR (#208) found and fixed a fourth misclassified position before merge: a
`function` *expression*'s body (`const f = function(){}`) was classified `Block` identically to a
`function` *declaration*'s, because both are `)`-preceded — `brace_opens_block` cannot tell them
apart from the `{` alone. Live-tested regression: `eval --stringify 'const f = function(){} / 2'`
threw `unterminated regular expression literal` on a script that evaluates fine on `main`. Fixed in
the same PR by `function_keyword_is_declaration`, which checks the token before the `function`
keyword itself (walking back past a leading `async`), not just before its `{`.

That fix has one acknowledged, unreached residual: it detects the `function` keyword by a forward
word-boundary scan and pushes the current depth onto a stack, popped at the next `{` reached at that
depth. If `function` appears in expression position but is never actually followed by a matching
`(...) {` — a case no live or unit input has produced — the stale marker could misclassify a later,
unrelated `{}` at the same depth as `ObjectLiteral` instead of `Block`. That is the *safe* direction
(the pre-170 default), so it costs a missed opportunity, not a new crash, and is not itself a reason
to reopen iter-170. Worth a Theme A line here if this iteration's own measurement turns up a
matching case; otherwise it stays a documented, unreached theoretical gap.

## Out of scope

- Replacing the wrap machinery with a real parser or a WASM JS tokenizer — rejected in iter-167 and
  again in iter-170 for a policy reason that has not changed.
- Re-litigating DEC-039 or DEC-042.

## References

- [[iteration-170-eval-scanner-residual-gaps]] — the brace classification this builds on, and its
  36-script A/B comparison
- [[iteration-167-eval-statement-scanner-is-not-a-tokenizer]] — the scanner's regex/comment/escape
  handling
- [[decision-log]] — DEC-042
