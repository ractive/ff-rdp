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

```sh
ff-rdp run login.json
ff-rdp run login.yaml --vars email=me@example.com --vars password=secret
ff-rdp run login.json --dry-run              # validate without executing
ff-rdp run login.json --continue-on-failure  # don't stop at first failure
ff-rdp run login.json --show-secrets         # include secret values in output
ff-rdp run login.json --record session.json  # also write executed steps to file
```

Output is **NDJSON** — one JSON line per step:
```
{"step":1,"verb":"navigate","ok":true,"results":{"navigated":"..."},"elapsed_ms":42}
{"step":2,"verb":"assert_text","ok":true,...}
{"summary":true,"ok":true,"total":2,"failed":0,"passed":2,"total_elapsed_ms":55}
```

## Recording

```sh
ff-rdp record start session.json     # begin recording
ff-rdp navigate https://example.com  # ...run commands...
ff-rdp click "button[type=submit]"
ff-rdp record stop                   # finalise → prints the file path
ff-rdp record status                 # check if recording is active
```

The recording file is a valid script that can be replayed with `ff-rdp run`.

## Variable substitution

Syntax: `{{env.NAME}}`, `{{vars.NAME}}`, `{{steps[N].key}}`

- `env.NAME` — reads an environment variable.
- `vars.NAME` — reads from the script's `vars:` section or `--vars` flags.
- `steps[N].key` — reads a field from step N's result (1-based).

Variables matching `*password*`, `*token*`, `*secret*` are redacted in
step output unless `--show-secrets` is passed.

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
"page-map support requires iter-62" error at runtime.

## Nested scripts (`run:`)

```json
{ "run": { "path": "sub/login.json", "with": { "email": "{{vars.email}}" } } }
```

The runner detects cycles and errors out cleanly (no stack overflow).

## Assertions

- `assert_text`: polls (with timeout) until the condition is met, using
  iter-59 auto-wait semantics.  On failure, `diagnostics` field contains
  the actual observed text.
- `assert_url`: fetches `window.location.href` and checks against
  `matches` (regex) or `equals` (exact string).
- `assert_no_console_errors`: checks the console buffer for error-level
  messages; filterable via `ignore_patterns`.
- `assert_network`: scans buffered network events for a matching entry.

## Examples

- `examples/scripts/login.json` — JSON canonical example
- `examples/scripts/login.yaml` — same flow in YAML
- `crates/ff-rdp-cli/tests/fixtures/script_fixture.html` — local HTML
  fixture used by e2e tests
