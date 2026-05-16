---
title: "Iteration 61b: Recorder CLI wiring + iter-61 review-feedback fixes"
type: iteration
date: 2026-05-16
status: planned
branch: iter-61b/recorder-cli-wiring
depends_on: [iteration-61-script-runner-recorder]
tags:
  - iteration
  - scripts
  - recorder
  - agent-speed
  - review-feedback
  - schema-strict
  - dry-run
---

# Iteration 61b: Recorder CLI wiring + iter-61 review-feedback fixes

Follow-up to [[iteration-61-script-runner-recorder]]. Iter-61 landed the
verb set, runner core, variable system, and YAML/JSON dual-parse cleanly
— but a self-audit against the plan surfaced four real gaps. This
iteration closes them.

The headline is **recorder CLI wiring (gap #1 from the iter-61 audit)**:
`ff-rdp record start <out>` followed by manual `ff-rdp navigate`,
`click`, `type`, etc. should produce a replayable script. Today the
recorder only captures steps re-played through `ff-rdp run --record`.
The `recorder::record_step_to_active` helper exists but is marked
`#[allow(dead_code)]` because no dispatch hook calls it — this iteration
makes it live.

The remaining three gaps (strict-schema parser, dry-run reference
validation, dead `--script-format` flag) are small enough to bundle in
the same PR — they touch the same files and the reviewer is already
paged in.

Themes:

- **A — Recorder CLI wiring.** Hook the recorder into the post-success
  dispatch path so every recordable CLI invocation appends to the active
  recording.
- **B — Strict-schema parsing.** `#[serde(deny_unknown_fields)]` on the
  script types so a typo like `clikc:` errors loudly instead of being
  silently dropped.
- **C — Dry-run reference validation.** Detect deferred iter-62 features
  (`page_map`, `field`, `api_route`) at `--dry-run` time so missing or
  misnamed references fail before execution starts.
- **D — Make `--script-format` actually do something.** Today it's
  parsed and immediately `let _ = fmt_override`'d. Either honour it or
  remove it.

## Tasks

### A. Recorder CLI wiring

#### A1. `Command → Option<Step>` mapper
- [ ] Add a `to_recorded_step(&self) -> Option<Step>` method on the
  `Command` enum in `crates/ff-rdp-cli/src/cli/args.rs`, or an
  equivalent free function in `script/format.rs`. Return `Some` for
  recordable verbs:
  `navigate`, `click`, `type`, `wait`, `scroll`, `screenshot`, `eval`,
  `reload`, `back`, `forward`. Return `None` for inspection-only
  commands (`tabs`, `dom`, `snapshot`, `console`, `network`,
  `page-text`, `cookies`, `storage`, `sources`, `geometry`, `styles`,
  `computed`, `responsive`, `a11y`, `doctor`, `daemon *`,
  `launch`, `record *`, `inspect`).
- [ ] Document the recordable-vs-skipped split in
  `kb/reference/script-format.md` so users know what to expect.

#### A2. Hook into dispatch post-success
- [ ] In `crates/ff-rdp-cli/src/dispatch.rs`, after a command's
  in-process function returns `Ok(_)`, call
  `recorder::record_step_if_active(&cmd)`. Failures default to skipped
  (replay shouldn't include broken steps); a top-level
  `--record-failures` flag opts in. Best-effort: a recorder write
  failure logs to stderr but does not fail the command.
- [ ] Remove the `#[allow(dead_code)]` from `record_step_to_active` —
  it's no longer dead.

#### A3. Concurrency-safe append
- [ ] Use `fs2::FileExt::lock_exclusive` around the append in
  `recorder::append_step`. Today, two parallel CLI invocations against
  the same recording file would interleave bytes and produce malformed
  JSON.
- [ ] Add `fs2` to `ff-rdp-cli/Cargo.toml` (workspace pin if other
  crates already use it).
- [ ] Test: spawn two CLI commands in parallel against an active
  recording; assert the resulting file parses as valid JSON with both
  steps in some order.

#### A4. Ref resolution at record time
- [ ] When a CLI invocation uses `--ref e23` (iter-60), the recorder
  must NOT emit `"ref": "e23"` — refs are session-scoped and won't
  resolve in a fresh replay. Resolve the ref to its underlying CSS
  selector (via the daemon's per-tab ref map from iter-60) at the
  moment of recording, and emit
  `"selector": "<resolved css>"` instead.
- [ ] If the ref cannot be resolved (e.g. recording is active but the
  daemon is not running, or the ref expired): skip the step and log to
  stderr; do not write a broken step.
- [ ] Test: record a `click --ref e23` against the e2e fixture; assert
  the produced file has `selector:` not `ref:`.

#### A5. End-to-end test
- [ ] Add to `tests/e2e/script_runner.rs`: spawn `record start <out>`,
  run a sequence (`navigate`, `click`, `type`, `wait`) against the
  fixture page, `record stop`, parse the file, assert all four steps
  are present with the right verbs and arguments. Replay via `run` to
  verify round-trip fidelity (this is iter-61's acceptance criterion #2,
  which couldn't pass before this iteration).

### B. Strict-schema parsing

#### B1. `deny_unknown_fields` on all script types
- [ ] Add `#[serde(deny_unknown_fields)]` to `Script`, every `*Step`
  struct in `script/format.rs`, and `ElementTarget`. Today a typo like
  `{"navigate": {"urll": "..."}}` deserializes silently to default
  values; with this, it errors with a precise field-name message.
- [ ] Add a test for each: malformed JSON with a typo'd field name is
  rejected with an error that names the unknown field.

#### B2. Verify against the shipped JSON Schema
- [ ] Add a build-time or test-time check that
  `schemas/script.schema.json` accepts every example fixture in
  `examples/scripts/`. Use the `jsonschema` crate (lightweight). This
  catches drift between the Rust types and the schema file.
- [ ] CI test: `cargo test schema_examples_valid` validates each
  `examples/scripts/*.{json,yaml}` against the schema.

### C. Dry-run reference validation

#### C1. Hoist `deferred_iter62_check` into the dry-run path
- [ ] Today `script/runner.rs:498` `deferred_iter62_check` only runs
  inside the execution loop. Call it from `run_dry` for every step;
  fail the dry-run with the same message ("page_map / field / api_route
  requires iter-62 page-map support — not yet implemented") so scripts
  with iter-62 refs surface the problem *before* any command runs.
- [ ] Test: a script with a `page_map:` reference in dry-run mode exits
  non-zero with the deferred-feature message.

### D. `--script-format` flag

#### D1. Honour the flag or remove it
- [ ] Today `commands/run.rs:31` computes `fmt_override` then immediately
  `let _ = fmt_override` discards it. Two acceptable resolutions:
  - **Honour it**: pipe through to `parse_script_file` as the format
    override (the API already accepts an `Option<ScriptFormat>`).
  - **Remove it**: drop the flag from CLI args.
- [ ] Pick honour. Reason: piping into `run` via stdin (already a
  reasonable agent pattern via `bash -c "ff-rdp run /dev/stdin
  --script-format json <<EOF ..."`) needs format coercion because
  `/dev/stdin` has no extension. Add a test for stdin + override.

### F. Additional review feedback from PR #64

Findings from the post-merge review (local + CodeRabbit + Copilot) that
are not covered by themes A–D above. Grouped by severity.

#### F1. `{{steps[N].results.X}}` resolver vs. runner mismatch — **major**
- [ ] `script/runner.rs` pushes the inner step result directly into
  `step_results`, but `script/vars.rs` docstring and
  `kb/reference/script-format.md` advertise `{{steps[N].results.X}}`.
  Either wrap pushed values as `{"results": ...}` or fix the docs and
  resolver to use `{{steps[N].X}}`. Pick wrap — it matches the
  documented contract and keeps the `results` namespace open for future
  per-step metadata (timing, refs).

#### F2. `ref`-as-selector regression for `click` / `type` — **major**
- [ ] In `script/runner.rs:558,568,606` the raw `ref` id is handed to
  `commands::click::run` / `commands::type_text::run` as if it were a
  CSS selector. The normal CLI path resolves refs through the daemon
  first (`dispatch.rs:338–344`). Route script verbs through the same
  resolver so `ref:` targets work in scripts.
- [ ] Test: script step `{"click": {"ref": "e23"}}` against the e2e
  fixture clicks the resolved element, not the literal selector `e23`.

#### F3. Sub-command stdout contaminates NDJSON — **major**
- [ ] `script/runner.rs:558` invokes the regular command runners which
  write their own JSON envelopes to stdout *before* the runner emits its
  per-step NDJSON line. Either capture and suppress sub-command stdout,
  or refactor verbs to call lower-level helpers that return values
  instead of printing.

#### F4. Recorder lifecycle on failed runs — **major**
- [ ] `commands/run.rs:61` early-`?`s on a failing step (default
  `--bail`), skipping `FileRecorder::finish()`. The `--record` output
  is left with `"steps": [\n` and never closed → invalid JSON.
  Wrap in a guard / RAII type that always calls `finish()`.
- [ ] `script/runner.rs:234` uses `rec.record(..).ok()` and silently
  drops I/O errors. Surface them (warn to stderr or fail the run with
  `--record-strict`).

#### F5. `base_url` is parsed but ignored — **major**
- [ ] Resolve relative `navigate` URLs against the script's `base_url`
  in `script/runner.rs:386`. Today `base_url` is part of the schema but
  has no effect.
- [ ] Test: a script with `base_url: "https://example.com"` and a step
  `{"navigate": {"url": "/login"}}` navigates to
  `https://example.com/login`.

#### F6. Summary counts wrong on bail — **minor**
- [ ] `runner.rs:245` computes `passed = total - failed`, counting
  un-executed steps as passed. Track `executed` / `succeeded`
  separately; report `executed`, `succeeded`, `failed`, `skipped`.

#### F7. `--env-file` semantics — **minor**
- [ ] `dispatch.rs:660`: malformed lines (no `=`) silently dropped —
  fail with a line-numbered error.
- [ ] `dispatch.rs:658`: values are loaded into `extra_vars`, not the
  process environment, so `{{env.X}}` doesn't see them. Either rename
  the flag (`--vars-file`) or also set them in the env. Pick the
  rename — env-side leakage to child processes is surprising.

#### F8. `assert_no_console_errors` silent downgrade — **minor**
- [ ] `commands/console.rs:200`: when
  `get_cached_messages(PageError+ConsoleAPI)` errors, the helper falls
  back to ConsoleAPI-only without flagging the gap. Propagate the
  error so the assertion fails loud instead of silently passing.

#### F9. `assert_network` 500 ms drain — **minor**
- [ ] `commands/network.rs:489` / `script/runner.rs:843`: hardcoded
  500 ms drain in direct mode produces flaky negatives. Add a
  `timeout` field on `assert_network` mirroring `assert_text`, and
  default to the script's `default_timeout_ms`.

#### F10. Recorder edge cases — **minor**
- [ ] `script/recorder.rs:210`: `output_path.file_name().unwrap_or_default()`
  produces an empty filename for paths ending in `/`. Use
  `.context("output path must have a filename component")?`.
- [ ] `script/recorder.rs:91`: `record start` is not atomic — two
  parallel starts race. Use `OpenOptions::create_new` for the state
  file to make existence-check atomic.
- [ ] `script/recorder.rs:120`: `finalise_output_file` is not
  idempotent; a second call writes `]\n}\n` again. Mark finalised
  state in the file (or in the state file) and no-op on second call.

#### F11. Secret-redaction edge cases — **minor**
- [ ] `script/vars.rs:140`: substring `replace` over every string
  field for any `*password*` var. A short value (e.g. `"a"`) wipes
  unrelated output. Require minimum-length (≥ 4 chars) before
  enabling substring redaction; otherwise redact by exact match only.
- [ ] `script/vars.rs:60`: `env.NAME` values are not auto-redacted
  (only `vars.*` are). Iterate over env-loaded values too when
  building the redaction set.

#### F12. Diagnostics plumbing — **minor**
- [ ] `script/runner.rs:953`: `extract_diagnostics` parses structured
  data out of a formatted error string via `splitn`. Replace with an
  `AppError` variant carrying a typed `diagnostics: serde_json::Value`
  payload.

#### F13. Unsubstituted assertion fields — **minor**
- [ ] `script/runner.rs:466`: `assert_no_console_errors.ignore_patterns`
  and `assert_network.url_contains` are cloned without running
  through `substitute`. Either substitute or document the limitation.

### E. Documentation hygiene

#### E1. Tick the iter-61 plan checkboxes that this iteration unblocks
- [ ] In `kb/iterations/iteration-61-script-runner-recorder.md`, leave
  the items this iteration closes as `- [ ]` if you prefer one-iteration-
  one-tick discipline, or add a footnote pointing here. Update the
  plan's `## Acceptance Criteria` section if AC #2 (record + replay
  round-trip) is now passable.

#### E2. Reference doc update
- [ ] `kb/reference/script-format.md`: add a "Recording" section
  describing the daemon-aware CLI-level recording flow and the
  recordable-vs-inspection split decided in A1.

## Acceptance Criteria

- [ ] `ff-rdp record start /tmp/session.json`, followed by
  `ff-rdp navigate <url>`, `ff-rdp click <sel>`, `ff-rdp type <sel>
  --text foo`, `ff-rdp record stop`, produces a JSON file that
  re-parses cleanly and contains exactly four steps in the right
  order — without anyone ever invoking `ff-rdp run --record`.
- [ ] `ff-rdp run /tmp/session.json` against the same fixture page
  replays to the same end state (this is iter-61 AC #2, now actually
  verifiable).
- [ ] A script with `{"navigate": {"urll": "..."}}` (typo) errors at
  parse time with a message naming the unknown field. (Currently it
  silently goes nowhere.)
- [ ] A script with `{"click": {"page_map": "x"}}` errors at dry-run
  time with the deferred-feature message; today it passes dry-run and
  errors at step 1 of execution.
- [ ] `ff-rdp run /dev/stdin --script-format json` works (stdin path
  without extension).
- [ ] Two concurrent CLI invocations against an active recording
  produce a valid JSON file with both steps recorded (file-locking
  works).
- [ ] All examples in `examples/scripts/` validate against
  `schemas/script.schema.json`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.

## Design Notes

- The `record_step_to_active` helper from iter-61 already exists with
  the right signature — this iteration just connects it to the
  dispatcher and adds the per-command `to_recorded_step()` mapper. No
  re-architecting needed.
- Refs in recorded scripts (A4) are subtle: a recorded `--ref` would
  appear to work in the same session but fail on replay days later.
  Resolving at record time is the safe default. The same logic will be
  relevant for iter-62 page-map references — record those as resolved
  selectors, not as `page_map:` paths, until the user opts in to
  page-map authoring.
- Why not daemon-side recording? Daemon-side (intercepting RPC calls
  in the daemon itself) is architecturally cleaner but requires
  extending the daemon protocol and doesn't help in `--no-daemon` mode.
  CLI-side recording covers the same use case with much less code and
  works regardless of daemon state. We can move to daemon-side later
  if there's a real need (e.g. recording per-tab recordings across
  parallel sessions).
- Out of scope: synthesising steps from observed Firefox events
  (human clicking in the browser window). That's "Playwright codegen"
  territory — sizable selector-inference subsystem, noisy event
  filtering, deserves its own iteration if pursued.
- The `deny_unknown_fields` change is technically a breaking change
  for any consumer who relies on extra fields being ignored. Given
  iter-61 just landed and v0.1.0 is still in progress, this is the
  right window.

## References

- [[iteration-61-script-runner-recorder]] — the predecessor; this
  iteration is its review-feedback closer.
- [[iteration-60-compact-responses-refs]] — daemon ref map; A4 uses
  it to resolve `--ref` at record time.
- [[iteration-62-page-map-index]] — references the same target-form
  validation; C1 makes the dry-run check consistent across both
  iterations.
- Playwright's codegen recorder (out of scope, but the reference for
  the human-interaction model):
  <https://playwright.dev/docs/codegen>
