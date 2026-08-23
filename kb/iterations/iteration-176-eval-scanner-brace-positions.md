---
title: "Iteration 176: the eval scanner refuses to judge three brace positions"
type: iteration
date: 2026-08-17
status: planned
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
- [ ] If it reproduces: `brace_opens_block` recognizes `=>` before the `{`
- [ ] Unit test: a regex after an arrow body, and a division after an object literal, in one script

### C. Class bodies and labelled blocks
- [ ] If they reproduce: extend the word lookback, or record why a label stays unjudged

## Acceptance Criteria [0/3]

- [ ] Each of the three positions is either fixed with a live test, or left as-is with the reason
      recorded here — **no position is silently dropped**
- [ ] `const o = {v:8}; o.v / 2` → 4 and `const r = !function(){ return 1 }(); r` → false still
      hold on a live browser (the A/B comparison in iter-170's plan is the template)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
