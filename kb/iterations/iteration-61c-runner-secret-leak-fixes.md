---
title: "Iteration 61c: NDJSON contract + secret-leak fixes + recorder fidelity"
type: iteration
date: 2026-05-16
status: planned
branch: iter-61c/runner-secret-leak-fixes
depends_on: [iteration-61b-recorder-cli-wiring]
tags:
  - iteration
  - scripts
  - recorder
  - security
  - secret-leak
  - ndjson
  - agent-speed
  - dogfood-feedback
---

# Iteration 61c: NDJSON contract + secret-leak fixes + recorder fidelity

Follow-up to [[iteration-61b-recorder-cli-wiring]], driven by the
findings in [[dogfooding-session-45]]. iter-61b closed most of the
iter-61 review feedback (F1, F2, F5, F6, F8, F10, F13 + a few partials).
The dogfood session against `admin.wardrobe-assistants.ch` confirmed
strict schema, dry-run reference validation, `--script-format`, and the
end-to-end record→replay flow all work — but it also surfaced a
**security-relevant** issue: the runner's contaminated stdout (F3) leaks
typed passwords that the runner itself dutifully marks `[REDACTED]` on
its own NDJSON line.

The headline is **fix the NDJSON contract**: each script step must emit
exactly one JSON line on stdout, with no leakage from the sub-command
helpers. This single fix resolves the major contamination issue *and*
the secret-leak that rides on top of it.

Themes:

- **A — NDJSON contract + secret containment (majors).** Make
  `run` emit one line per step. Stop the sub-command stdout from
  bleeding through. Re-run secret redaction on that captured output.
- **B — Recorder fidelity (minor majors).** Record `--timeout` and
  other recordable command flags. Auto-mark `secret: true` when typing
  into password-shaped selectors. Cosmetic JSON formatting.
- **C — Documentation drift (minors).** Fix `page-text` field path in
  docs, decide on `default_timeout_ms` (add or remove), and ship a
  small recipes section that an LLM can copy-paste.
- **D — Headless screenshot regression.** Cover the
  `screenshotContentActor` `noSuchActor` error and replace the
  misleading "relaunch with --headless" hint when already headless.
- **E — Carry-overs from iter-61b section F.** Items left in iter-61b
  with unchecked boxes that didn't make the iter-61b merge.

## Tasks

### A. NDJSON contract + secret containment

#### A1. Capture sub-command stdout instead of letting it through — **major**
- [ ] In `script/runner.rs` around line 558 (where verbs call into
  `commands::*::run`), redirect the sub-command's stdout into an
  in-memory buffer rather than the process stdout. Two acceptable
  shapes:
  - **Refactor**: split each command into a `run_core() ->
    Result<Value>` helper plus a thin stdout-printing shell. The
    script runner calls `run_core` directly, never the printing shell.
    Preferred — cleanest long-term.
  - **Capture**: temporarily replace stdout with a `Vec<u8>` while the
    sub-command runs, then discard or attach the captured bytes to
    the step's `results.raw_stdout` field (gated behind
    `--verbose-substeps`). Faster to land but leaves a wart.
- [ ] Pick the refactor — pulled forward from iter-62's "stdin
  contract" thinking anyway.
- [ ] Test: a 5-step script's stdout must contain exactly 6 lines (5
  step lines + 1 summary), each parsable as JSON.

#### A2. Re-redact captured sub-command output — **major (security)**
- [ ] If the capture from A1 ever surfaces (via `--verbose-substeps`
  or in error diagnostics), run it through `vars::redact_value` /
  `redact_string` so values typed into `input[type=password]` etc.
  cannot leak.
- [ ] Test: a script that types `"hunter2"` into
  `input[type=password]` and runs with `--verbose-substeps` must not
  contain `hunter2` anywhere in stdout or stderr; redaction must be
  string-level over the captured sub-command output, not just over
  the runner's own NDJSON.

#### A3. End-to-end stdout-purity assertion in CI — **major**
- [ ] Add `tests/e2e/script_runner_stdout.rs`: run every fixture
  script in `examples/scripts/` and `crates/ff-rdp-cli/tests/fixtures/`,
  assert each stdout line is valid JSON with the expected shape.
  Catches future regressions where someone adds a `println!` in a
  command helper.

### B. Recorder fidelity

#### B1. Record `--timeout` and other recordable flags — **minor**
- [ ] `script/recorder.rs` (or wherever `to_recorded_step()` lives —
  added in iter-61b A1): map the full set of recordable flags into
  the step JSON, not just the positional selector / text. Specifically:
  - `wait --timeout` → `wait.timeout`
  - `click --wait-for-text` / `--wait-for-selector` → matching fields
  - `navigate --wait-text` / `--wait-selector` → matching fields
  - `type --clear` → `type.clear`
- [ ] Test: record a session with each of the above flags; replay
  the recorded file; assert the per-step elapsed_ms is consistent
  with the recorded timeout (within 100 ms).

#### B2. Auto-mark password-shaped `type` steps as `secret: true` — **minor (security)**
- [ ] In the recorder's `type` capture, default `secret: true` if the
  selector contains `password`, `passwd`, `[type=password]`,
  `[type="password"]`, or `[type='password']` (case-insensitive).
  Document the heuristic so users know they can override with
  `--no-secret` if they ever record a non-password into a
  password-shaped selector.
- [ ] Test: record `type "input[type=password]" --text x` and assert
  the resulting step has `secret: true`.

#### B3. Cosmetic JSON formatting — **cosmetic**
- [ ] The recorded file currently ends the steps array with `}  ]`
  (two spaces, no newline before the closing bracket). Use a
  `serde_json::ser::PrettyFormatter` with the standard indent so the
  file looks the same as an LLM-authored one.
- [ ] Test: a 3-step recording equals (modulo step contents) the
  output of `cargo run -- run … --emit-recorded` on a hand-authored
  3-step script, ensuring the on-disk format is identical.

### C. Documentation drift

#### C1. `page-text` field path — **minor**
- [ ] `dogfooding-session-45` finding #4: `page-text` response is
  `.results` (string), not `.text`. Either:
  - Restore the `.text` alias for back-compat (minimal blast radius), or
  - Update `kb/reference/script-format.md` and the `page-text --help`
    examples to use `.results`.
- [ ] Pick restore-alias — `.text` is the more readable field name and
  this avoids breaking everyone's existing jq filters.

#### C2. `default_timeout_ms` — add or remove the documented field — **minor**
- [ ] The dogfood script tried `default_timeout_ms: 10000` at script
  scope. Today this errors with `deny_unknown_fields`. Two options:
  - **Add it**: extend `Script` with `default_timeout_ms:
    Option<u64>` and use it as the fallback for any verb that has a
    `timeout` field but doesn't set one. Big ergonomic win for
    assert-heavy scripts.
  - **Remove the wishful docs**: ensure `script-format.md` doesn't
    imply a global default-timeout knob exists.
- [ ] Pick add — assert-heavy scripts genuinely need this and the
  cost is one field + one fallback site.
- [ ] Test: a script with `default_timeout_ms: 50` and an
  `assert_text` that takes 200 ms fails; the same script with
  `default_timeout_ms: 5000` succeeds.

#### C3. Recipes section for `script-format.md` — **minor**
- [ ] Add a "Recipes" section: login-and-assert, navigate-and-screenshot,
  record-then-replay, secret-from-env. Each recipe is a copy-pasteable
  ~10-line script + the exact `ff-rdp run` invocation.

### D. Headless screenshot regression

#### D1. Diagnose `screenshotContentActor` `noSuchActor` — **major**
- [ ] Reproduce against `admin.wardrobe-assistants.ch` and at least one
  other SPA. Suspect: the screenshot actor is per-tab and is invalidated
  by a navigation that completes after the actor was looked up. Fix:
  re-resolve the screenshot actor at capture time, not at session start.
- [ ] Test: navigate → wait 500 ms → screenshot; assert it succeeds.

#### D2. Stop suggesting `--headless` when already headless — **minor**
- [ ] In the screenshot error path, the "relaunch with: ff-rdp launch
  --headless" hint fires unconditionally on actor failure. Gate it on
  the actual headless state (we already know it because `launch`
  recorded it). When headless, the hint should be empty or point to
  `doctor`.

### E. Carry-overs from iter-61b section F

These had unchecked boxes in iter-61b at merge time. Verify each
against current `main` and either close it or pull the work into this
iteration.

#### E1. F3 — NDJSON contamination
- [ ] Subsumed by A1. Mark closed in iter-61b once A1 merges.

#### E2. F4 — `rec.record(..).ok()` swallows errors — **major**
- [ ] `script/runner.rs:234` still uses `.ok()` per the iter-61b plan.
  Surface the error to stderr; with `--record-strict` (new) fail the
  whole run.

#### E3. F7 — `--env-file` semantics — **minor**
- [ ] Live test showed line-numbered errors *do* fire — F7 first item
  is closed. Re-verify and tick.
- [ ] Second item: values go to `extra_vars`, not the process env;
  `{{env.X}}` can't see them. Rename the flag to `--vars-file` (clearer
  + non-breaking semantics) and keep `--env-file` as a deprecated alias
  for one release with a stderr warning.

#### E4. F9 — `assert_network` configurable timeout — **minor**
- [ ] Add `timeout` field on `assert_network`; default to script-level
  `default_timeout_ms` (C2) or 5000 ms.

#### E5. F11 — env-loaded secrets not auto-redacted — **minor (security)**
- [ ] In `script/vars.rs:60`, when env values are read in, push their
  names + values into the same redaction set used for `vars.*`. Same
  ≥ 4-char minimum guard as F11 first item.

#### E6. F12 — diagnostics plumbing — **minor**
- [ ] Replace the `splitn`-based `extract_diagnostics` with an
  `AppError::Diagnostics { payload: serde_json::Value }` variant.
  Touches every assertion verb's failure path; mechanical but wide.

#### E7. Iter-61b A5 e2e test + B2 schema-examples test
- [ ] These were planned as part of iter-61b but the unchecked boxes
  remain. Verify whether they're actually present in
  `crates/ff-rdp-cli/tests/e2e/`; if not, add them here.

## Acceptance Criteria

- [ ] A 5-step script's stdout is exactly 6 lines, every line parseable
  as JSON (NDJSON contract restored).
- [ ] A script that types `hunter2` into `input[type=password]` has no
  occurrence of `hunter2` anywhere in stdout/stderr, even with
  `--verbose-substeps`.
- [ ] A recorded `wait --timeout 10000` round-trips: replaying the
  recorded file uses the 10 s timeout.
- [ ] Recording a `type "input[type=password]"` produces a step with
  `secret: true` by default.
- [ ] `page-text --jq '.text'` still works on `main`.
- [ ] A script with `default_timeout_ms: 5000` and an `assert_text` that
  hasn't set its own `timeout` uses 5000 ms.
- [ ] On the same fixture page, `navigate → wait → screenshot` succeeds
  in headless mode (the `noSuchActor` regression is fixed).
- [ ] On a screenshot failure when already headless, the error does not
  suggest "relaunch with --headless".
- [ ] All examples in `examples/scripts/` validate against the JSON
  Schema (closes iter-61b B2 if still open).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
  && cargo test --workspace -q` clean.

## Design Notes

- **Why the refactor (A1-option-1) over the capture (A1-option-2):**
  capturing stdout is a footgun — every new command someone adds will
  default to printing, and the contamination will silently come back.
  A `run_core() -> Result<Value>` split makes the contract enforceable
  by the type system: the script runner literally cannot call the
  printing shell.
- **Why default_timeout_ms over per-step:** in a 9-step login flow
  you'd otherwise repeat `"timeout": 10000` on 5 verbs. The script-level
  default is also the right place to express "this whole flow should
  complete in N seconds" which is what users actually want.
- **The `--env-file` → `--vars-file` rename** is the second-best
  outcome — best would be "also set the process env" — but env-var
  injection has subtle child-process leakage concerns that nobody
  signed up for when adding `--env-file`. Renaming is the honest fix.
- **Out of scope**: human-interaction recording (clicks in the browser
  window producing recorded steps) — that's iter-62-territory codegen,
  needs a separate iteration.
- **Out of scope**: any iter-62 page-map work. C2-add-it on
  `default_timeout_ms` deliberately leaves `page_map` references still
  rejected at dry-run (iter-61b C1 behavior).

## References

- [[dogfooding-session-45]] — the bug report this iteration closes.
  Findings #1, #2, #3, #4, #5, #6, #7, #8 map to A1, A2, D1, C1, C2,
  B1, B2, B3 respectively.
- [[iteration-61-script-runner-recorder]] — runner core
- [[iteration-61b-recorder-cli-wiring]] — section F carry-overs
  reconciled in E1–E7.
- [[iteration-62-page-map-index]] — referenced by C2's decision to
  add a script-level default that page-map authoring will inherit.
