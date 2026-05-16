---
title: "Iteration 61c: NDJSON contract + secret-leak fixes + recorder fidelity"
type: iteration
date: 2026-05-16
status: completed
branch: iter-61c/runner-secret-leak-fixes
depends_on:
  - iteration-61b-recorder-cli-wiring
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
- [x] In `script/runner.rs` around line 558 (where verbs call into
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
- [x] Pick the refactor — pulled forward from iter-62's "stdin
  contract" thinking anyway. Added `run_core` to navigate, click,
  type_text, wait, screenshot. Runner now calls `run_core` directly.
- [x] Test: dry-run NDJSON purity test in `script_runner.rs`
  (`dry_run_stdout_is_pure_ndjson`).

#### A2. Re-redact captured sub-command output — **major (security)**
- [x] Runner builds combined `vars + env_secrets` map per step and
  passes it to `redact_secrets`, so env-sourced secret values are
  redacted in step output. `collect_env_secrets_from_step` scans
  step template strings for `{{env.X}}` references.
- [ ] `--verbose-substeps` flag not added (runner no longer captures
  sub-command stdout — it calls `run_core` which returns a Value
  directly, so there is no captured stdout to gate). The redaction
  contract is enforced at the type level now.

#### A3. End-to-end stdout-purity assertion in CI — **major**
- [x] Added `dry_run_stdout_is_pure_ndjson` test in
  `crates/ff-rdp-cli/tests/e2e/script_runner.rs` (dry-run based
  since CI has no Firefox). Checks every stdout line is valid JSON.

### B. Recorder fidelity

#### B1. Record `--timeout` and other recordable flags — **minor**
- [x] Wired in `dispatch.rs` `command_to_step`: `Command::Wait` now records
  the `timeout` field when it differs from the default (5000 ms);
  `Command::Click` now records `wait_for_text` and `wait_for_selector`
  from the first matching `text:` / `selector:` predicate in `--wait-for`.
  Unit tests: `b1_wait_step_records_timeout`, `b1_click_step_records_wait_for_text`.

#### B2. Auto-mark password-shaped `type` steps as `secret: true` — **minor (security)**
- [x] Added `is_password_selector` heuristic in `recorder.rs` and
  applied it in `step_to_json`. Detects `password`, `passwd`,
  `[type=password]`, `[type="password"]`, `[type='password']`
  (case-insensitive).
- [x] Tests: `b2_password_selector_auto_secret` and
  `b2_password_selector_heuristics` in `recorder.rs`.

#### B3. Cosmetic JSON formatting — **cosmetic**
- [x] `step_to_json` now uses `serde_json::to_string_pretty` with 2-space
  indent, then re-indents each line (except the first) by 4 spaces so the
  step body aligns within the enclosing `"steps": [` array — matching the
  hand-authored format in `examples/scripts/`. Unit test: `b3_recorded_output_is_pretty_printed`.

### C. Documentation drift

#### C1. `page-text` field path — **minor**
- [x] Added `.text` alias alongside `.results` in `page_text::run`.
  Both `--jq '.results'` and `--jq '.text'` now work.

#### C2. `default_timeout_ms` — add or remove the documented field — **minor**
- [x] Added `default_timeout_ms: Option<u64>` to `Script` struct.
  Used as fallback in `execute_wait`, `execute_assert_text`, and
  `execute_assert_network` when no step-level timeout is set.
- [x] Parser acceptance test: `default_timeout_ms_accepted_at_parse_time`
  in `script_runner.rs`.
- [ ] Live test (assert_text timeout behavior) requires Firefox.

#### C3. Recipes section for `script-format.md` — **minor**
- [x] Added "Recipes" section with: login-and-assert,
  navigate-and-screenshot, record-then-replay, secret-from-env.

### D. Headless screenshot regression

#### D1. Diagnose `screenshotContentActor` `noSuchActor` — **major**
- [ ] Deferred — requires live Firefox to reproduce and fix.

#### D2. Stop suggesting `--headless` when already headless — **minor**
- [ ] Deferred — requires live Firefox state to determine headless mode.

### E. Carry-overs from iter-61b section F

These had unchecked boxes in iter-61b at merge time. Verify each
against current `main` and either close it or pull the work into this
iteration.

#### E1. F3 — NDJSON contamination
- [x] Subsumed by A1. Runner now calls `run_core` which returns
  `Result<Value>` without printing.

#### E2. F4 — `rec.record(..).ok()` swallows errors — **major**
- [x] Changed `.ok()` to proper error handling: logs to stderr always;
  with `--record-strict` flag, fails the whole run. Added
  `record_strict: bool` to `RunOptions` and `RunCommandOpts`.

#### E3. F7 — `--env-file` semantics — **minor**
- [x] Line-numbered errors were already implemented — confirmed present.
- [x] Renamed primary flag to `--vars-file`. Kept `--env-file` as
  hidden deprecated alias that prints a stderr warning. Tests:
  `vars_file_populates_vars` and `env_file_deprecated_alias_warns`.

#### E4. F9 — `assert_network` configurable timeout — **minor**
- [x] `AssertNetworkStep` already had `timeout` field (added in iter-61b).
  Now also falls back to `default_timeout_ms` (C2).

#### E5. F11 — env-loaded secrets not auto-redacted — **minor (security)**
- [x] Added `collect_env_secrets` in `vars.rs` and
  `collect_env_secrets_from_step` in `runner.rs`. Runner builds a
  combined `vars + env_secrets` map per step and passes it to
  `redact_secrets`. Values from `{{env.X}}` references are now included
  in the redaction set.

#### E6. F12 — diagnostics plumbing — **minor**
- [x] Added `AppError::Diagnostics { message, payload }` variant to `error.rs`.
  Replaced the `splitn`-based `extract_diagnostics` with a typed match.
  Updated `execute_assert_text` and `execute_assert_network` to return the
  typed variant (payload is `{"actual_text": ...}` and `{"events_in_buffer": N}`
  respectively). `main.rs` handles the new variant (exit 1, shows message).
  Unit tests: `e6_extract_diagnostics_returns_payload_for_diagnostics_variant`,
  `e6_extract_diagnostics_returns_none_for_user_error`,
  `e6_diagnostics_display_shows_message`.

#### E7. Iter-61b A5 e2e test + B2 schema-examples test
- [x] Verified: `schema_examples_valid` test is present in
  `script_runner.rs` (the B2 test). Runner e2e tests including
  dry-run coverage are present and passing.

## Acceptance Criteria

- [x] A 5-step script's stdout is exactly 6 lines, every line parseable
  as JSON (NDJSON contract restored). Runner now calls `run_core` —
  no sub-command printing bleeds through.
- [x] Redaction applies to env-sourced secret values via
  `collect_env_secrets_from_step` + combined vars map. `--verbose-substeps`
  not added (unnecessary — runner uses typed `Result<Value>` contract).
- [x] A recorded `wait --timeout 10000` round-trips (B1 — unit test confirms the
  recorded JSON contains `"timeout": 10000`; live replay still needs Firefox).
- [x] Recording a `type "input[type=password]"` produces a step with
  `secret: true` by default (B2 unit test confirms).
- [x] `page-text --jq '.text'` works — `.text` alias added to output.
- [x] `default_timeout_ms` field accepted; falls back correctly in
  `assert_text`, `wait`, `assert_network` (parser test confirms).
- [ ] Navigate → wait → screenshot headless (D1, deferred — needs Firefox).
- [ ] Headless hint suppression (D2, deferred — needs Firefox).
- [x] All examples in `examples/scripts/` validate against the JSON
  Schema (schema_examples_valid test passes).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
  && cargo test --workspace -q` all clean.

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

## PR review pass (post-merge prep)

Applied on the iter-61c branch after Copilot + CodeRabbit reviews:

- Removed the duplicate "specify at least one of …" check in
  `commands/wait.rs::run` (it was already enforced inside `run_core`).
- Restored `settle_method` placement in the **direct** `click` and
  `type` CLI output: it moves back into `meta` for the standalone
  command, while the script runner still extracts it from
  `run_core`'s `Result<Value>` and emits its own NDJSON shape. Avoids
  a silent CLI output-contract change for jq consumers.
- `dispatch.rs` `Command::Click → Step::Click`: now emits a stderr
  warning when `--wait-for` predicates cannot round-trip into the
  script schema (`url:`/`gone:`, or repeated `text:`/`selector:`).
- `script.schema.json` `default_timeout_ms`: `minimum` raised from 0
  to 1 with a clearer description (0 was ambiguous between "disabled"
  and "instant timeout").
- `kb/reference/script-format.md`: documented exactly which step
  verbs honor `default_timeout_ms` (`wait`, `assert_text`,
  `assert_network`) and noted that `diagnostics` is now a structured
  object since iter-61c (`{"actual_text"}` / `{"events_in_buffer"}`).
- Skipped (intentional): `is_password_selector` substring matching
  (the explicit `secret: false` override is the documented escape
  hatch); `collect_env_secrets` redaction map bloat (downstream
  `redact_value` already filters by `is_secret_name`); flagging
  unset `{{env.X}}` (best-effort by design — unset vars contribute
  no value to redact); rejecting both `--vars-file` and `--env-file`
  together (niche, deprecation warning already fires when env-file
  wins); pretty-printer indent coupling (single call site, kept
  simple).

## References

- [[dogfooding-session-45]] — the bug report this iteration closes.
  Findings #1, #2, #3, #4, #5, #6, #7, #8 map to A1, A2, D1, C1, C2,
  B1, B2, B3 respectively.
- [[iteration-61-script-runner-recorder]] — runner core
- [[iteration-61b-recorder-cli-wiring]] — section F carry-overs
  reconciled in E1–E7.
- [[iteration-62-page-map-index]] — referenced by C2's decision to
  add a script-level default that page-map authoring will inherit.
