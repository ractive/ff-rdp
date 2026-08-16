---
title: "Iteration 167: eval's statement scanner mis-splits regex literals and comments"
type: iteration
date: 2026-08-16
status: in-review
branch: iter-167/eval-statement-scanner
depends_on: []
first_call_sites: []
dogfood_path: |
  # `--stringify` accepts "exactly what bare eval accepts" (iter-161). Measure
  # whether that survives a regex literal containing a semicolon.
  ff-rdp launch --headless --debug-port 7501
  ff-rdp --port 7501 navigate https://example.com
  
  ff-rdp --port 7501 eval '/a;b/.test("a;b")'
  # → true. The plain path sends it verbatim (iter-165 deliberately keeps
  #   declaration-free scripts off the wrap), so this one works.
  
  ff-rdp --port 7501 eval --stringify '/a;b/.test("a;b")'
  # → on main (2026-08-16, measured at iter-165 by reading the scanner, NOT
  #   yet run): expected to fail. `top_level_statement_boundaries` sees the
  #   `;` inside the regex as a top-level statement separator, so the script
  #   is split into `/a;` and `b/.test("a;b")` and wrapped into invalid JS.
  #   RUN THIS FIRST and record the exact error before touching any code —
  #   the whole premise of this plan is that prediction.
  
  ff-rdp --port 7501 eval --stringify 'const r = /a;b/; r.test("a;b")'
  # → same question with a declaration, which also puts the PLAIN path on the
  #   wrap since iter-165. Record both.
  
  ff-rdp --port 7501 eval --stringify 'const t = 1 // note
  t'
  # → line comments are not tracked either; record whether the split lands
  #   inside the comment.
tags:
  - iteration
  - eval
---

# Iteration 167: `eval`'s statement scanner mis-splits regex literals and comments

Carry-over from [[iteration-165-eval-scope-leak-contradicts-help]], filed before that PR merged.

`top_level_statement_boundaries` in `crates/ff-rdp-cli/src/commands/eval.rs` decides where one
top-level JS statement ends and the next begins. Everything built on it — the `--stringify` wrap
(iter-161), the top-level-`await` wrap (iter-132/142) and now the iter-165 per-call scope wrap —
inherits its accuracy. It is a character scanner, not a tokenizer, and its own doc comment already
admits the gap: it tracks `'`/`"`/`` ` `` string state and bracket depth, but it does **not**
understand regex literals, `//` line comments, `/* */` block comments, or backslash escapes inside
strings.

That is not hypothetical. `/a;b/.test("a;b")` contains a top-level `;` inside a regex literal; the
scanner reports a boundary there, so a wrap splits the script into `/a;` and `b/.test("a;b")` and
emits invalid JavaScript. iter-165 measured this by inspection and deliberately narrowed its own
wrap trigger to scripts containing a top-level declaration so the plain path could not be exposed
to it — but `--stringify` and `await` scripts still are, and a declaring script on the plain path
now is too.

## Themes

- **A — Measure the real blast radius.** Run the `dogfood_path` above against a live Firefox and
  record, per input, which of the three wrap paths breaks and with what error. The plan's premise
  is a prediction from reading the code; if it does not reproduce, say so and close the iteration
  obsolete rather than fixing an imagined defect.
- **B — Teach the scanner what a `/` means.** Regex-vs-division is decidable from the previous
  significant character, which the scanner already tracks as `prev_significant`: a `/` starts a
  regex when the previous significant char cannot end an expression (`is_statement_end_char` is
  false), and is division otherwise. `//` and `/*` are recognised in the same place. Backslash
  escapes need handling inside regex and string state alike.
- **C — Decide whether the two wrap triggers converge.** iter-165's plain path wraps only when
  `declares_at_top_level`; `--stringify` wraps whenever the script is not a single expression. The
  asymmetry was accepted in DEC-039 *because* the scanner is unreliable. If Theme B makes it
  reliable, re-examine whether the plain path should wrap uniformly — which would also make
  `eval 'return 1'` work instead of `SyntaxError: illegal return statement`. Decide on evidence and
  record it; converging is not automatically right, because the uniform trigger costs
  `eval 'if (1) { 2 }'` its script completion value.

## Theme A measurement (run 2026-08-16, live Firefox on port 7501, before any code change)

`ff-rdp launch --headless --debug-port 7501` + `navigate https://example.com`, then each input below.
`✗` = the wrap emitted invalid JavaScript; `✓ (luck)` = the scanner still reported a wrong boundary
set but the split happened to land somewhere that reassembled into valid JS.

| # | command | main's output | verdict |
|---|---|---|---|
| 1 | `eval '/a;b/.test("a;b")'` | `{"results": true}` | ✓ — sent verbatim, no wrap (iter-165 keeps declaration-free scripts off it) |
| 2 | `eval --stringify '/a;b/.test("a;b")'` | `{"error":"unterminated regular expression literal","error_type":"User"}` | **✗ reproduces exactly as predicted** |
| 3 | `eval --stringify 'const r = /a;b/; r.test("a;b")'` | `{"results": true}` | ✓ (luck) — two bogus boundaries, but `wrap_statements_in_iife` uses only the *last*, which lands after the real `;` |
| 4 | `eval --stringify 'const t = 1 // note\nt'` | `{"results": 1}` | ✓ (luck) — the comment is not tracked, but the ASI newline boundary lands on `t` anyway |
| 5 | `eval 'const r = /a;b/; r.test("a;b")'` (iter-165 plain wrap) | `{"results": true}` | ✓ (luck), same reason as #3 |
| 6 | `eval 'await Promise.resolve(/a;b/.test("a;b"))'` | `{"results": true}` | ✓ — the regex sits at depth ≥ 1, where `;` is never a boundary |
| 7 | `eval --stringify 'const x = 1 /* a; b */; x'` | `{"results": 1}` | ✓ (luck) |
| 8 | `eval 'const x = 1 /* a; b */; x'` | `{"results": 1}` | ✓ (luck) |
| 9 | `eval --stringify 'const t = 1; // a; b\nt'` | `{"results": 1}` | ✓ (luck) |
| 10 | `eval --stringify 'const s = "a\";b"; s'` | `{"error":"\"\" string literal contains an unescaped line break","error_type":"User"}` | **✗ backslash escapes inside strings** |
| 11 | `eval --stringify 'const a = 8; a / 2'` | `{"results": 4}` | ✓ — division is not currently mistaken for a regex (there is no regex state to mistake it for) |
| 12 | `eval --stringify 'const s = "x"; /a;b/.test("a;b")'` | `{"error":"unterminated regular expression literal","error_type":"User"}` | **✗ regex as the last statement** |
| 13 | `eval --stringify "// don't touch\nconst x = 1; x"` | `{"error":"expected expression, got keyword 'const'","error_type":"User"}` | **✗ an apostrophe in a `//` comment opens string state and swallows the rest** |
| 14 | `eval --stringify 'const re = /a\/b;c/; re.test("a/b;c")'` | `{"results": true}` | ✓ (luck) |
| 15 | `eval "// don't touch\nconst x = 1; x"` | `{"results": 1}` | ✓ (luck) — `declares_at_top_level` misses the swallowed `const`, so the script stays on the unwrapped path |
| 16 | `eval --stringify 'const s = ` + "`a\\`;b`" + `; s'` | `{"error":"expected expression, got ')'","error_type":"User"}` | **✗ backslash escapes inside template literals** |

Premise confirmed: 5 of 16 inputs produce invalid JavaScript on main, and the plan's headline case
(#2) reproduces with the exact predicted failure. The iteration is **not** closed obsolete. The
measurement also widened the blast radius past what the plan predicted — an apostrophe in a `//`
comment (#13) and a backslash escape in a string or template (#10, #16) are the same class of
defect and are fixed here too.

Note on the `✓ (luck)` rows: they are not evidence the scanner works. Each one reports a wrong
boundary set; they survive only because the two consumers that matter
(`wrap_statements_in_iife`, which reads `.last()`, and `declares_at_top_level`, which reads
`.any()`) happen to be insensitive to the extra boundaries in those particular inputs. Every one
of them is a regression waiting for a slightly different input.

## Tasks

### A. Measure
- [x] Run every line of `dogfood_path` and paste the actual outputs into this plan
- [x] Add each reproducing input to the live matrix in `live_161_build_script_matrix_evaluates`

### B. Fix the scanner
- [x] Track regex-literal state in `top_level_statement_boundaries`, keyed off `prev_significant`
- [x] Skip `//` line comments and `/* */` block comments
- [x] Handle backslash escapes inside strings, templates and regex literals
- [x] Unit tests for each: division vs regex, a regex containing `;`, a comment containing `;`,
      an escaped quote inside a string

### C. Trigger convergence
- [x] Re-examine `declares_at_top_level` vs `!looks_like_single_expression` and record the decision
      (a DEC entry if the triggers converge, a note in this plan if they stay apart)

## Theme B — what the fix actually changed

`crates/ff-rdp-cli/src/commands/eval.rs`:

- `slash_starts_regex` — regex-vs-division from `prev_significant`, plus a `KEYWORDS_BEFORE_REGEX`
  walk-back so `return /a;b/` is a regex while `count / 2` is a division. A `/` after `}` stays
  division (object-literal vs block needs parser feedback; failing safe means an extra boundary,
  which costs at most a wrap).
- `scan_regex_literal` — finds the closing `/`, honouring `\` escapes and `[...]` classes, and
  gives up at a newline so a mis-detected regex cannot swallow the rest of the script.
- `//` comments are skipped up to but not including their newline, so that newline is still an ASI
  boundary candidate; `/* */` comments are skipped whole, with an `asi_boundary_after` check when
  the comment spans a line terminator (otherwise the fix would have *lost* a boundary the old
  scanner found).
- Backslash escapes inside `'`, `"` and `` ` `` literals.
- `prev_significant` is now tracked at every bracket depth, not only depth 0 — `foo(/a'b/)` used to
  open string state on the apostrophe.
- `trim_leading_trivia` — a statement may *begin* with a comment. Without it
  `declares_at_top_level("// note\nconst x = 1; x")` returned false and the iter-165 isolation
  silently did not apply.

Unwanted side effect found while fixing (not in the plan, kept): a comment-only script and
`// c\nconst q = 1` used to reach `--stringify`'s argument slot raw and fail to parse; both now
evaluate to `undefined`.

**Review fix (post-implementation, PR #205 local review):** `slash_starts_regex` matched
`KEYWORDS_BEFORE_REGEX` against the previous identifier regardless of what preceded it, so a
dotted property access named like a reserved word (`obj.in`, `obj.new`, `obj.case`, … — legal
since ES5) was misread as the keyword. `obj.in / 2; foo() / 3` demonstrated the failure: the fake
regex scan swallowed the real top-level `;` between the two `/` — the one direction of this
heuristic that does not fail safe. Fixed by excluding the match when the identifier is immediately
preceded by `.`; see `unit_167_dotted_property_named_like_keyword_is_not_the_keyword`.

## Theme C — decision: the two triggers stay apart

Recorded as a revisit note under DEC-039 rather than a new DEC entry, because the decision is
unchanged; only its *rationale* shrank.

iter-165 gave two reasons for the plain path's narrow `declares_at_top_level` trigger: (1) a
declaration-free script cannot leak, so wrapping it buys no isolation and costs its script
completion value; (2) it confined the blast radius of an unreliable scanner. Theme B removed
reason (2). Reason (1) never depended on it and is sufficient. Measured live at iter-167:

| script | today | under a converged (`--stringify`-style) trigger |
|---|---|---|
| `eval 'if (1) { 2 }'` | `2` | `undefined` |
| `eval 'for (let i = 0; i < 3; i++) { i }'` | `2` | `undefined` |
| `eval 'window.__c3 = 1; 40 + 2'` | `42` | `42` (last statement is a bare expression) |
| `eval 'return 1'` | `return not in function` | `1` |

Converging trades two working behaviours for one. It stays split.

## Acceptance Criteria [4/4]

- [x] live_167_regex_literal_survives_every_wrap: `eval`, `eval --stringify` and an `await`
      variant of `/a;b/.test("a;b")` all return `true` against a live Firefox
- [x] unit_167_scanner_ignores_regex_and_comments: `top_level_statement_boundaries` reports no
      boundary for a `;` inside a regex literal, a `//` comment or a `/* */` comment, and still
      reports one for a real top-level `;`
- [x] unit_167_division_is_not_a_regex: `a / b; c / d` still splits at the real `;` and is not
      swallowed as a regex literal
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The scanner is deliberately not a JS parser and must not become one — all code stays in Rust and
this repo has no JS parser dependency. The regex-vs-division rule above is the standard lexer
heuristic and is decidable from one character of context, which the scanner already carries. Any
case it still cannot decide must fail *safe*: leaving a script unwrapped means it evaluates as it
did before, which is the pre-165 behaviour, not a crash.

## Out of scope

- Replacing the wrap machinery with a real parser or a WASM JS tokenizer.
- Re-litigating DEC-039's choice of outcome (a) over (b); only the *trigger*, not the contract, is
  open here.

## References

- [[iteration-165-eval-scope-leak-contradicts-help]] — where this was found and why the plain path
  was narrowed to avoid it
- [[iteration-161-eval-and-flag-strictness]] — the `--stringify` wrap that first depended on the
  scanner
- [[decision-log]] — DEC-039
