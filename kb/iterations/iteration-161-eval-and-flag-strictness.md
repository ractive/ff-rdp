---
branch: iter-161/eval-and-flag-strictness
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-160-envelope-honesty.md
dogfood_path: |
  # Prereq: a live Firefox. Every line below was run on 2026-08-13 against
  # main and the `# →` comment records what main ACTUALLY printed.
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://news.ycombinator.com

  # --- Theme A: --stringify must accept what bare eval accepts ---
  ff-rdp eval 'const x = 5; x'
  # → {"results": 5, "total": 1}                                  (works today)
  ff-rdp eval --stringify 'const x = 5; x'
  # → error: expected expression, got keyword 'const'             (THE DEFECT)
  # → after this iteration: {"results": 5, "total": 1}
  printf 'const a=1;\nconst b=2;\na+b\n' | ff-rdp eval --stdin
  # → {"results": 3, "total": 1}                                  (works today)
  printf 'const a=1;\nconst b=2;\na+b\n' | ff-rdp eval --stdin --stringify
  # → error: expected expression, got keyword 'const'             (THE DEFECT)
  # → after: {"results": 3, "total": 1}
  ff-rdp eval --stringify 'const o = {a:1, b:[2,3]}; o'
  # → after: {"results": {"a":1,"b":[2,3]}, "total": 1}  — structured, not a grip
  ff-rdp eval --stringify 'const r = await Promise.resolve({n:7}); r'
  # → after: {"results": {"n":7}, "total": 1}  — stringify + await + multi-statement

  # --- Theme C: eval must return the whole string ---
  ff-rdp eval '"x".repeat(5000)' --jq '.results | length'
  # → main: 1000-ish (the preview grip); after: 5000
  ff-rdp eval '"x".repeat(5000)'
  # → main: {"results": {"type":"longString","value":"xxx…"}, no meta.truncated
  # → after: {"results": "xxxx…"} with all 5000 chars, no longString grip anywhere
  ff-rdp eval --stringify 'Array.from({length:400},(_,i)=>({i}))'
  # → after: a 400-element array in results (JSON well over 1000 chars)

  # --- Theme D: --fields / --sort must fail loud ---
  ff-rdp dom 'a' --limit 2 --fields bogusfield
  # → main: {"results": [{},{}], "total": 2}, exit 0            (data destroyed)
  # → after: error naming --fields, the unknown name, and the available keys; exit 1
  ff-rdp dom 'a' --limit 2 --sort nosuchfield
  # → main: document order, exit 0                             (silent no-op)
  # → after: error naming --sort, the unknown name, the available keys; exit 1
  ff-rdp dom 'a' --limit 2 --fields tag,text
  # → unchanged: only tag and text on each entry, exit 0
  ff-rdp dom 'a' --limit 2 --sort tag --asc
  # → unchanged: sorted ascending by tag, exit 0
  ff-rdp dom '.no-such-class-anywhere' --fields tag
  # → after: {"results": [], "total": 0}, exit 0 — an empty result set is not an error

  # --- Theme E: meta.eval_path is gone ---
  ff-rdp eval 'document.title' --jq '.meta'
  # → main: {"eval_path": "page-await"}; after: no eval_path key
first_call_sites: []
status: planned
title: "Iteration 161: --stringify cannot take a script, eval truncates long strings, and two flags fail silently"
type: iteration
tags:
  - iteration
---

# Iteration 161: `--stringify` cannot take a script, `eval` truncates long strings, and two flags fail silently

Four defects out of [[analysis-2026-08-13-what-ff-rdp-became]] §3.5 and §3.6, plus one
deletion from its §4. All four were measured on the wire on 2026-08-13; the commands and
their outputs are reproduced verbatim below. Three of the four are in `eval` — the command
whose entire job is handing an agent a raw value — and the fourth silently destroys the
output of every list command.

## The defects

### 1. `--stringify` cannot take a multi-statement script

Measured:

```
$ ff-rdp eval 'const x = 5; x'
{"results": 5, "total": 1}

$ ff-rdp eval --stringify 'const x = 5; x'
error: expected expression, got keyword 'const'

$ printf 'const a=1;\nconst b=2;\na+b\n' | ff-rdp eval --stdin
{"results": 3, "total": 1}

$ printf 'const a=1;\nconst b=2;\na+b\n' | ff-rdp eval --stdin --stringify
error: expected expression, got keyword 'const'
```

Bare `eval`, `--file` and `--stdin` all handle multi-statement scripts correctly. **Only
`--stringify` breaks.** The cause is one line — `eval.rs:132-136` splices the user's raw
text into a *call-argument slot*:

```rust
let (base, base_is_single_expression) = if stringify {
    (
        format!("(function(){{return {STRINGIFY_HELPER}({user_script});}})()"),
        true,
    )
```

`HELPER(const x = 5; x)` is not JavaScript. The doc comment at `eval.rs:128-131` even
records the constraint ("that is only valid JS if `user_script` is itself a single
expression (a pre-existing constraint, unrelated to this iteration)") — it was written
down and then left in place.

**The fix already exists in the same file.** The ~290 lines of statement-boundary
machinery built in iter-142 for top-level `await` solve exactly this problem:
`top_level_statement_boundaries` (`eval.rs:255`) finds the top-level statement splits
(tracking string/template state and bracket depth), `looks_like_single_expression`
(`eval.rs:176`) classifies the tail, and `wrap_top_level_await` (`eval.rs:420-439`)
already demonstrates the shape needed: run every earlier statement verbatim, then
`return (<last expression>)`. `--stringify` needs the same treatment with a plain
(non-async) IIFE, so that what reaches the helper is a single *call expression*
regardless of how many statements the user wrote.

Aggravating factor: the CLI tells users to do this. `args.rs:99`, in the AI AGENT TIPS
block of the top-level `--help`:

```
  - Use eval --stringify '<expr>' to get actual values instead of actor grip metadata
```

and `args.rs:521-523` repeats the advice in `eval --help`. Neither mentions that the flag
only accepts a single expression. The advice is given to agents, and agents write
multi-statement scripts.

### 2. A fake-green test that constructs the defect on every run

`eval.rs:884` `build_script_never_emits_eval_for_any_combination` iterates
`["document.title", "1 + 1", "const x = 1; x", "throw new Error('boom')"]` ×
`stringify ∈ {false,true}` × `isolate ∈ {false,true}` and asserts exactly one thing:

```rust
assert!(!s.contains("eval("), …);
```

For `("const x = 1; x", stringify=true)` it *generates the syntactically invalid
JavaScript from defect 1* and passes, because invalid JS contains no `eval(` either. The
CSP invariant it guards (iter-93) is real and worth keeping; the test as written cannot
distinguish "safe" from "broken". Strengthen it or delete it — do not leave it asserting
the wrong property.

Note the constraint from [[CLAUDE]]: all code stays in Rust, no polyglot tooling, so
there is no in-process JS parser to validate against. The only ground truth for "does this
parse" is Firefox itself — so the matrix belongs in a live test that evaluates each
generated script, with the unit test keeping the cheap `eval(` invariant.

### 3. `eval` is the one command that silently truncates long strings

`js_helpers::resolve_result` (`js_helpers.rs:57-82`) handles the `longString` grip
correctly:

```rust
Grip::LongString { actor, length, initial: _ } => {
    let full = LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
```

It serves ~18 commands. `eval::run` does not call it. `eval.rs:561` builds its result
from `eval_result.result.to_json()`, so when Firefox exceeds its ~1000-char inline limit
(`crates/ff-rdp-core/src/actors/string.rs:5-10`) the caller receives the preview grip:

```
$ ff-rdp eval '"x".repeat(5000)' --jq '.results | length'
1000
```

with **no `meta.truncated`**, no `hint`, and no warning. `eval.rs:572-575` even
recognises the grip — it wraps `Grip::LongString` in a `ScopedGrip` so the actor can be
released — and then releases the only handle by which the value could have been fetched.

DEC-015 added `LongStringActor::full_string()` for "any consumer that evaluates JS
producing large output" — that is the definition of `eval`. iter-102's longString sweep
([[iteration-102-longstring-sweep-and-reply-matching]]) covered `dom_walker`, `storage`
and `page_style` and missed `eval`.

There is also no workaround: no CLI command can fetch a longString by actor id. `inspect`
speaks `prototypeAndProperties` (`eval.rs:585`), a different protocol from `substring`
(`string.rs:17-31`). The truncated value is simply unreachable.

### 4. `--fields` and `--sort` fail silently

Measured:

```
$ ff-rdp dom 'a' --limit 2 --fields bogusfield
{"results": [{}, {}], "total": 2}          exit 0

$ ff-rdp dom 'a' --limit 2 --sort nosuchfield
… document order, unsorted …               exit 0
```

`--fields` silently destroys the data; `--sort` is a silent no-op. Both operate on untyped
`serde_json::Value` with no schema check:

- `output_controls.rs:75-93` `apply_fields` — `filter(|(k, _)| fields.iter().any(|f| f == k))`.
  Nothing matches, every object becomes `{}`.
- `output_controls.rs:39-52` `apply_sort` — `a.get(field)` yields `None` for both sides,
  `compare_values(None, None)` is `Equal`, the sort is stable, the order is unchanged.
- `output_controls.rs:100-113` `apply_fields_object` — the single-record counterpart, same
  bug (`perf vitals --fields bogus` → `{}`).

**The fix needs no static schema.** After the result set is built and before filtering,
compute the union of keys actually present across its object entries and reject any
`--fields`/`--sort` name outside that union. The data is already in hand; the union is the
schema.

`--jq-strict` (`args.rs:310-314`, `output_pipeline.rs:88-92`) already proves this project
believes in strict modes. It was applied to exactly one flag.

### 5. `meta.eval_path` discriminates nothing

`eval.rs:613-615` hard-sets it:

```rust
if let Some(m) = meta.as_object_mut() {
    m.insert("eval_path".to_owned(), json!("page-await"));
}
```

The second value (`"chrome"`) was deleted in iter-93 and DEC-020 confirmed it stays
deleted. Every `eval` since has emitted the same constant. It reads like a strategy
discriminator in the envelope and carries zero information.

## Themes

### Theme A — `--stringify` accepts exactly what bare `eval` accepts

Route the stringify wrap through the existing statement machinery instead of interpolating
raw text into an argument position. The target shape, for a multi-statement script:

```js
HELPER((function(){ <earlier statements verbatim> return (<last expression>); })())
```

and for a single expression, the current shape unchanged (`HELPER(<expr>)`) — do not
regress the common case into an extra IIFE.

Requirements:

- Reuse `top_level_statement_boundaries` and `looks_like_single_expression`; do not write a
  second classifier. If the tail statement is not a bare expression (a declaration, a
  control-flow construct, an explicit `return` the user wrote), fall back to the
  no-auto-return form, exactly as `wrap_top_level_await` does at `eval.rs:436-438` — the
  script still evaluates, it just yields `undefined` unless the user returns something.
- The `stringify` × `await` interaction must work. `build_script` (`eval.rs:144-148`)
  applies the await wrap *after* the stringify wrap; the stringify wrap now produces a
  single call expression in every case, so `base_is_single_expression = true` stays
  correct — but pin it with a test rather than an argument, because a synchronous IIFE
  containing `await` is a syntax error and the two wraps must compose in the right order
  (the `async` must end up on the outer function).
- The `--help` text must state what `--stringify` accepts. `args.rs:519-523` and the
  AI AGENT TIPS line at `args.rs:99` both currently imply "expression only" by example;
  once multi-statement works, say so.

### Theme B — the matrix test asserts something true

Replace `build_script_never_emits_eval_for_any_combination` (`eval.rs:884-902`) with a
pair that between them cover what it pretended to cover:

- a unit test keeping the iter-93 CSP invariant (`!s.contains("eval(")`) over the same
  matrix, plus a structural assertion that for `stringify=true` the user's statements land
  in statement position (the helper's argument list contains a call expression, not the
  user's raw text);
- a live test that hands each generated script to Firefox and asserts it evaluates without
  a `SyntaxError` — Firefox is the only JS parser this repo is allowed to use.

Deleting the old test outright is acceptable if both replacements land. Leaving it as-is
is not.

### Theme C — `eval` returns the whole value

`eval::run` resolves `Grip::LongString` through `LongStringActor::full_string()` before
building `results`, matching `js_helpers::resolve_result` (`js_helpers.rs:60-68`). Points
to get right:

- Ordering against `ScopedGrip` (`eval.rs:572-575`): fetch the full string *before* the
  grip is released, or the actor is gone.
- `--stringify` interacts with this. A stringified object larger than the inline limit
  comes back as a longString too; the JSON parse at `eval.rs:616-630` currently sees the
  grip's `to_json()` object rather than a `Value::String`, so it silently skips, and the
  caller gets a grip with no `meta.stringify_parsed: false` either. Fetching the full
  string first fixes both paths at once — verify it with a stringified payload well over
  1000 chars.
- Bound it. `LongStringActor::MAX_FETCH` (`string.rs:39`) is 16 MiB and `full_string`
  already enforces it; the resulting error must surface through the normal JSON error
  envelope, not a panic or a bare stderr line.
- Do not add a `meta.truncated` flag as the fix. The point is that nothing is truncated.

### Theme D — `--fields` and `--sort` reject names that are not there

Validate against the union of keys present in the result set. Design decisions this
iteration must make and record in [[decision-log]]:

**Strict by default, not opt-in.** Justification: a `--jq` filter that resolves to nothing
is often deliberate — jq is a query language and `.results[].maybe` is a legitimate probe,
which is why `--jq-strict` is opt-in. A `--fields`/`--sort` name that appears on *no*
entry in the result set is never deliberate: it is a typo or a field renamed out from
under a script, and today's outcome (`[{},{}]`, exit 0) is strictly worse than an error
in every case a caller could want. Adding a `--fields-lax` escape hatch would preserve a
behaviour nobody asked for; callers wanting tolerance already have `--jq`.

**Union, not intersection.** A key present on some entries and absent on others (`dom`
emits `text` only for elements that have it) must be accepted — otherwise the fix breaks
working commands.

**Skip validation when there is nothing to validate against.** An empty result set
(`total: 0`) and a result set with no object entries (a list of strings) both mean the
union is empty; erroring there would turn a legitimate empty query into a failure. Skip,
exit 0.

**The check must be structurally unavoidable.** There are 43 `apply_sort`/`apply_fields`/
`apply_fields_object` call sites across ~20 command modules; an opt-in validation helper
that each command has to remember to call will be forgotten by the next command added.
Make the existing methods return `Result` so the compiler forces every call site to
handle it. That is churn, but it is mechanical churn that buys fail-closed behaviour —
and it introduces no new public surface, which is why `first_call_sites` is empty.

Error text must name the flag, the offending name, and the keys that *are* available —
the standard set by the error messages the analysis singled out as this project's best
work. `AppError::User` (exit 1, `error.rs:195`) is the right variant: bad input from the
caller, routed through the JSON error envelope like every other command failure.

### Theme E — delete `meta.eval_path`

Remove the insert at `eval.rs:613-615` and the doc comment above it (`eval.rs:609-612`).
`live_61r_eval.rs:90-94` asserts `json["meta"]["eval_path"] == "page-await"` and must be
updated to assert absence. Check for other readers before deleting — DEC-020 names
`live_61l.rs` as a consumer of the same field, and the `--help` prose at `args.rs:500-503`
describes the path in words (that prose is accurate and stays; only the envelope field
goes).

## Out of scope

Two flags were **wrongly** reported as no-ops during the 2026-08-13 dogfooding. Both are
working code. Do not "fix" them:

- **`--redact-threshold`** applies to **trace output only** — its own help text says so.
  The dogfood lane tested it against `results`, which it never touches. There is no
  defect here.
- **`--max-frame-mb`** caps RDP frame payloads and has passing unit tests. The 2 MB repro
  that appeared to bypass it used a longString, which Firefox chunks over multiple
  `substring` responses and which therefore never crosses the wire as one oversized
  frame. The cap behaved correctly; the test was measuring the wrong thing.

Also out of scope, deliberately:

- `--log-level debug` producing no output on ordinary commands (§3.5). The filter is
  correct; the cause is that there are only 22 `debug!` sites and none in the command
  paths. That is an instrumentation iteration, not this one.
- `network`'s `--jq` shape divergence (`network.rs:326`), which is an envelope-shape
  problem belonging with [[iteration-160-envelope-honesty]].
- Any change to `click`/`type` hit testing (§3.1).

## Acceptance Criteria [11/12]

- [x] `unit_161_stringify_wraps_multi_statement_in_iife`: `build_script("const x = 5; x",
      true, false)` places the user's statements inside a zero-argument IIFE whose last
      statement is a synthesized `return (x)`, and the helper's argument list contains
      that call expression rather than the raw text `const x = 5; x`
- [x] `unit_161_stringify_single_expression_shape_unchanged`: `build_script("document.title",
      true, false)` is byte-identical to what `main` produces today (no extra IIFE for the
      common single-expression case)
- [x] `live_161_stringify_multi_statement_positional`: `ff-rdp eval --stringify
      'const x = 5; x'` exits 0 with `results == 5`, and stdout contains no
      `expected expression` text
- [x] `live_161_stringify_multi_statement_stdin`: `printf 'const a=1;\nconst b=2;\na+b\n' |
      ff-rdp eval --stdin --stringify` exits 0 with `results == 3` — the ASI-separated
      form, matching what bare `--stdin` already returns
- [x] `live_161_stringify_await_multi_statement`: `ff-rdp eval --stringify
      'const r = await Promise.resolve({n:7}); r'` exits 0 with `results == {"n": 7}` —
      the stringify wrap and the await wrap compose with `async` on the outer function
- [x] `live_161_build_script_matrix_evaluates`: every script the old matrix covered
      (`document.title`, `1 + 1`, `const x = 1; x`, `throw new Error('boom')`) ×
      `stringify ∈ {false,true}` × `isolate ∈ {false,true}` is handed to live Firefox and
      evaluates without a `SyntaxError`; the `throw` case surfaces its own `Error: boom`,
      which counts as a pass
- [x] `unit_161_build_script_emits_no_bare_eval`: the iter-93 CSP invariant survives —
      no output of `build_script` over that matrix contains `eval(`; the old
      `build_script_never_emits_eval_for_any_combination` (`eval.rs:884`) is gone from the
      file
- [x] `live_161_eval_returns_full_long_string`: `ff-rdp eval '"x".repeat(5000)'` returns
      `results` as a 5000-character string with `results.length == 5000`, and the envelope
      contains no `"type":"longString"` substring anywhere
- [x] `live_161_eval_stringify_long_payload_parses`: `ff-rdp eval --stringify
      'Array.from({length:400},(_,i)=>({i}))'` (JSON well over the ~1000-char inline
      limit) returns a 400-element array in `results` with no `meta.stringify_parsed`
      key — the parse at `eval.rs:616-630` succeeds because the full string was fetched
- [ ] `live_161_fields_and_sort_reject_unknown_names`: `ff-rdp dom 'a' --limit 2 --fields
      bogusfield` and `--sort nosuchfield` each exit 1 with a JSON error envelope whose
      message names the flag, the offending name, and at least one available key; the
      same command with `--fields tag,text` and with `--sort tag --asc` still exits 0 with
      the current output
- [x] `unit_161_field_validation_union_and_empty_set`: a field present on only one entry
      of a two-entry result set validates successfully; an empty result set and a result
      set of non-object values both validate successfully (no error, nothing filtered);
      `apply_fields_object` rejects an unknown name on a single-record result
- [x] `live_161_eval_meta_has_no_eval_path`: `ff-rdp eval 'document.title' --jq '.meta'`
      returns an object with no `eval_path` key, and `live_61r_eval.rs:90-94` asserts the
      absence rather than the constant

## Notes

- **Reproduce before fixing.** The analysis records that across iterations 135–151 the
  stated root cause diverged from reality at least eight times. Every command/output pair
  in `dogfood_path` and in `## The defects` was captured from a real run on 2026-08-13;
  re-capture them on your branch point before changing a line, and if one of them does
  not reproduce, say so in the PR instead of implementing against a phantom.
- **The `live_161_*` ACs are `#[ignore]`-gated and will never run in CI.** Ticking one
  requires the `[verified: <YYYY-MM-DD>, <measured result>]` annotation, and per
  [[analysis-2026-08-13-what-ff-rdp-became]] §3.4 a passing single-test invocation is weak
  evidence — `live_153_replace_emits_single_envelope` carried a truthful `[verified: …]`
  annotation for a broken feature because it was run in isolation. Quote
  `cargo run -p xtask -- live-sweep`'s `LIVE_SWEEP_SUMMARY executed=N` line.
- Themes A and B are one change viewed from two sides: the wrap that makes `--stringify`
  correct is also the thing the strengthened matrix test would have demanded. Land them
  together, and let the live matrix test fail first on the un-fixed wrap so there is
  evidence the test can see the defect.
- Theme D is the largest diff (43 call sites) and the least interesting. Do it last, in
  its own commit, so the `eval` fixes stay reviewable.
- Same honesty family as [[iteration-153-launch-replace-double-envelope]] and
  [[iteration-149-a11y-restore-honesty]]: in all three, the command did something —
  or refused to — and reported a shape the caller could not act on. Here the caller is an
  LLM agent, which cannot tell a 1000-char preview from a 1000-char answer, and cannot
  tell `[{},{}]` from a page with two empty links.

- **Adaptation from iter-160 (confirmed, not a scope change).** iter-160's PR shipped
  two `unit_160_*` ACs unticked despite the work being done, because
  `ac-fidelity-check.sh`'s heuristic 1 recognises only `live_`/`test_`/`bench_` slug
  prefixes and heuristics 2–3 read only the AC's first *physical* markdown line for a
  backticked symbol — those two ACs' first lines carried no backticks, only the wrapped
  continuation lines did. Checked against this plan's four `unit_161_*` ACs (lines
  357, 361, 378, 394): each already backticks its own slug name on the first physical
  line (e.g. `` `unit_161_stringify_wraps_multi_statement_in_iife`: `build_script(...)` ``),
  which is a symbol the diff will contain verbatim as `fn unit_161_…` — so heuristic 3
  should find it without relying on heuristic 1's prefix list. Not re-verified against a
  real diff (there isn't one yet), so re-check with `ac-fidelity-check.sh` once ACs are
  ticked, but no rewording is anticipated to be needed. This is the same gate;
  [[project_discipline_gate_gaps]] still tracks fixing the heuristics themselves (read
  `full_text`, learn the `unit_` prefix) so future plans don't have to hand-verify this.
- **Confirmed still accurate**: this plan's "out of scope" note that `network`'s `--jq`
  shape divergence belongs to iter-160 is correct as landed — iter-160 Theme F removed
  `cli.jq.is_some()` from `network`'s `use_detail_mode` and merged to main. Nothing in
  this plan needs to touch `network.rs` because of it.
- **Available but not required**: iter-160 added an optional `details: Option<Value>`
  field to `AppError::Unsupported`, merged flat into the error envelope
  (`error.rs`). Theme D's field/sort validation errors are specified as `AppError::User`,
  a different variant with no such field — that is fine, `AppError::User`'s existing
  shape already carries a message, and Theme D's error text (flag, offending name,
  available keys) is prose, not structured data a caller would parse out separately. Do
  not switch variants or add a details field to `AppError::User` just for symmetry with
  iter-160; do it only if a real caller-facing need for structured fields shows up.

## Measured on the branch (2026-08-14)

Every command/output pair in `dogfood_path` and in `## The defects` was
re-captured against the branch point before a line was changed. All five
defects reproduced exactly as written — nothing here was implemented against a
phantom. Two findings the plan did not anticipate:

- **`dom` has no `text` key.** AC `live_161_fields_and_sort_reject_unknown_names`
  names `--fields tag,text` as the control that must still exit 0. Measured:
  `ff-rdp dom 'a' --limit 2 --jq '.results[0]|keys'` → `["attrs","name","tag"]`
  (plus `ref`; `--text-attrs` adds `textContent`). There is no `text`. On main
  `--fields tag,text` therefore returned only `tag` and dropped `text`
  silently — a milder instance of the very defect Theme D fixes — and under
  DEC-035 it now exits 1. The AC's *substance* (unknown names rejected with
  flag, offender and available keys; `--sort tag --asc` and an empty result set
  unaffected) is implemented and verified live; only its control case rests on
  a false premise, so **the AC is left unticked rather than reworded**. The
  live test covers `--fields tag,name` as the equivalent two-key pass case and
  additionally pins `--fields tag,text` → exit 1 as measured.
- **`const` declarations DO leak between `eval` calls.** `eval --help`
  (`args.rs`) claims "each call already has its own scope and `const`/`let`
  declarations never leak across calls". Measured while building
  `live_161_build_script_matrix_evaluates`: running `const x = 1; x` twice in
  the same tab fails the second time with `redeclaration of const x`, because
  the non-stringify path sends the script to `Debugger.evalInGlobal` verbatim.
  The live matrix test works around it with a fresh global per combination.
  Out of scope here (it is a `--help` accuracy / scoping question, not one of
  this plan's five defects) and **not fixed** — worth a follow-up plan.

Scope note: `apply_sort`/`apply_fields`/`apply_fields_object` had 29 non-test
call sites, not the 43 the plan estimated (43 counts test call sites too).
`navigate.rs`'s `apply_network_controls` had to change return type as well,
since it wraps two of them.
