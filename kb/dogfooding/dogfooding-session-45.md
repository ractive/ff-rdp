---
title: "Dogfooding Session 45 — iter-61/61b script runner + recorder on wardrobe-assistants"
type: dogfooding
date: 2026-05-16
status: completed
site: https://admin.wardrobe-assistants.ch
commands_tested: [launch, tabs, navigate, wait, type, click, page-text, dom, snapshot, a11y, screenshot, run, record, record-status, record-stop, eval]
tags:
  - dogfooding
  - script-runner
  - recorder
  - iter-61
  - iter-61b
  - ndjson-contamination
  - secret-leak
---

# Dogfooding Session 45

First session against the new iter-61 (`run`) and iter-61b (`record`) features.
Same site as [[dogfooding-session-44]] so regressions are easy to spot.
The new flow works end-to-end — record a manual login, replay it, get a clean
pass/fail summary — but the **NDJSON-contract violation flagged as F3 in the
iter-61b plan is still real**, and it actively leaks a typed password.

## What's New Since Last Session

- **iter-61** — `ff-rdp run <script.json|.yaml>` with verbs `navigate`, `click`,
  `type`, `wait`, `assert_text/url/no_console_errors/network`, `eval`,
  `screenshot`, `scroll`, `run`. Variable substitution (`{{vars.X}}`,
  `{{env.X}}`, `{{steps[N].X}}`), `--dry-run`, `--continue-on-failure`,
  `--vars`, `--env-file`, `--record`, `--show-secrets`.
- **iter-61b** — Recorder CLI wiring: `ff-rdp record start <out>` makes
  *every* subsequent recordable CLI command append to the file until
  `record stop`. Strict-schema parsing (`deny_unknown_fields`), dry-run
  validation of iter-62 deferred features (`page_map` / `field`),
  `--script-format` actually overriding extension detection, and a
  follow-up `chore` commit that landed many small fixes flagged during PR
  review.

## Regression Checks (vs [[dogfooding-session-44]])

| Command | S44 Status | S45 Status | Notes |
|---------|-----------|-----------|-------|
| `screenshot` in headless | "version unknown" refusal | **broken differently** — now `screenshotContentActor … noSuchActor` + the unhelpful "relaunch with: ff-rdp launch --headless" hint when *already* headless | New finding #1 |
| `click` on Radix dropdown | broken | not retested |
| `console --follow` daemon | silent | not retested |
| `eval` error → stdout | wrong stream | not retested |
| `--format text` + `--jq` | mutually exclusive | not retested |

## Script-Runner / Recorder Smoke Test

| Step | Result | Notes |
|------|--------|-------|
| `record start /tmp/wardrobe-session.json` | ✅ | status reports `active:true` |
| `navigate https://…` (recorded) | ✅ | step 1 in file |
| `wait --selector body --timeout 10000` (recorded) | ⚠️ | recorder dropped `--timeout` — file has `{"wait":{"selector":"body"}}` only |
| `screenshot` (recorded?) | ❌ | command failed; correctly *not* recorded |
| `type input[type=email] --text demo@example.com` (recorded) | ✅ | |
| `type input[type=password] --text wrong-password` (recorded) | ⚠️ | recorded with `"secret": false` — recorder doesn't auto-mark password selectors |
| `record stop` | ✅ | prints file path, file ends with `]\n}\n` |
| Generated JSON parses | ✅ | passes `--dry-run` |
| `run /tmp/wardrobe-session.json` (live replay) | ✅ | round-trip identical; summary `succeeded:4` |
| Richer hand-written flow (9 steps with `assert_text`, `assert_url`, `assert_no_console_errors`, `{{vars.email}}`) | ✅ | summary `executed:9 succeeded:8 failed:1` — the failing assertion caught **real CSP errors on the live site**, not a runner bug |

## Findings

### What Works Well

- **Strict-schema diagnostics (iter-61b B)** — typo'd verb produces
  `unknown variant \`navigatte\`, expected one of \`navigate\`, \`click\`, …`.
  Exactly the kind of error you want as an LLM author.
- **Dry-run rejection of iter-62 deferred features (iter-61b C)** —
  `{"click":{"page_map":"login.submit"}}` fails at `--dry-run` with
  `page_map and field target selectors require iter-62 page-map support
  (not yet implemented)`. Saves a real run.
- **`--script-format yaml` override (iter-61b D)** — YAML content in a
  `.json` file: without the flag, JSON parser barfs. With `--script-format
  yaml`, parses cleanly.
- **`--env-file` line-numbered errors (iter-61b F7)** — `--env-file
  /tmp/env` with a bad line 2 prints
  `error: --env-file '/tmp/env' line 2: expected KEY=VALUE, got: "BAD line
  no equals"`. No silent dropping.
- **Summary counts (iter-61b F6)** — `executed:9 succeeded:8 failed:1
  skipped:0` is correct. The old `passed=total-failed` bug is gone.
- **Variable substitution** — `{{vars.email}}` overridden via `--vars
  email=demo2@…` worked first try.
- **Recorder fail-safe** — a failing command (the bad `click --ref e1`)
  is *not* appended to the recording. File closes cleanly with empty
  `steps`.
- **Real-world assertion power** — `assert_no_console_errors` surfaced
  two legitimate CSP violations on `admin.wardrobe-assistants.ch`:
  ```
  Content-Security-Policy: Ignorieren von "'self'" innerhalb script-src:
    'strict-dynamic' angegeben
  Content-Security-Policy: Die Einstellungen der Seite haben die
    Ausführung eines JavaScript-Evals (script-src) blockiert …
  ```

### Issues Found

#### 1. NDJSON contamination — sub-command stdout pollutes the per-step stream — **major**

Listed as **F3** in the iter-61b plan but **not fixed** in the merged
iter-61b. Each script step internally calls the regular command function
which writes its full JSON envelope to stdout *before* the runner emits
the contractual NDJSON line. Measured ratio on a 9-step script:

```
$ ff-rdp run wardrobe-flow.json --continue-on-failure 2>&1 | wc -l
66
$ ff-rdp run wardrobe-flow.json --continue-on-failure 2>&1 | \
    awk '/^\{"(elapsed_ms|summary|executed)/' | wc -l
10
```

66 lines, only 10 are valid NDJSON — 85% noise. An LLM consuming `run`
output cannot reliably `jq -c` it or stream-parse it.

#### 2. Secret leak via contaminated output — **major** (security)

When `type` runs in a script, the runner emits:
```
{"elapsed_ms":97,"ok":true,"results":{"selector":"input[type=password]",
"typed":"[REDACTED]"},"step":6,"verb":"type"}
```
…but immediately *above* that, the sub-command leaks the raw value:
```
{
  "results": {
    "tag": "INPUT",
    "typed": true,
    "value": "wrong-password"        ← un-redacted
  },
  ...
}
```
This is the same root cause as #1 (sub-command stdout bleeds through),
but it elevates F3 from "ugly" to "security-relevant". Redaction is only
applied to the runner's own NDJSON line, not the helper output it
co-mingles with.

#### 3. `screenshot` misleading error in headless mode — **major**

Already in headless mode (launched with `ff-rdp launch --headless`):
```
$ ff-rdp screenshot -o /tmp/x.png
error: screenshot: screenshotContentActor capture failed (actor error
  from server1.conn3.child3/screenshotContentActor15: noSuchActor
  (unknownActor) — No such actor for ID: …) — screenshots require
  headless mode; relaunch with: ff-rdp launch --headless
```
Wrong on two counts: (a) the hint contradicts the actual launch flags,
(b) the underlying RDP actor was destroyed, which is the real error.
Same site, same launch flags, same regression seen in S44 but with a
different surface message.

#### 4. `page-text` jq path drift — **minor**

`ff-rdp page-text --jq '.text | .[0:300]'` errors with `jq runtime
error: cannot use null as rangeable`. The response field is now
`.results` (string), not `.text`. Likely fallout from iter-60's
compact-responses refactor. Update either docs or restore the alias.

#### 5. `default_timeout_ms` is in the docs but not in the schema — **minor**

`kb/reference/script-format.md` mentions per-step `timeout` (line 142)
but never defines a script-level `default_timeout_ms`, yet that's the
field name an LLM would guess. Setting it in a script now fails loudly
(`deny_unknown_fields`) — good — but the docs should either add the
field or call out that there's no global default-timeout knob.

#### 6. Recorder drops command-level flags — **minor**

`ff-rdp wait --selector body --timeout 10000` was recorded as
`{"wait":{"selector":"body"}}` — the `--timeout` is dropped, so replays
will use whatever default the runner picks. Recorder's
`to_recorded_step()` (iter-61b A1) isn't round-tripping all relevant
fields.

#### 7. Recorder doesn't auto-mark password selectors as `secret` — **minor**

A typed value into `input[type=password]` was recorded with
`"secret": false`. Replaying the script then logs the literal value
unless `--show-secrets=false` is the default *and* the recorder marks it.
Heuristic: any `type` step whose selector contains `password`,
`type=password`, or `name*=password` should default `secret: true` at
record time.

#### 8. Recorder's output JSON has a small formatting glitch — **cosmetic**

The last step line ends with `}  ]` (no newline, two spaces before the
closing `]`), making the file slightly ugly but still valid JSON.

### Feature Gaps

- **A script-level `default_timeout_ms`** would be a real ergonomic win
  for assert-heavy scripts (right now I'd have to write `"timeout":
  10000` on each `wait`/`assert_text`).
- **`record start --tag <label>`** so you can name a recording at start
  time. The frontmatter slot exists (`"name": null` in status) but
  there's no CLI way to set it.
- **`run --strict`** to fail the script if any sub-command emits
  unexpected stdout (would force #1 to be addressed and protect agent
  consumers).

## Summary

- 13 commands exercised (8 new in iter-61/61b)
- iter-61b's headline themes B, C, D all verified working end-to-end
  against a real site
- 8 issues found: 2 major (NDJSON contamination + secret leak — same
  root cause), 1 major regression (screenshot), 5 minor
- **Key takeaway**: the script-runner UX is *very* good for an LLM
  author — strict schemas, sharp dry-run errors, useful summary line —
  but the contaminated stdout (F3) needs to be fixed before any agent
  can reliably consume `ff-rdp run` output. Bundle it into iter-61c
  with the secret-leak fix, since they're the same root cause.

## References

- [[dogfooding-session-44]] — previous session, same site, pre-iter-61
- [[iteration-61-script-runner-recorder]] — runner core
- [[iteration-61b-recorder-cli-wiring]] — recorder + strict-schema +
  dry-run, section F captures findings not yet fixed
- [[iteration-62-page-map-index]] — deferred features that C1 now blocks
  cleanly at dry-run time
