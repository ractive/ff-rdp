---
title: Dogfooding Session 48 — iter-61h verification on Wikipedia, Firefox 151
type: dogfooding
date: 2026-05-22
status: completed
site: https://en.wikipedia.org
commands_tested: [doctor, launch, tabs, navigate, page-text, snapshot, screenshot, dom, click, scroll, wait, eval, perf, a11y, network, console, cookies, storage, sources, computed, daemon, record, run]
tags:
  - dogfooding
  - iter-61h
  - firefox-151
  - regression-verification
  - recorder-roundtrip
---

# Dogfooding Session 48

Verification pass on Wikipedia (`https://en.wikipedia.org/wiki/Firefox`)
against an even newer Firefox than iter-61h targeted: **Firefox 151**
(doctor warns "outside tested range 120–150"). Three previously-major
issues are fixed, three are still broken, and a handful of new ones
showed up — most notably a shape inconsistency in `dom` and the
**still-unfixed re-navigate-to-same-URL timeout** that bites both the
CLI and the new script runner.

Previous: [[dogfooding-session-47]] · [[iteration-61h-headless-screenshot-firefox150]]

## TL;DR

- ✅ **Headless `screenshot` works on Firefox 151** — iter-61h's chrome-scope fallback
  carries forward to a Firefox version it wasn't tested against. Win.
- ✅ **`wait` error now names the unmet selector** with a helpful
  `dom --count` hint. (Session 47 #2 fixed.)
- ✅ **Recorder + runner round-trip works end-to-end** —
  `record start` → manual CLI commands → `record stop` → `ff-rdp run`
  replays cleanly (iter-61b's headline promise delivered).
- ❌ **Re-navigating to the current URL still times out after 5 s** —
  same as session 47 #1, reproducible in both `ff-rdp navigate` and
  inside `ff-rdp run`. This is by far the most painful agent friction:
  any script that "navigates to the home page" will fail when run from
  that home page.
- ❌ **`computed --prop` still rejects multiple values** —
  `--prop color --prop background-color` errors; `--prop color,background-color`
  silently returns `""`. (Session 47 #4 unfixed.)
- ❌ **`dom` returns `results` as an object for 1 match, array for >1** —
  agent `--jq '.results[0]'` only works for >1, returns `null` for 1.
- ❌ **Iter-60 ref IDs (`e23`) are not registered** by `dom` (and not
  emitted by `snapshot`) even with daemon running. `meta.refs_registered: false`
  on every call.
- ⚠ **`screenshot --full-page` produces viewport-sized PNG** (1366×683
  on a 44 571-tall page). New bug.
- ⚠ **Firefox 151 warning is printed for every command and every script
  step** — 20-step script = 20 redundant warnings, all to stderr but
  still token cost for any agent piping stderr.

## What's New Since Last Session

- iter-61g: navigate blocking, network nav-scoped buffer, sources walker fallback (merged)
- iter-61h: headless screenshot chrome-scope fallback for Firefox 149+ (in flight on PR #73)
- iter-61b: recorder CLI wiring + script runner schema-strict mode (merged)

## Regression Checks

| Session 47 issue                                                    | Status                  | Notes |
|---------------------------------------------------------------------|-------------------------|-------|
| #1 Re-navigate to current URL times out                             | **STILL BROKEN**        | reproduces in both CLI and script runner |
| #2 `wait` timeout error not selector-aware                          | ✅ FIXED                 | now: `selector 'X' not found after Nms on tab '…' — the element may not exist; verify with 'ff-rdp dom 'X' --count'` |
| #3 sources fallback path-of-record disagrees with iter-61g design   | not directly tested     | `sources` returns 2 results on Wikipedia, no error |
| #4 `computed --prop` not repeatable                                 | **STILL BROKEN**        | `--prop a --prop b` errors; `--prop a,b` returns empty string |
| #5 Headless screenshot broken                                       | ✅ FIXED (on Firefox 151)| iter-61h's chrome-scope fallback works |
| #6 `network --since all` output shape varies                        | resolved as my error    | flag is nav-index, not duration — clearly documented in `--help` |
| #7 Snapshot default-depth knob not visible                          | partial — visible in `meta` | `snapshot --jq '.meta'` shows `{"depth":6,"max_chars":50000}`, no CLI flag to override |

## Smoke Test Results

| Command | Status | Notes |
|---|---|---|
| `doctor` | ✅ | warns "Firefox 151 outside tested range" — accurate |
| `launch --headless` | ✅ (false stderr) | works but prints `error: Firefox exited immediately with exit status: 0: *** You are running in headless mode.` to stderr while Firefox is actually fine |
| `tabs` | ✅ | clean |
| `navigate` | ⚠ | spurious `tabDestroyed` error on first nav from about:blank; page actually navigates |
| `page-text` | ✅ | 125 KB on Wikipedia article — reasonable |
| `snapshot` (text) | ✅ | terse, but NO `[ref=eN]` IDs as the iter-60 plan promised |
| `snapshot` (JSON) | ✅ | works; `meta.depth=6, max_chars=50000` |
| `screenshot` | ✅ | 309 KB PNG, 1366×683, opens cleanly |
| `screenshot --full-page` | ❌ | identical to non-full-page output |
| `dom h1` (1 match) | ⚠ | shape: `results: {…}` (object) — breaks `--jq '.results[0]'` |
| `dom p` (>1 match) | ✅ | shape: `results: [{…}]` (array) |
| `dom --count` | ✅ | clean |
| `click` | ✅ | clean, returns `{clicked: true, entered: true, tag, text}` |
| `scroll bottom/top/text` | ✅ | works |
| `scroll down` | (n/a) | not a real subcommand — dogfood skill template is stale |
| `wait --selector --wait-timeout` | ✅ | error msg now hint-rich |
| `eval` | ✅ | primitives return `results: <value>` directly; objects return full RDP-grip shape |
| `perf vitals` | ✅ | FCP 920ms, TTFB 639ms, LCP approx 2296ms (all "good") |
| `perf audit` | ✅ | DOM 12 799 nodes, 70 images without lazy, 6 render-blocking |
| `a11y contrast --fail-only` | ✅ | 59 pass / 0 fail, capped |
| `network` (default) | ✅ | 9 entries scoped to last nav |
| `network --since all` | ✅ | 20 entries (capped); shape is array |
| `console --level error` | ✅ | returns array |
| `cookies` | ✅ | 7 cookies |
| `storage localStorage` | ⚠ | 1.2 MB dump (MediaWiki caches scripts in storage); `--key` filter helps; doc says shape is `[{key,value}]` array but actual is `{key: value, …}` object |
| `sources` | ✅ | 2 sources |
| `daemon status` | ✅ | shows `buffer_sizes.network-event: 521` |
| `record start` → CLI cmds → `record stop` | ✅ | writes valid script JSON; captured 3 steps |
| `run <recorded-script>` | ✅ | replays in 701 ms, all 3 steps ok |
| `run` with `assert_text/assert_url/assert_no_console_errors` | ✅ | all assertions work; `ignore_patterns` works for filtering Wikipedia's Referrer-Policy noise |

## Findings

### What Works Well

- **Recorder workflow is genuinely useful.** `record start` → manual
  CLI commands → `record stop` → `ff-rdp run` is the
  five-actions-becomes-one-tool-call agent-speed payoff the iter-61
  bundle was designed for. Felt low-friction.
- **Headless screenshot survival on Firefox 151** is a vote of
  confidence in iter-61h's chrome-scope fallback — it's tolerant of a
  Firefox version it never saw.
- **`wait` error messages** are now actively helpful — pointing the
  user at `ff-rdp dom 'X' --count` as a debugging next step is the
  exact thing an agent will reach for next.
- **Assert vocabulary** in the runner: `assert_text` /
  `assert_url` / `assert_no_console_errors` (with `ignore_patterns`)
  cover the routine smoke-test surface cleanly.

### Issues Found

#### 1. Re-navigate to current URL still times out — **major (regression)**

```sh
$ ff-rdp navigate https://en.wikipedia.org/wiki/Firefox  # first time: ok
$ ff-rdp navigate https://en.wikipedia.org/wiki/Firefox  # second time:
error: navigate: page did not commit within 5000ms — use --no-wait to skip commit check or increase --timeout
```

Same in the script runner — any script whose first `navigate` lands on
the current URL fails before it begins. This was issue #1 of session 47;
the fix didn't land. Agent impact: high (scripts can't be re-run from
their target page; recorded scripts can't replay against the same tab
without manually navigating away first).

Suggested fix: detect that the requested URL equals the current URL
*before* setting the commit-watch, and short-circuit to "ready" (or
trigger a forced reload of the same URL via the dedicated reload path).

#### 2. `screenshot --full-page` produces viewport-only PNG — **new, major**

```sh
$ ff-rdp screenshot -o /tmp/a.png            # 1366×683
$ ff-rdp screenshot --full-page -o /tmp/b.png # 1366×683 (page is 44 571 tall)
$ file /tmp/a.png /tmp/b.png
# both: PNG image data, 1366 x 683
```

iter-61h's chrome-scope fallback delivers the screenshot but doesn't
honour `--full-page`. Likely the fallback path doesn't pass through the
`full_page` parameter, or the chrome-scope JS uses the default capture
size.

#### 3. `dom` returns object for 1 match, array for >1 — **major**

```sh
$ ff-rdp dom 'h1' --jq '.results | type'  # "object"
$ ff-rdp dom 'p'  --jq '.results | type'  # "array"
$ ff-rdp dom 'h1' --jq '.results[0]'      # null  (breaks the "first match" idiom)
```

Inconsistent polymorphic shape. Every agent that learns to write
`--jq '.results[0]'` will randomly get null. Recommend: always return
an array, even for single matches, like `network`/`console`/`a11y`
already do.

#### 4. Iter-60 ref IDs not registered — **moderate**

```sh
$ ff-rdp dom 'h1' --jq '.meta'
{"refs_registered":false,"selector":"h1"}

$ ff-rdp snapshot --format text | grep -c '\[ref='
0
```

The iter-60 plan promised `dom`/`snapshot` would mint stable
`e<N>` ref IDs (Playwright `aria-snapshot` style). They are missing
from output even with daemon running. Either the registration path is
gated on something not satisfied here, or the feature regressed.
Worth a triage: is this a Wikipedia-specific thing, a Firefox-151
thing, or the wider iter-60 gap I called out in
[[dogfooding-session-44]]'s follow-up?

#### 5. `computed --prop` still single-valued — **moderate (regression)**

```sh
$ ff-rdp computed body --prop color --prop background-color
error: the argument '--prop <NAME>' cannot be used multiple times

$ ff-rdp computed body --prop color,background-color
"results": ""  # silently empty
```

Session 47 #4. Workaround: call `computed` twice. Real fix: change the
clap arg to `Vec<String>` with `value_delimiter = ','`.

#### 6. `navigate` spurious `tabDestroyed` on cold-start — **minor**

```sh
$ ff-rdp navigate https://en.wikipedia.org/wiki/Firefox  # after fresh launch
internal error: actor error from server1.conn2.tabDescriptor1: tabDestroyed (tabDestroyed) — Tab destroyed while performing a TabDescriptorActor update
$ ff-rdp tabs --jq '.results[0].url'
"https://en.wikipedia.org/wiki/Firefox"  # actually navigated fine
```

The first navigate after `launch` returns this internal-error message
to stderr, yet the page navigates successfully. Agents that trust
non-zero exit will retry unnecessarily. Worth swallowing if the tab
ends up where it was asked to go.

#### 7. `launch --headless` prints a stderr "error" while succeeding — **minor**

```sh
$ ff-rdp launch --headless --port 6000
error: Firefox exited immediately with exit status: 0: *** You are running in headless mode.
$ ff-rdp doctor   # …firefox is running fine
```

The "*** You are running in headless mode" line is Firefox printing to
stderr during normal headless startup; ff-rdp is misinterpreting it as
an "exited immediately" failure. Pure cosmetic but it's the very first
command in any agent transcript.

#### 8. Firefox-version warning prints on every call — **minor**

`warning: connected to Firefox 151, but ff-rdp is tested against
Firefox 120–150; some features may not work correctly` appears on
**every** ff-rdp invocation, including each step of `ff-rdp run`.
A 20-step script = 20 identical warnings. Consider once-per-process
(after first connect) or suppress entirely under `run` unless `--verbose`.

#### 9. `storage` doc says array, returns object — **doc/output mismatch**

`storage --help` documents result shape as `[{key, value}]`. Actual
output for the no-`--key` case is `{key: value, key: value, ...}`.
Pick one and align both.

### Feature Gaps

- **`scroll page`** would be welcome — `scroll by --dy <viewport-height>`
  works but having `scroll page down|up` would be friendly.
- **`storage --keys`** (just enumerate keys, no values) would prevent
  the 1.2 MB MediaWiki dump from blowing the context.
- **`navigate --reload-if-same`** explicit flag to bypass the
  same-URL-deadlock without falling back to `reload`.

## Summary

- **23 commands tested**, of which **20 work cleanly**, **3 are
  broken/regressed**, and **6 minor friction items** noted.
- Headline takeaway: iter-61h's screenshot fix lands cleanly **on a
  Firefox version it wasn't tested against**, the recorder→runner
  round-trip works end-to-end, but the **same-URL navigate timeout
  from session 47 remains** and is now the highest-impact bug for
  agent flows.

## References

- [[iteration-61h-headless-screenshot-firefox150]] — fix verified working on Firefox 151
- [[iteration-61b-recorder-cli-wiring]] — recorder + runner round-trip verified
- [[iteration-61g-session-48-deferred]] — navigate-blocking introduced the same-URL bug
- [[dogfooding-session-47]] — predecessor; issues #1 and #4 are unfixed regressions
- [[dogfooding-session-44]] — the dom/snapshot ref-IDs gap was first noted here
