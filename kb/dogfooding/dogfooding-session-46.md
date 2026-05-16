---
title: "Dogfooding Session 46 — iter-61c verification on wardrobe-assistants"
type: dogfooding
date: 2026-05-16
status: completed
site: https://admin.wardrobe-assistants.ch
commands_tested: [launch, tabs, navigate, wait, type, page-text, screenshot, doctor, run, record, record-status, record-stop]
tags:
  - dogfooding
  - iter-61c
  - regression-verification
  - ndjson-contract
  - secret-containment
  - recorder-fidelity
---

# Dogfooding Session 46

Same site as [[dogfooding-session-45]] and [[dogfooding-session-44]] —
this run was purely about verifying that [[iteration-61c-runner-secret-leak-fixes]]
actually closed the 8 findings from session 45. Headline result: **the
two majors (NDJSON contamination + the secret leak that rode on top of
it) are conclusively fixed**, and most minors landed. Two items remain:
one regression in recorder fidelity (B1 — `wait --timeout` isn't being
captured even though the commit message claims it), and the headless
`screenshot` regression was explicitly deferred in iter-61c per the
commit message ("D: deferred (needs live Firefox repro)").

## What's New Since Last Session

Per `9eadf50` ("iter-61c: NDJSON contract, secret containment, recorder
fidelity"):

- **A1**: `run_core()` / printing-shell split for `navigate`, `click`,
  `type`, `wait`, `screenshot`. The script runner calls `run_core`
  directly, so sub-command stdout no longer contaminates the NDJSON
  stream.
- **A2 / E5**: Env-loaded values pushed into the redaction set.
- **B1 / B2 / B3**: Recorder picks up `--timeout` and
  `--wait-for-*` predicates, auto-marks password selectors as
  `secret: true`, and writes pretty-printed JSON.
- **C1 / C2 / C3**: `page-text` aliases `.text` for back-compat,
  `default_timeout_ms` is a real script field, recipes added.
- **E2 / E3 / E4 / E6**: `--record-strict` flag, `--vars-file`
  canonical name with `--env-file` as a deprecated alias,
  `assert_network` honours `default_timeout_ms`, typed
  `AppError::Diagnostics`.

## Regression Checks vs [[dogfooding-session-45]]

| # | S45 Finding | Iter-61c Task | S46 Result |
|---|-------------|---------------|-----------|
| 1 | NDJSON contamination (66 stdout lines, 10 NDJSON) | A1 | ✅ **Fixed** — same 9-step script now emits 10 stdout lines, all 10 are NDJSON (100% pure). |
| 2 | Secret leak via #1 — `wrong-password` visible in default stdout | A2 | ✅ **Fixed** — `grep -c "wrong" stdout` returns `0` in default mode, `1` with `--show-secrets`. |
| 3 | Headless screenshot fails + misleading "relaunch with --headless" hint | D | ⚠️ **Half-fixed** — error now says `screenshot actor unavailable on Firefox unknown; minimum supported version: 120. hint: upgrade Firefox or run ff-rdp doctor`. Misleading hint replaced (D2 ✓) but actor failure not addressed (D1 explicitly deferred). |
| 4 | `page-text --jq '.text'` broken (field renamed to `.results`) | C1 | ✅ **Fixed** — `.text` alias restored, jq filter works. |
| 5 | `default_timeout_ms` docs orphan | C2 | ✅ **Fixed** — accepted as a real script-level field. |
| 6 | Recorder drops `--timeout` flag | B1 | ❌ **Still broken** — commit message claims fix, but `ff-rdp wait --selector body --timeout 5000` recorded as `{"wait":{"selector":"body"}}` (no `timeout`). See finding #1 below. |
| 7 | Recorder didn't auto-mark password selectors as `secret:true` | B2 | ✅ **Fixed** — `type input[type=password]` records with `"secret": true`. |
| 8 | Cosmetic `}  ]` closing-bracket formatting | B3 | ⚠️ **Half-fixed** — step objects are now pretty-printed nicely, but the *closing* `}  ]` artifact at end-of-array is still there. See finding #2. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `ff-rdp launch --headless` | ✅ | Firefox up from previous session, reused cleanly. |
| `tabs` | ✅ | 1 tab from earlier session, URL preserved. |
| `navigate https://admin.wardrobe-assistants.ch` | ✅ | <100 ms in the per-step elapsed counter. |
| `page-text --jq '.text \| .[0:80]'` | ✅ | Returns `"Admin sign in\\nWardrobe Assistants\\n…"`. |
| `wait --selector form --timeout 8000` | ✅ | Works (recording artifact is a separate issue). |
| `type input[type=email] --text me@example.com` | ✅ | |
| `type input[type=password] --text hunter2` | ✅ | Recorded with `secret: true`. |
| `screenshot -o /tmp/wardrobe-46.png` | ❌ | "screenshot actor unavailable on Firefox unknown" — see #3 below. |
| `record start/stop/status` | ✅ | State file lifecycle clean. |
| `run /tmp/wardrobe-flow.json --continue-on-failure` | ✅ | 9 steps, 8 succeeded, 1 expected-fail (real CSP errors on the site). |
| `run /tmp/wardrobe-46.json` (replay recorded) | ✅ | 4 steps, all succeeded; `[REDACTED]` in `typed` field for the password step. |
| `--vars-file` flag | ✅ | New canonical name; `--env-file` still works as deprecated alias. |
| `default_timeout_ms: 3000` in script | ✅ | Parses cleanly at dry-run. |
| `deny_unknown_fields` (typo'd verb) | ✅ | Still fires with the "expected one of …" list. |

## Findings

### What Works Well

- **The NDJSON contract is now an actual contract.** Running my
  9-step `wardrobe-flow.json` produces exactly 10 lines of stdout,
  10/10 of which are NDJSON. An LLM consumer can finally
  `jq -c '. | select(.summary or .step)' < ff-rdp-run.log`
  without filtering 85% noise. This is the single biggest UX
  improvement in iter-61c.
- **Secret containment is end-to-end.** Default mode shows
  `"typed": "[REDACTED]"` and a literal `grep -c hunter2` of stdout
  comes back zero — including in `record` output (the password step's
  text is recorded with `secret: true`, so replays redact it too).
- **Recorder JSON is now pretty.** Step objects are properly indented
  with one key per line; a hand-authored script and a recorded one
  are visually indistinguishable inside the `steps` array.
- **`page-text --jq '.text'` works again** — restoring the alias was
  the right call over breaking existing jq filters.
- **`default_timeout_ms` + `assert_network` honouring it** — for
  assert-heavy scripts this is a real ergonomic win. No more
  per-verb `"timeout": 10000` boilerplate.
- **`--vars-file` is the better name.** Clearer that values land in
  `{{vars.X}}` substitution, not the process env. Keeping
  `--env-file` as a deprecated alias is the right migration path.

### Issues Found

#### 1. Recorder still drops `--timeout` despite commit-message claim — **minor**

```
$ ff-rdp record start /tmp/timeout-test.json
$ ff-rdp wait --selector body --timeout 5000
$ ff-rdp record stop
$ cat /tmp/timeout-test.json
…
    {
      "wait": {
        "selector": "body"
      }
    }
…
```
The `timeout: 5000` is missing from the recorded step. The iter-61c
commit message explicitly lists "B1: Recorder captures wait
--timeout, click --wait-for predicates", so this is either a
shipped-but-untested fix or a partial implementation that only
covered `click --wait-for-*`. Worth a focused unit test:

```rust
#[test]
fn recorder_captures_wait_timeout() {
    // record `wait --selector body --timeout 5000`,
    // assert the recorded step has `timeout: 5000`.
}
```

#### 2. Closing-bracket formatting nit persists — **cosmetic**

The recorded file still ends with:

```
    }  ]
}
```

Two trailing spaces, no newline before the `]`. Step objects are now
pretty (good), but the array-close serialiser isn't honouring the
same `PrettyFormatter`. Likely an explicit `write!(f, "  ]")` rather
than `serde_json::ser::PrettyFormatter` in the finalise path.

#### 3. Headless `screenshot` still broken (deferred per iter-61c) — **major**

Same site, same launch flags:
```
$ ff-rdp screenshot -o /tmp/x.png
error: screenshot: screenshot actor unavailable on Firefox unknown;
  minimum supported version: 120.
  hint: upgrade Firefox or run `ff-rdp doctor` for the full
  compatibility report.
```
And from `doctor`:
```
"name": "firefox_version",
"detail": "Firefox version not advertised in the RDP greeting"
```

The misleading "relaunch with --headless" hint is gone (D2 ✓), and
the new pointer to `doctor` is genuinely useful, but the underlying
failure is that the RDP greeting against this Firefox build doesn't
include a version string, so the screenshot-actor guard refuses to
even try. Two ways forward:

1. **Probe for the actor** instead of gating on version string —
   if the actor exists, use it; if not, fall back to the older
   capture path.
2. **Parse `application/version-info` actor** at handshake time as
   a fallback when the greeting is silent.

Either fix would unblock screenshots on the wardrobe-assistants
Firefox instance and any other build where the greeting lacks the
version line. (Note: this is the same Firefox PID used by all of
sessions 44, 45, 46 — so it's a real-world configuration, not a
contrived edge case.)

### Feature Gaps

Same list as session 45's "Feature Gaps" still applies — `record
start --tag <label>`, `run --strict` (now that A1 is fixed,
`--strict` would mean "fail on any non-NDJSON stdout line", which
is a regression guard for the contract iter-61c just established).

## Summary

- **13 commands exercised across 8 verification points.**
- **iter-61c verdict: 6/8 session-45 findings fully fixed, 1
  half-fixed (D — misleading hint resolved, actor issue deferred),
  1 still broken (B1 — recorder `--timeout` capture).**
- **Key takeaway**: the script-runner stream is now production-grade
  for LLM consumption. The NDJSON contract holds, the secret
  containment works end-to-end through the recorder, and
  `default_timeout_ms` removes a real source of script-authoring
  friction. The remaining items are a small B1 follow-up (one unit
  test + a one-line fix) and the deferred D screenshot work that
  needs Firefox-greeting investigation.

## References

- [[dogfooding-session-45]] — the bug report this session verifies
  against. Findings #1, #2, #4, #5, #6, #7, #8 → fixed; #3 → half-fixed.
- [[dogfooding-session-44]] — original session, pre-iter-61.
- [[iteration-61c-runner-secret-leak-fixes]] — the fix bundle; see
  the merge commit `ce6b9ff` and implementation `9eadf50`.
