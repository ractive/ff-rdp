---
title: Dogfooding Session 49 — post-iter-61h verification + iter-60 ref resolution bug
type: dogfooding
date: 2026-05-22
status: completed
site: https://news.ycombinator.com + https://example.com
commands_tested: [doctor, launch, navigate, tabs, page-text, perf, screenshot, dom, click, scroll, eval, computed, record, run, daemon]
tags:
  - dogfooding
  - iter-61h
  - iter-60-refs
  - regression-verification
  - same-url-navigate
  - csp
---

# Dogfooding Session 49

First post-merge run after iter-61h landed (PR #73 merged at `b61f739`).
Confirms the version-warning removal works (sessions 47/48 issue #8 →
✅ silent) and the rebuilt doctor probe now reports
"Firefox 151 (newer than tested range 120–150, but supported)" instead of
warning. Beyond that — **most of session 48's bug list is unfixed** and
the iter-60 stable-ref feature turns out to be fundamentally broken at
resolve time.

Previous: [[dogfooding-session-48]] · [[iteration-61-script-runner-recorder]]

## TL;DR

- ✅ Version warning silenced. `ff-rdp doctor` shows version informationally.
- ✅ `launch --headless` no longer prints the false "exited immediately" stderr.
- ✅ Recorder workflow continues to work on the happy path.
- ❌ **iter-60 stable refs are broken**: ref resolver passes the stored JS
  expression (`document.querySelectorAll('tr.athing')[0]`) to
  `Document.querySelector` as if it were a CSS selector. Every `--ref e1`
  click against a ref minted from a multi-match `dom` query fails with
  "is not a valid selector".
- ❌ Re-navigate to current URL still times out (session 47 #1, 48 #1, again).
- ❌ `screenshot --full-page` still produces a viewport-only PNG.
- ❌ `dom` still returns polymorphic shape (object for 1 match, array for >1).
- ❌ `computed --prop` still single-valued **and** rejects `--p`-prefixed
  names like `--bg-color` as ambiguous flags.
- ⚠ `eval --stringify --format text` still prints a tip line below the
  value (`--no-hints` works as workaround). Should auto-suppress hints
  whenever `--stringify` is on.
- ⚠ `eval` is unconditionally blocked by CSP on strict sites (HN, Wikipedia,
  the admin Wardrobe app) — no privileged-realm bypass. Feature gap, not
  a regression.

## Regression Checks

| Session 47/48 issue | Status here | Notes |
|---|---|---|
| Re-navigate to current URL | ❌ STILL BROKEN | reproduces on HN and example.com; bites the recorder too (failed navigate → empty `steps[]`) |
| `--full-page` viewport-only | ❌ STILL BROKEN | both regular and full-page produce 1366×683 PNG on HN |
| `dom` polymorphic shape | ❌ STILL BROKEN | `a.morelink` (1) returns object; `tr.athing` (30) returns array |
| `computed --prop` repeatable | ❌ STILL BROKEN | `--prop color --prop font-size` errors |
| `computed --prop "--bg-color"` | ❌ STILL BROKEN | clap parses `--bg-color` as a flag (user-reported) |
| Per-command Firefox version warning | ✅ FIXED | nothing on stderr, doctor reports informationally |
| `launch --headless` false stderr error | ✅ FIXED | stderr clean on cold-start |
| `eval --stringify --format text` hint suffix | ⚠ unchanged | `--no-hints` works; should be implicit under `--stringify` |
| Iter-60 refs registered for `dom`/`snapshot` | ❌ NEW BUG | refs ARE minted for multi-match `dom`, but unusable — see Finding #1 below |

## Findings

### 1. iter-60 ref resolution is broken — **major / new**

`dom <selector>` on HN's `tr.athing` mints 30 refs (e1..e30). Each
result entry carries a `ref` field. Pull the first one and try to use
it:

```sh
$ ff-rdp dom 'tr.athing' --jq '.results[0].ref'
"e1"

$ ff-rdp click --ref e1
error: selector 'document.querySelectorAll('tr.athing')[0]' not ready
       after 5000ms: Document.querySelector:
       'document.querySelectorAll('tr.athing')[0]' is not a valid selector
```

The daemon is storing the ref as a JavaScript expression
(`document.querySelectorAll(...)[i]`) but the resolver hands that string
to `Document.querySelector` verbatim — which treats it as a CSS selector
and rejects it. Refs are unusable for any element that was selected via
`querySelectorAll[i]`, which is essentially every multi-match path.

This is the iter-60 acceptance criterion #3 ("an agent flow can:
`snapshot` → pick a ref → `click --ref e23` → no intermediate
'find the right selector' calls") and it does not work. Session 48
noticed `refs_registered: false` and concluded the feature was wired
but unused; session 49 shows it's actually worse — refs are minted but
resolve to garbage.

Fix sketch: the daemon should store refs as one of (a) a stable CSS
locator inferred from the element (Playwright-style locator
generation), (b) the raw JS expression but resolved with
`Function('return ' + expr)()` instead of `querySelector`, or (c) a
stable `data-ff-rdp-ref` attribute pinned to the DOM node.

### 2. Re-navigate to current URL still times out — **major (regression)**

Same as session 47 #1 and session 48 #1. Reproduces on every site
tested. New twist: it bites the recorder too — `record start` →
`navigate <currentUrl>` → `record stop` produces a file with empty
`steps: []` because the recorder skips the failed navigate.

```sh
$ ff-rdp record start /tmp/r.json
$ ff-rdp navigate https://example.com   # already on example.com
error: navigate: page did not commit within 5000ms
$ ff-rdp record stop
$ cat /tmp/r.json
{ "$schema": "…", "version": 1,  "steps": [  ] }
```

This is the highest-impact bug in the tree: any script that "navigates
home, then …" cannot be re-run from the home page; any agent that
records a flow starting from its target page produces an empty
script.

### 3. `screenshot --full-page` produces viewport-only PNG — **major (unfixed from session 48)**

Confirmed again on HN: both `screenshot` and `screenshot --full-page`
emit 1366×683 PNGs. iter-61h's chrome-scope fallback ignores
`full_page`. The user-typed fix (passing `arr.length` to
`writeByteArray`, 0o600 perms, configurable poll timeout) doesn't
address this — the chrome-scope JS calls `captureScreenshot({ fullpage:
full, dpr: 1.0, snapshotScale: 1.0 })` but Firefox 151's
`captureScreenshot` may not honour `fullpage` from this entry point.
PR #73 deferred forwarding `prepareCapture` metadata (rect/DPR/zoom);
this is the visible symptom.

### 4. `dom` polymorphic shape — **major (unfixed from session 48)**

```sh
$ ff-rdp dom 'a.morelink' --jq '.results | type'  # "object"
$ ff-rdp dom 'tr.athing'  --jq '.results | type'  # "array"
$ ff-rdp dom 'a.morelink' --jq '.results[0]'      # null
```

Agent friction: every `--jq '.results[0]'` pattern is wrong half the
time.

### 5. `computed --prop` is single-valued AND rejects `--<name>` — **moderate (unfixed)**

```sh
$ ff-rdp computed body --prop color --prop font-size
error: the argument '--prop <NAME>' cannot be used multiple times

$ ff-rdp computed body --prop "--bg-color"
error: similar argument exists: '--port'   # clap treats --bg-color as a flag
```

Fix: `--prop = Vec<String>` with `value_delimiter = ','`; accept
`--prop=--bg-color` (the `=` form bypasses clap's flag-parsing for
positional-style values).

### 6. `eval --stringify --format text` emits hint suffix — **minor (unfixed)**

```sh
$ ff-rdp eval '({a:1})' --stringify --format text
"{\"a\":1}"

  -> ff-rdp console --level error  # Check for console errors
```

The hint is meant for humans but agents piping the output get the
trailing tip in their captured string. `--no-hints` works as opt-out.
Suggested: when `--stringify` is set, treat it like `--jq` and
auto-suppress hints (the intent is "raw value extraction").

### 7. `eval` blocked by CSP — **feature gap**

Every `eval` on HN (and Wikipedia per session 44/48, and the admin
Wardrobe app per session 44/45) fails with:

```json
{"message":"call to eval() blocked by CSP","name":"EvalError",...}
```

Firefox's debugger uses `eval()` under the hood for the "evaluate JS"
RPC; sites with strict CSP block this. Playwright and Puppeteer work
around this by using the privileged debugger Realm (executes JS
without going through the page's CSP). ff-rdp has no such fallback,
which makes every `eval`-based recipe useless on the most security-
sensitive sites — exactly the sites agents would be most likely to
test.

Not a regression — has always been this way — but worth tracking as a
real feature gap.

### 8. Iter-60 `meta.refs_registered` flag is unreliable — **minor / new**

Some `dom` calls return `meta.refs_registered: false` even when refs
*are* in the per-item output. The flag is informational and we don't
have to rely on it, but it's misleading either way.

## What Works Well

- **Version warning removal**: every prior session had a redundant
  warning per command; agents pay tokens for it. Now silent.
- **Doctor probe wording**: post-iter-61h reports
  `Firefox 151 (newer than tested range 120–150, but supported)` — both
  informative and unambiguous about whether the user should worry.
- **`scroll bottom` / `scroll text` / `scroll to`** all work. Only
  `scroll top` was flagged but couldn't be verified here (CSP blocks
  the `scrollY` eval used to check).
- **Recorder skip-on-failure** is correct: failed `navigate` doesn't
  poison the recording.

## Summary

- 14+ commands exercised, **6 prior bugs still unfixed**, **1 new
  major bug** (iter-60 ref resolution), **1 confirmed feature gap**
  (CSP eval), **2 fixed** (version warning, launch stderr).
- Next iteration (iter-61i) should prioritise:
  1. Same-URL navigate timeout — bites every agent
  2. iter-60 ref resolution — promised feature, doesn't work
  3. `dom` polymorphic shape — agent-jq footgun
  4. `--full-page` chrome-scope path
  5. `computed --prop` repeatable + `--<name>` quoting
  6. Auto-`--no-hints` under `--stringify`

## References

- [[iteration-61-script-runner-recorder]] — refs are an iter-60 surface;
  the runner just relays them
- [[dogfooding-session-48]] — predecessor; the same-URL navigate and
  full-page screenshot are returning items here
- [[dogfooding-session-47]] — original site for the same-URL bug; HN
- [[dogfooding-session-44]] — CSP-eval gap was first noted on the
  admin Wardrobe app
