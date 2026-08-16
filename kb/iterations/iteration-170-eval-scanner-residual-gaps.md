---
title: "Iteration 170: eval's scanner still cannot see into ${} or past a }"
type: iteration
date: 2026-08-16
status: planned
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

## Tasks

### A. Measure
- [ ] Run every line of `dogfood_path` against a live Firefox and paste the actual output here
- [ ] State explicitly, per gap, whether it reproduces

### B. Template interpolation
- [ ] Re-enter `${…}` in `top_level_statement_boundaries` with full nested state
- [ ] Unit tests: a backtick, a quote and a `;` inside an interpolation

### C. The `}` ambiguity
- [ ] Decide: heuristic or leave as documented. Record the decision and its reasoning

## Acceptance Criteria [0/3]

- [ ] unit_170_interpolation_is_scanned: a `` ` `` or `;` inside `${…}` does not end the template
      literal or produce a boundary — **only if Theme A shows gap 1 reproduces**
- [ ] The `}`-ambiguity decision is recorded (here if it stays as-is, in `kb/decision-log.md` if
      the behaviour changes)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The scanner must not become a JS parser — all code stays in Rust and this repo has no JS parser
dependency (`kb/decision-log.md`, DEC-039 and iter-167's design notes). Any case the scanner
cannot decide must fail safe: an unwrapped script evaluates as it always did, which is a working
behaviour, while a wrongly-consumed one is a SyntaxError.

## Out of scope

- Replacing the wrap machinery with a real parser or a WASM JS tokenizer. This was out of scope in
  iter-167 for a policy reason that has not changed, and it remains out of scope here: it is the
  answer to "make the scanner perfect", and the repo has decided against paying that price.
- Re-litigating DEC-039. iter-167 Theme C re-examined the *trigger* asymmetry on live evidence and
  left it in place; the contract itself was out of scope there and stays out of scope here.

## References

- [[iteration-167-eval-statement-scanner-is-not-a-tokenizer]] — the fix that closed the other
  three gaps and documented these two
- [[decision-log]] — DEC-039, including its iter-167 revisit note
