---
title: "Script format reference"
type: reference
date: 2026-05-15
tags: [scripts, runner, recorder, format]
---

# ff-rdp script format (v1)

Iteration 61 introduced a replayable script format for ff-rdp.  Scripts
encode proven browser flows as declarative JSON (or YAML) so an LLM can
replay an entire login/interaction sequence in a single tool call instead
of 10–20 round-trips.

## Format overview

```json
{
  "$schema": "https://ff-rdp.dev/schemas/script/v1.json",
  "version": 1,
  "name": "Login flow",
  "vars": {
    "email": "user@example.com"
  },
  "steps": [
    { "navigate": { "url": "{{vars.fixture_url}}" } },
    { "type": { "selector": "#email", "text": "{{vars.email}}", "clear": true } },
    { "type": { "selector": "#password", "text": "{{vars.password}}", "clear": true, "secret": true } },
    { "click": { "selector": "#submit-btn", "wait_for_text": "Welcome" } },
    { "assert_text": { "selector": "h1", "contains": "Welcome" } },
    { "assert_url": { "matches": "dashboard" } }
  ]
}
```

**JSON is the default** — it validates cleanly against a JSON Schema, is
whitespace-insensitive, and is deterministic.  YAML is accepted as input
(same verb shapes, same semantics) but all emitted artefacts (recorder
output, examples) use JSON.

The JSON Schema lives at
`crates/ff-rdp-cli/schemas/script.schema.json` (draft-2020-12).

## Running scripts

The example scripts in `examples/scripts/` reference `{{vars.fixture_url}}`,
so pass it explicitly when copy-pasting:

```sh
ff-rdp run login.json --vars fixture_url=https://example.com
ff-rdp run login.yaml --vars fixture_url=https://example.com --vars email=me@example.com --vars password=secret
ff-rdp run login.json --vars fixture_url=https://example.com --dry-run              # validate without executing
ff-rdp run login.json --vars fixture_url=https://example.com --continue-on-failure  # don't stop at first failure
ff-rdp run login.json --vars fixture_url=https://example.com --show-secrets         # include secret values in output
ff-rdp run login.json --vars fixture_url=https://example.com --record session.json  # also write executed steps to file
```

Output is **NDJSON** — one JSON line per step:
```json
{"step":1,"verb":"navigate","ok":true,"results":{"navigated":"..."},"elapsed_ms":42}
{"step":2,"verb":"assert_text","ok":true,"results":{}}
{"summary":true,"ok":true,"executed":2,"succeeded":2,"failed":0,"skipped":0,"total_elapsed_ms":55}
```

## Recording

The recorder runs at the **CLI level** — no daemon changes required.
When a recording is active, every successful recordable CLI invocation
appends a step to the output file.  The file is a valid script that can
be replayed immediately with `ff-rdp run`.

```sh
ff-rdp record start session.json     # begin recording to session.json
ff-rdp navigate https://example.com  # appended as {"navigate": {...}}
ff-rdp click "button[type=submit]"   # appended as {"click": {...}}
ff-rdp type "#email" --text foo      # appended as {"type": {...}}
ff-rdp record stop                   # finalise → prints the file path
ff-rdp record status                 # check whether a recording is active
```

Recording state is stored in the XDG state directory
(`~/.local/state/ff-rdp/recording.json` on Linux/macOS).  Only one
active recording is permitted at a time; `record start` when already
recording is an error.

`ff-rdp run --record out.json <script.json>` is the alternative form:
it re-runs an existing script *and* writes the executed steps to
`out.json`.  Both forms produce the same output format.

### Ref resolution at record time

When a command uses `--ref e23` (an iter-60 runtime ref), the recorder
resolves the ref to its underlying CSS selector at the moment of
recording and emits `"selector": "<css>"` instead of `"ref": "e23"`.
Refs are session-scoped and expire; embedding a raw ref in a replay
script would silently break on the next session.

If the ref cannot be resolved (daemon not running, ref expired), the
step is skipped and a warning is printed to stderr.

### Recordable vs. inspection-only commands

Only commands that mutate browser state are recorded; read-only
inspection commands produce no step in the recording.

| Recordable | Inspection-only (not recorded) |
|-----------|-------------------------------|
| `navigate` | `tabs`, `dom`, `snapshot` |
| `click` | `console`, `network` |
| `type` | `page-text`, `cookies`, `storage` |
| `wait` | `sources`, `geometry`, `styles`, `computed` |
| `screenshot` | `responsive`, `a11y` |
| `eval` | `doctor`, `daemon *` |
| `scroll` | `launch`, `record *`, `inspect` |
| `reload` | |
| `back` | |
| `forward` | |

## Script-level options

| Field | Type | Description |
|-------|------|-------------|
| `version` | `1` | Must be `1` |
| `name` | string | Human-readable name |
| `base_url` | string | Prefix for relative `navigate` URLs |
| `vars` | object | Default variable values |
| `default_timeout_ms` | number | Default timeout (ms) applied to `wait`, `assert_text`, and `assert_network` steps that omit their own `timeout` field. Does **not** apply to per-action waits inside `click`/`type` (e.g. `wait_for_timeout_ms`). Falls back to the CLI `--timeout` value when this field is omitted. |
| `metadata` | object | Opaque metadata (ignored by runner) |
| `steps` | array | Steps to execute |

## Variable substitution

Syntax: `{{env.NAME}}`, `{{vars.NAME}}`, `{{steps[N].results.FIELD}}`

- `env.NAME` — reads an environment variable.  Resolution is **fail-closed**
  (iter-67): `NAME` must be one of `HOME`, `USER`, `LANG`, `LC_ALL`, `TZ`,
  or appear in `--allow-env <name1,name2,...>`.  Names matching the
  secret-name pattern (`*password*`, `*token*`, `*secret*`, `*key*`,
  `*passwd*`, `*pwd*`) are refused **unconditionally** — even an explicit
  allowlist entry will not unlock them; rename the variable or pass the
  value via `--vars`.  Output redaction for secret-shaped names continues
  to apply to `vars.*` / `--vars` / `--vars-file` values (legacy
  iter-61c behaviour); env interpolation for those names is blocked
  outright by the policy above and never reaches output.
- `vars.NAME` — reads from the script's `vars:` section or `--vars` /
  `--vars-file` overrides.  Secret-shaped names are auto-redacted.
- `steps[N].results.FIELD` — reads a field from step N's result object
  (N is 1-based).  E.g. `{{steps[1].results.url}}` reads the `url` field
  from the first step's result.

Variables matching `*password*`, `*token*`, `*secret*`, `*key*`, `*passwd*`,
`*pwd*` are redacted in step output unless `--show-secrets` is passed.
The `--vars-file PATH` flag loads a dotenv-style `KEY=VALUE` file; values go
to `{{vars.KEY}}` (not the process environment).  `--env-file` is a deprecated
alias for `--vars-file`.

`--dry-run` validates all variable references and reports missing ones
before executing anything.

## Verb reference

| Verb | Action | Key fields |
|------|--------|------------|
| `navigate` | Navigate to a URL | `url`, `wait_text`, `wait_selector` |
| `click` | Click a DOM element | target (see below), `wait_for_text`, `wait_for_selector` |
| `type` | Type text into an element | target, `text`, `clear`, `secret` |
| `wait` | Wait for a condition | `selector`, `text`, `eval`, `timeout` |
| `assert_text` | Assert element text | `selector`, `contains`\|`equals`, `not`, `timeout` |
| `assert_url` | Assert current URL | `matches`\|`equals` |
| `assert_no_console_errors` | Assert no JS errors | `ignore_patterns` |
| `assert_network` | Assert a network request | `url_contains`, `status`, `method` |
| `screenshot` | Capture a screenshot | `output`, `base64`, `full_page` |
| `eval` | Evaluate JavaScript | `script`, `stringify` |
| `run` | Execute a nested script | `path`, `with` (var overrides) |

## Element targeting

Steps that act on DOM elements accept exactly one of:

| Field | Meaning |
|-------|---------|
| `selector` | Raw CSS selector |
| `ref` | Iter-60 runtime ref ID (e.g. `"e23"`) |
| `page_map` | Page-map path (iter-62, deferred) |
| `field` | Page-map field shorthand (iter-62, deferred) |

Specifying more than one is a parser error.

`page_map` and `field` require iter-62 page-map support (not yet
implemented); they are accepted at parse time but produce a clear
"page-map support requires iter-62" error at runtime.  As of iter-61b,
`--dry-run` also rejects steps that use `page_map` or `field`, so the
error surfaces early without needing to connect to Firefox.  Full runtime
support (beyond the error) is deferred to iter-62.

## Nested scripts (`run:`)

```json
{ "run": { "path": "sub/login.json", "with": { "email": "{{vars.email}}" } } }
```

The runner detects cycles and errors out cleanly (no stack overflow).

**Depth cap (iter-67):** nested `run:` calls are capped at 16 levels
deep — a script that recurses past that bails with `run nesting depth 17
exceeds MAX_RUN_DEPTH=16`. The cap is intentionally above any realistic
legitimate nesting (top → suite → subtest → fixture-setup → action).

**Path containment (iter-67):** by default, sub-script paths must be
relative and stay within the top-level script's directory. Absolute
paths and `..`-traversing relative paths are refused. Pass
`--allow-unsafe-script-paths` only when you author every file in the
include chain — e.g. when developing a shared lib under
`~/scripts/lib/`. The flag opens the runner to reading any file the
process can `open(2)`, so do not enable it for scripts received from
untrusted sources.

## Assertions

- `assert_text`: polls (with timeout) until the condition is met, using
  iter-59 auto-wait semantics.  On failure the step's NDJSON line carries a
  structured `diagnostics` object (since iter-61c) of the form
  `{"actual_text": "<observed>"}` — earlier versions emitted a plain string.
  Respects `default_timeout_ms` if no step-level `timeout` is set.
- `assert_url`: fetches `window.location.href` and checks against
  `matches` (regex) or `equals` (exact string).
- `assert_no_console_errors`: checks the console buffer for error-level
  messages; filterable via `ignore_patterns`.
- `assert_network`: scans buffered network events for a matching entry.
  On failure the `diagnostics` object is `{"events_in_buffer": <N>}`
  (since iter-61c).  Respects `default_timeout_ms` if no step-level
  `timeout` is set.

## Password-shaped selectors

When recording a `type` step into a selector that contains `password` or
`passwd` (case-insensitive), the recorder automatically sets `"secret": true`
on the recorded step.  This prevents the typed text from appearing in replay
output.

Override with `"secret": false` in the script if you are intentionally typing
a non-secret value into a password-shaped field.

## `page-text` output

`ff-rdp page-text` emits:

```json
{"results": "<full page text>", "text": "<same>", "total": 1}
```

Both `.results` and `.text` are aliases for the same value.  Use
`--jq '.results'` or `--jq '.text'` — both work.

## Recipes

### Login and assert

```json
{
  "$schema": "https://ff-rdp.dev/schemas/script/v1.json",
  "version": 1,
  "name": "Login and assert dashboard",
  "default_timeout_ms": 10000,
  "vars": {"url": "https://app.example.com", "email": "user@example.com"},
  "steps": [
    {"navigate": {"url": "{{vars.url}}/login"}},
    {"type": {"selector": "input[name=email]", "text": "{{vars.email}}", "clear": true}},
    {"type": {"selector": "input[type=password]", "text": "{{vars.password}}", "clear": true}},
    {"click": {"selector": "button[type=submit]", "wait_for_text": "Dashboard"}},
    {"assert_text": {"selector": "h1", "contains": "Dashboard"}},
    {"assert_url": {"matches": "/dashboard"}}
  ]
}
```

Run: `ff-rdp run login.json --vars password=secret`

### Navigate and screenshot

```json
{
  "version": 1,
  "steps": [
    {"navigate": {"url": "https://example.com"}},
    {"wait": {"selector": "body", "timeout": 3000}},
    {"screenshot": {"output": "screenshot.png"}}
  ]
}
```

Run: `ff-rdp run screenshot.json`

### Record then replay

```sh
ff-rdp record start session.json
ff-rdp navigate https://app.example.com/login
ff-rdp type "input[name=email]" --text user@example.com --clear
ff-rdp type "input[type=password]" --text secret --clear
ff-rdp click "button[type=submit]"
ff-rdp record stop
# Replay:
ff-rdp run session.json --vars password=secret
```

### Secret from env

```json
{
  "version": 1,
  "steps": [
    {"navigate": {"url": "{{vars.url}}"}},
    {"type": {"selector": "input[type=password]", "text": "{{env.APP_PASSWORD}}", "clear": true}}
  ]
}
```

Run: `APP_PASSWORD=secret ff-rdp run flow.json --vars url=https://app.example.com`

The `{{env.APP_PASSWORD}}` value is automatically redacted from step output
because `APP_PASSWORD` matches the `*password*` pattern.

## Examples

- `examples/scripts/login.json` — JSON canonical example
- `examples/scripts/login.yaml` — same flow in YAML
- `crates/ff-rdp-cli/tests/fixtures/script_fixture.html` — local HTML
  fixture used by e2e tests
