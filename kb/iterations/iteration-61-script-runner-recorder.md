---
title: "Iteration 61: Script runner and session recorder"
type: iteration
date: 2026-05-15
status: completed
branch: iter-61/script-runner-recorder
depends_on:
  - iteration-60-compact-responses-refs
tags:
  - iteration
  - scripts
  - recorder
  - e2e
  - agent-speed
  - json
  - replay
---

# Iteration 61: Script runner and session recorder

The qualitative jump for agent speed: turn proven flows into replayable
scripts so the LLM doesn't re-derive them on every run. Today an agent
spends 10–20 turns discovering how to log in, click around, and verify
something. With this iteration, the second run of that flow is one tool
call: `ff-rdp run login.yaml`.

The runner and the recorder are paired deliberately. A recorder that
emits a format the runner can't replay is useless; a runner without a
recorder relies on humans to hand-write YAML. They share a format,
designed once.

Scripts are written by LLMs, not humans. That changes the format decision:
**JSON is the default**, not YAML. YAML's token savings for a 20-step script
are ~100–200 tokens — trivial next to one LLM round-trip — and its
write-brittleness (indentation drift, the Norway problem, ambiguous
unquoted strings, multiline-string escaping) produces silent semantic
errors. JSON is whitespace-insensitive, deterministic, and validates
cleanly against a JSON Schema.

YAML is accepted as input too — for free, since any sane YAML parser is a
superset of JSON — but emitted artefacts (recorder output, examples,
documentation) use JSON.

The verb vocabulary is **Maestro-flavoured** (one verb per step,
declarative, flat) — readable by both LLMs and humans, no JS sandbox to
maintain.

Themes:

- **A — Script format.** JSON (default) / YAML (accepted), small verb
  set mapping 1:1 to existing ff-rdp subcommands plus a small assertion
  vocabulary.
- **B — Runner.** `ff-rdp run <script.json>` executes steps sequentially,
  emits one JSON result per step, supports variables and step-output
  references.
- **C — Recorder.** A live ff-rdp session can be recorded; commands
  executed against the same daemon get appended to a JSON file. Output
  is replayable as-is.
- **D — Assertions.** Lightweight assertion vocabulary so scripts can
  encode "did this actually work?" without requiring an external test
  harness.

## Tasks

### A. Script format

#### A1. Document the format
- [ ] Add `kb/reference/script-format.md` defining the JSON schema. Top
  level: `$schema: "https://ff-rdp.dev/schemas/script/v1.json"`
  (required, literal discriminator — not fetched), `version: 1`,
  `name`, `base_url`, `page_map` (optional path to a page-map),
  `vars`, `steps`. Each step is a single-key object whose key is the
  verb (`navigate`, `click`, `type`, `wait`, `assert_text`,
  `assert_url`, `assert_no_console_errors`, `assert_network`,
  `screenshot`, `eval`, `run` for nesting). Reserve optional
  `metadata`.
- [ ] Ship the schema as a real JSON Schema (`draft-2020-12`) at
  `crates/ff-rdp-cli/schemas/script.schema.json`. Both runner and
  recorder validate against it; the parser rejects unknown keys (no
  silent typo drift).
- [ ] Accept `.yaml`/`.yml` files too — parse via `serde_yaml` and
  immediately normalise to the same in-memory representation. No
  semantic difference between formats.

#### A1b. Target-selection discipline (selector / ref / page_map / field)

Every step that targets a DOM element accepts **at most one** of the
four targeting fields. The script parser errors on >1 with a clear
"specify exactly one of: selector, ref, page_map, field" message.

- [ ] `selector: '<css>'` — raw CSS selector (current behaviour).
- [ ] `ref: 'e23'` — iter-60 runtime ref ID; only valid during one
  session, errors if the daemon's ref map has expired.
- [ ] `page_map: '<dotted.path>'` — resolves through the loaded
  page-map (iter-62) to a stable selector.
- [ ] `field: 'pages.<page>.forms.<form>.<field>'` — shorthand for the
  common case "type into the `email` field of the login form."
  Resolves to the page-map field's `selector`. Auto-fills `type:
  password` fields when used with the `type` verb without echoing the
  value to logs.

#### A2. Define variable substitution
- [ ] Substitution syntax: `{{env.NAME}}`, `{{vars.NAME}}`,
  `{{steps[N].results.X}}` (N is 1-based step index). Resolved at step
  execution time, not parse time, so step-output refs work.
- [ ] `--vars k=v` CLI flag passes ad-hoc variables. `--env-file <path>`
  loads a dotenv-style file (no shell expansion).
- [ ] Secrets discipline: variables matching `*password*`, `*token*`,
  `*secret*` are redacted in step-result output unless `--show-secrets`.

#### A3. Minimal example fixture
- [ ] Ship `examples/scripts/login.json` as the canonical example,
  driving against a local fixture page. Used by the e2e test below.
  Also ship `examples/scripts/login.yaml` as a one-to-one mirror so
  consumers can see both shapes; both must produce identical step
  outcomes (asserted by a test).

### B. Runner

#### B1. `ff-rdp run <script.json>` subcommand
- [ ] New module under `crates/ff-rdp-cli/src/commands/run.rs`. JSON
  parser via `serde_json` (already in workspace); YAML accepted via
  `serde_yaml` behind the same `serde::Deserialize` impl. Dispatch by
  file extension, with a `--format json|yaml` override for stdin / odd
  extensions.
- [ ] Each step dispatches to the same in-process functions the CLI
  commands use today — no subprocess fork. (If a command currently only
  has a CLI entry, refactor it to expose a callable function.)
- [ ] Streaming output: one JSON line per step to stdout (NDJSON), with
  `{step: N, verb, ok, results, elapsed_ms}`. Final summary line:
  `{summary: true, ok, total, failed, total_elapsed_ms}`.

#### B2. Run modes
- [ ] `--bail` (default): stop on first failed step. Exit non-zero.
- [ ] `--continue-on-failure`: run all steps, exit code is 0 if all pass,
  else 1 with summary count.
- [ ] `--dry-run`: parse and validate the script; resolve variables;
  print the resolved step list without executing.

#### B3. Nested `run` verb
- [ ] A step `- run: path/to/sub.yaml` recursively executes another
  script. Detect cycles via the resolution stack. Variables inherit
  unless overridden with `with:`.

### C. Recorder

#### C1. `ff-rdp record start` / `record stop`
- [ ] Daemon-aware: when recording is active, every CLI command that
  goes through the daemon is observed and serialised into the active
  file. Default output extension: `.json`; `--format yaml` switches to
  YAML emission. State stored in the daemon (path of target file, last
  step index, chosen format).
- [ ] On `record stop`, the daemon flushes and prints the final file
  path. Concurrent recordings: error, only one active recording per
  daemon at a time.

#### C2. Record-while-replaying
- [ ] `ff-rdp run --record <out.json> <existing.json>` runs the existing
  script *and* records any additional commands issued in parallel (rare
  but useful for incremental flow extension).

#### C3. Round-trip fidelity
- [ ] Every command shape in the CLI must have a 1:1 mapping into the
  recorder's emitted artefact. Add a single source-of-truth table in
  code (a `RecordableCommand` enum or trait) used by both the runner's
  dispatch and the recorder's serialiser.
- [ ] e2e: record a session of "navigate + 3 clicks + 1 assert", stop,
  replay the resulting file — same end state. Test both JSON-out and
  YAML-out paths.

### D. Assertion vocabulary

#### D1. `assert_text`
- [ ] `assert_text: { selector: '<css>', contains: '<substr>' }`. Optional
  `equals` instead of `contains`. Optional `not: true`.
- [ ] Inherits auto-wait from iter-59 (so `assert_text` polls until
  match or timeout).

#### D2. `assert_url`
- [ ] `assert_url: { matches: '<regex>' }` or `equals: '<exact>'`.
  Polls during the iter-59 settle window.

#### D3. `assert_no_console_errors`
- [ ] Asserts the console buffer (level=error, scoped to since the last
  navigation) is empty. Filterable via `ignore_patterns: ["…"]`. Depends
  on a working console buffer — which means this is also a
  forcing-function for the daemon console-stream regression flagged in
  session 44 (Issue #1) to actually be fixed.

#### D4. `assert_network`
- [ ] `assert_network: { url_contains: '…', status: 200, method: POST,
  appeared_after: step_id }`. Asserts at least one matching event in the
  network buffer.
- [ ] Also accepts `api_route: '<named.ref>'` — resolves through the
  loaded page-map's `api_routes[]` (iter-62) to `{method, path}`.
  Same target-selection discipline as A1b: `api_route` and
  `url_contains`/`method` are mutually exclusive.

#### D5. Failure output
- [ ] Failing assertions include a `diagnostics` field with the
  observed state (actual URL, actual text, list of console errors) so
  the agent — or a human — can read the failure and decide next steps
  without rerunning anything.

## Acceptance Criteria

- [ ] `ff-rdp run examples/scripts/login.json --vars
  email=$E password=$P` logs into the fixture site and verifies the
  dashboard heading in **one tool call**, completing in well under 10 s.
  Equivalent YAML script produces identical step outcomes.
- [ ] The same flow, recorded via `ff-rdp record start session.json`
  followed by manual CLI commands then `record stop`, produces a script
  that replays to the same end state.
- [ ] `--dry-run` on a script with `{{vars.X}}` and missing `X` errors
  out before executing any step, naming the missing variable.
- [ ] Nested `run:` works with cycle detection — a self-referencing
  script errors out cleanly, not via stack overflow.
- [ ] Failing assertion exits non-zero, prints diagnostics, and (in
  `--bail` mode) does not execute later steps.
- [ ] Secrets in `--vars password=…` are not visible in the JSON output
  of step results unless `--show-secrets` is set.
- [ ] Records the `ff-rdp-debug` Tier 1 playbooks (from iter-58) as
  scripts — proves the format is expressive enough for real existing
  flows.
- [ ] A script using `field: pages.login.forms.login_form.email` and
  `api_route: auth.sign_in_email` resolves both correctly against a
  loaded page-map (iter-62). Missing or misnamed references fail at
  `--dry-run` time, not at execution time.
- [ ] A step that specifies more than one of `{selector, ref, page_map,
  field}` is rejected by the parser with a precise error.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.

## Design Notes

- One verb per step, one tool call per replay — that's the contract that
  makes agent flows fast. Resist the temptation to add control-flow
  (conditionals, loops) until there's evidence it's needed; an agent
  that needs branching can call `ff-rdp run` twice with different
  scripts.
- Recorder design intentionally mirrors the runner: same verb set,
  symmetric serialisation. Don't introduce a separate "trace" format.
- `assert_no_console_errors` is the integration point that *forces* the
  daemon console-stream regression from session 44 to be fixed (if it
  isn't fixed by then). Worth flagging in the iteration kick-off: if
  iter-59 didn't cover it, add a sub-task here.
- Maestro's format
  (<https://maestro.mobile.dev/api-reference/commands>) is the cleanest
  existing reference for "declarative commands an LLM can write." Borrow
  its verb shape and step-list structure, but render in JSON by default
  — Maestro's YAML brittleness (indentation, the Norway problem,
  ambiguous unquoted strings, multiline-string escaping) is exactly
  what we're avoiding for LLM authorship.
- Rationale for JSON-default: a 20-step script is ~100–200 tokens
  shorter in YAML. Trivial next to a single LLM round-trip. The
  determinism and schema-validation wins are much larger.

## References

- Maestro YAML reference:
  <https://maestro.mobile.dev/api-reference/commands>
- Playwright `_snapshotForAI`:
  <https://playwright.dev/docs/aria-snapshots>
- [[dogfooding/dogfooding-session-44]] — the "30+ s of LLM thinking per
  login flow" observation that motivates the runner.
- [[iteration-59-autowait-pointer-retry]] — runner relies on auto-wait
  being baked into primitives so scripts don't need explicit `wait`s.
- [[iteration-60-compact-responses-refs]] — runner step outputs use the
  trimmed envelope; recorded scripts reference elements by `ref:` when
  emitted from a `snapshot`-driven session.
