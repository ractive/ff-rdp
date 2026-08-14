---
branch: iter-165/eval-scope-leak
date: 2026-08-14
depends_on: []
dogfood_path: |
  # `eval --help` promises per-call scope. Measure whether it holds.
  ff-rdp launch --headless --debug-port 7301
  ff-rdp --port 7301 navigate https://example.com
  ff-rdp --port 7301 eval "const x = 1; x"
  # → 1
  ff-rdp --port 7301 eval "const x = 1; x"
  # → on main: error, "redeclaration of const x". Per `eval --help` this must
  #   succeed and print 1 again — the second call is supposed to have its own
  #   scope.
  ff-rdp --port 7301 eval "let y = 1; y"
  ff-rdp --port 7301 eval "let y = 1; y"
  # → same question for `let`; record what main actually does before choosing
  #   a fix, the two bindings may not behave identically.
  ff-rdp --port 7301 eval --stringify "const z = 1; z"
  ff-rdp --port 7301 eval --stringify "const z = 1; z"
  # → iter-161 wraps --stringify statements in an IIFE, so this path is
  #   expected to be unaffected. Confirm; the asymmetry is the clue.
status: planned
title: "Iteration 165: eval's const/let bindings leak across calls, contradicting eval --help"
type: iteration
tags:
  - iteration
  - eval
---

# Iteration 165: `eval`'s `const`/`let` bindings leak across calls, contradicting `eval --help`

Carry-over from [[iteration-161-eval-and-flag-strictness]], filed 2026-08-14 after that PR
merged. It should have been filed before the merge per CLAUDE.md's carry-over rule; it was
recorded only in iter-161's Notes and would otherwise have been lost.

## The defect

`crates/ff-rdp-cli/src/cli/args.rs:510-513` tells the user:

> Since iter-93, scripts are routed through Firefox's `Debugger.evalInGlobal` sandbox scope
> (which bypasses page CSP), so each call already has its own scope and `const`/`let`
> declarations never leak across calls. The `--no-isolate` flag is kept for backwards
> compatibility but is now a no-op.

Measured on 2026-08-13 while building `live_161_build_script_matrix_evaluates`: running
`const x = 1; x` twice in the same tab fails the second time with `redeclaration of const x`.
The non-stringify path sends the script to `Debugger.evalInGlobal` verbatim, and that call
shares one global lexical environment across invocations — the sandbox bypasses page CSP but
does not give each call a fresh scope. The live matrix test currently works around this with a
fresh global per combination, so the suite is green over a condition the product does not meet.

This is the same class of defect as [[iteration-160-envelope-honesty]]: output (here, `--help`)
asserting more than the command knows. It is a user-visible contract — anyone scripting
`ff-rdp eval` in a loop hits it on the second iteration — and the documented behaviour is the
one users will have written against.

## Open question the research must settle first

**Which side is wrong — the help text or the code?** Do not assume. Two defensible outcomes:

- **(a) The code is wrong.** Per-call scope is the better contract (it is what the help promises,
  what iter-93 intended, and what makes `eval` idempotent in a loop). Fix by wrapping the
  non-stringify path the way iter-161 already wraps `--stringify`: `wrap_statements_in_iife`
  exists in `eval.rs` and is async-aware. Risk: an IIFE changes the completion value of a bare
  declaration and may break scripts that currently rely on a binding persisting across calls.
- **(b) The help text is wrong.** Cross-call persistence may be load-bearing for interactive
  use (declare once, reuse across several `eval` calls against the same tab). If so, correct
  `args.rs` to describe what actually happens and say how to get a fresh scope (`--stringify`,
  or wrap it yourself).

Settle this on evidence, not preference: check what `Debugger.evalInGlobal` guarantees in the
Firefox source at the installed version, and check whether anything in the repo (tests,
playbooks, `kb/`) depends on bindings surviving between calls. Record the answer in the
decision log either way — this is a DEC-worthy contract choice, not an implementation detail.

## Themes

### Theme A — establish the real behaviour, all four paths

Measure and pin, per the `dogfood_path` above: `const` and `let`, on both the non-stringify and
`--stringify` paths. `var` and bare assignment too, since they go to a different binding store
and may already survive. The output is a table in this plan of what each combination does on
main, before any fix.

### Theme B — make code and documentation agree

Implement whichever of (a) or (b) Theme A's evidence supports, and state in the plan why the
other was rejected. If (a): the non-stringify path gets the same IIFE wrap, and `--no-isolate`
needs a decision too — it is currently documented as a no-op, which is only coherent under the
help text's current (false) claim.

### Theme C — drop the test's workaround

`live_161_build_script_matrix_evaluates` uses a fresh global per combination to route around
this. Under (a) that workaround must be removed and the test must pass without it, otherwise
the fix is unverified. Under (b) it stays, with a comment naming this plan.

## Acceptance Criteria [0/4]

- [ ] live_165_scope_behaviour_table: a live test asserts the measured behaviour of `const`,
      `let` and `var` on both the plain and `--stringify` paths across two consecutive `eval`
      calls against one tab, with the table recorded in this plan
- [ ] live_165_repeated_const_matches_help: two consecutive `ff-rdp eval "const x = 1; x"` calls
      against the same tab both exit 0 and both print `1` — or, under outcome (b), the test
      asserts the documented-and-measured persistence instead and this AC is rewritten to match
      the chosen contract before being ticked
- [ ] unit_165_help_text_matches_behaviour: a test over `args.rs`'s `eval` `long_about` asserts
      the scoping sentence describes the implemented behaviour, so the two cannot drift again
- [ ] live_161_build_script_matrix_evaluates: passes without its fresh-global-per-combination
      workaround under outcome (a); under (b) the workaround carries a comment citing this plan

## Notes

- Found by the iter-161 implement agent while building its live matrix; not fixed there because
  it is a scoping/`--help` question rather than one of that plan's five defects.
- The `--stringify` path is expected to be immune because
  [[iteration-161-eval-and-flag-strictness]] wraps statements in a value-producing IIFE. That
  asymmetry is the most direct evidence for what a fix on the plain path would look like.
- Related: [[iteration-161-eval-and-flag-strictness]] (where it was measured),
  [[iteration-160-envelope-honesty]] (same class: output asserting more than the code knows).
