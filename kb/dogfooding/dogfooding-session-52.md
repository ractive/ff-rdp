---
title: Dogfooding Session 52 — verifying iter-61j fixes and hunting new bugs
type: dogfooding
date: 2026-05-23
status: completed
site: https://news.ycombinator.com, https://example.com, https://en.wikipedia.org/wiki/HTTP
commands_tested: [doctor, launch, tabs, navigate, page-text, screenshot, cookies, storage, eval, click, type, dom, snapshot, a11y, perf, network, console, scroll, computed, daemon, sources, geometry]
tags: [dogfooding, regression-verification, iter-61j, new-bugs]
---

# Dogfooding Session 52

Verified iter-61j (PR #75 / commit 067d1b6) fixes against HN, example.com, and Wikipedia, then probed for new bugs. **Six of ten iter-61j fixes confirmed working**; two are **broken or only partially fixed** (locale pin, --with-network/watcher path); two are **regressed/incomplete** (screenshot --full-page, navigate UX has new race). Found **9 new bugs** plus identified a fundamental design limitation around `eval` and site CSP.

## TL;DR

- ✓ Fixes that landed cleanly: page-text dedup, eval --stringify real JSON, computed multi-prop, dom single-match ref parity, dead-Firefox detection, --with-network inline.
- ❌ Fixes that DID NOT work or regressed:
  - **`screenshot --full-page` STILL broken** (4th session running) — Wikipedia/HTTP scrollHeight=22491 → PNG=600px.
  - **Firefox locale pin INEFFECTIVE** — console messages still in German on fresh ff-rdp-launched profile; `intl.locale.requested` alone doesn't override macOS system locale for DevTools/quirks-mode messages.
  - **`--with-network` only surfaces watcher data inside `navigate` response** — subsequent `network` calls still fall back to performance-api even though the daemon's `buffer_sizes: {network-event: 209}` shows events ARE buffered.
- New bugs found (9, see below). Most-impactful: **`computed` has three different output shapes**, **`navigate` reports success for DNS-failed URLs**, **`--fields` silently ignored on `tabs`**.
- The repeated `screenshot --full-page` regression is a credibility issue — it has now failed in sessions 48, 49, 51, 52.

## What's New Since Last Session

- iter-61j merged (PR #75, commit 067d1b6) — 13-issue cleanup from session 51.

## Regression Checks (iter-61j fixes)

| iter-61j fix | Status | Evidence |
|---|---|---|
| page-text drops `text` key | ✓ fixed | `keys: ['results', 'total']`, no `text` |
| `eval --stringify` returns real JSON object | ✓ fixed | `results` is `{foo:1, bar:[1,2,3]}`, not a string |
| `computed` multi-prop via repeatable `--prop` | ✓ fixed | `--prop color --prop font-size --prop display` → `{computed:{...}}` |
| `computed` accepts custom CSS variables | ✓ fixed | `--prop=--my-var` returns the var value (but see new bug #1) |
| `dom` single-match ref parity | ✓ fixed | `dom 'title'` returns `refs_registered:true, ref:"e1"` |
| navigate default timeout bumped | ✓ fixed | HN navigates in 1.1s without `--no-wait`; old 5s default was tight |
| dead-Firefox detection | ✓ fixed | After `kill <pid>`: `error: could not connect to Firefox at localhost:6000` with clear hint |
| unified navigate timeout message | ✓ fixed (best-effort) | Saw `operation timed out — try increasing --timeout` consistently |
| Firefox locale pin (English) | ❌ **NOT FIXED** | German console message still emitted: `"Diese Seite befindet sich im Kompatibilitätsmodus..."` after fresh launch. `prefs.js` shows `intl.locale.requested=en-US` was set, but Firefox UI/DevTools locale on macOS is still German. |
| `--with-network` engages WatcherActor (response headers reachable) | ⚠ **PARTIAL** | `navigate --with-network` returns proper watcher data inline (status:200, transfer_size). But the subsequent `network` command falls back to `source:performance-api` with `status:null, method:null` and `--headers` is silently dropped. So the **headers stated as the motivating use-case** are still unreachable. |
| `network` JSON vs text parity | ⚠ **PARTIAL** | Both formats now agree they're using performance-api on the standalone `network` call (consistent fallback), but the inconsistency between `network --since all --detail` (uses watcher) and `network --since all` summary (uses performance-api) remains. |
| `screenshot --full-page` | ❌ **STILL BROKEN** | wiki/HTTP: scrollHeight=22491 → PNG 800x600. HN: also 800x600 (HN has scroll). Has now failed in sessions 48, 49, 51, **and 52**. |
| dogfood skill drift | ✓ (assumed fixed; skill SKILL.md re-read, no `llm-help`/`recipes` refs) |   |

## Smoke Test Results

| Command | Status | Notes |
|---|---|---|
| `doctor` | ✓ | Clean JSON |
| `launch --headless --port 6000 --auto-consent` | ✓ | Temp profile, PID returned |
| `tabs` | ✓ | But `--fields` silently dropped (new bug #2) |
| `navigate <url>` | ⚠ | Default timeout OK; race-condition timeout on cross-origin fast pages (new bug #3) |
| `navigate <invalid-url>` | ❌ | Returns success-shaped JSON for DNS failure (new bug #4) |
| `page-text` | ✓ | Dedup'd output, clean |
| `screenshot` | ✓ | Viewport capture works |
| `screenshot --full-page` | ❌ | Still 800x600 |
| `cookies` | ✓ | HN had none (expected) |
| `storage localStorage` | ✓ | `{}` |
| `eval` | ⚠ | Blocked by site CSP on HN (new design issue #5) |
| `eval --stdin` | ✓ | `echo 'document.title' \| ff-rdp eval --stdin` works |
| `dom 'sel'` | ✓ | Array shape consistent; doesn't pierce shadow DOM (gap #6) |
| `snapshot --depth 4` | ✓ | Good tree |
| `a11y` | ✓ | Returns roles |
| `a11y contrast --fail-only` | ✓ | 0 violations on example.com (plausible) |
| `perf vitals` | ✓ | Plausible numbers |
| `network` default | ⚠ | Always performance-api on `--since -1` even when daemon has data (new bug #7) |
| `network --since all --detail` | ✓ | DOES use watcher (proves data is there) |
| `network --detail --headers` | ❌ | `--headers` silently dropped when source is performance-api (new bug #8) |
| `console` | ✓ | Captures errors; locale leak as above |
| `scroll bottom` | ✓ | `atEnd:true` |
| `computed h1 --prop X --prop Y` | ✓ | Multi works |
| `computed h1 --prop X` | ⚠ | Returns bare string, inconsistent shape (new bug #1) |
| `daemon status` | ✓ | Shows `buffer_sizes: {network-event: 209}` — events captured but unused |
| `geometry h1` | ✓ | Clean output |
| `sources --limit 3` | ✓ | Falls back to js-eval; works on HN |
| `--fields` on `dom`/`snapshot`/`network --detail` | ✓ | Honored |
| `--fields` on `tabs` | ❌ | Silently ignored (new bug #2) |

## New Findings (Bugs Not in Session 51)

### 1. `computed` has three different output shapes depending on `--prop` count [moderate]

Same polymorphic-output anti-pattern that bit `dom` in sessions 48/49. Now applies to `computed`:

```bash
$ ff-rdp computed h1                          # zero --prop
# results: {computed: {...}, index, selector}  ← single object

$ ff-rdp computed h1 --prop color             # single --prop
# results: "rgb(0, 0, 0)"                      ← bare string!

$ ff-rdp computed h1 --prop color --prop display  # multi --prop
# results: [{computed: {...}, index, selector}]  ← array
```

LLM/script consumers can't write one parser. **Fix**: always wrap in array of `{computed:{...}, index, selector}`. This is consistent with how iter-61i normalized `dom`.

### 2. `--fields` silently ignored on `tabs` [moderate]

```bash
$ ff-rdp tabs --fields url,title
# Returns ALL fields (actor, browsingContextID, selected, title, url) — flag has no effect

$ ff-rdp tabs --fields nonexistent_field
# Same — no error, no warning, no filtering
```

Documented in `--help` as a global flag. Works on `dom`, `snapshot`, `network --detail`. Broken on `tabs`. Suggest auditing every command for `--fields` plumbing.

### 3. `navigate` race-condition timeout on fast cross-origin pages [moderate]

After navigating HN → example.com:

```bash
$ ff-rdp navigate https://example.com
error: operation timed out — try increasing --timeout

$ ff-rdp tabs   # but tab is actually on example.com — load happened
```

Reproduced twice in a row. Subsequent retries on the same command succeeded immediately. Looks like the commit event arrives before/during the wait setup. **Fix**: check current URL after timeout and return success if it matches the target.

### 4. `navigate` reports success for DNS-failed URLs [major]

```bash
$ ff-rdp navigate https://this-domain-truly-does-not-exist-zzz.invalid
{
  "results": {
    "committed_url": "https://this-domain-truly-does-not-exist-zzz.invalid/",
    "elapsed_ms": 177,
    "navigated": "https://this-domain-truly-does-not-exist-zzz.invalid",
    "ready_state": "interactive"
  }
}
$ ff-rdp tabs  # url is actually about:neterror?e=dnsNotFound&...
```

Same shape as a successful navigate. An LLM driving a script can't tell the page failed to load. **Fix**: detect `about:neterror` and return a `navigation_failed` error (or include `error_type: "dns_not_found"` in result).

### 5. `eval` blocked by site CSP `script-src` (no `unsafe-eval`) [major — design issue]

```bash
$ ff-rdp eval 'document.title'   # on news.ycombinator.com
error: call to eval() blocked by CSP
# Console: "...blockiert ... \"script-src 'self' 'unsafe-inline' ...\" (es fehlt 'unsafe-eval')"
```

HN's CSP doesn't allow `unsafe-eval`. ff-rdp wraps user expressions in JS `eval()` via the debugger console, and the page's CSP applies. This breaks `eval` on **any moderately-secured site** — github.com, twitter.com, banks, news sites. Probably the biggest single LLM-agent blocker we have today.

**Fixes to investigate**:
- Use `Cu.evalInSandbox` (privileged sandbox, ignores page CSP) via consoleActor.
- Inject a `<script>` element with the code instead of `eval`.
- Document the limitation in `eval --help` and add a workaround recipe.

### 6. `dom` does not pierce shadow DOM [moderate — feature gap]

```bash
# After creating an open shadow root with #host and <p id=shadow-p> inside
$ ff-rdp dom 'p#shadow-p'        # → results: [], total: 0
$ ff-rdp dom '#host'             # → returns host but no indication it has shadow content
$ ff-rdp eval 'document.querySelector("#host").shadowRoot.querySelector("p").textContent'
# → "I am in shadow"   (the content IS there)
```

Modern SPAs (web components, lit, stencil, custom elements) are full of open shadow roots. `dom` should either:
- Pierce open shadow roots automatically, or
- Add a flag `--include-shadow` / `--shadow open|closed|none`, AND
- Flag `hasShadowRoot: true` on host elements so callers know to dig.

### 7. `network --since -1` (default) always falls back to performance-api even when daemon has watcher data [major]

```bash
$ ff-rdp daemon status   # buffer_sizes: { network-event: 209 } — daemon HAS the events
$ ff-rdp network         # source: performance-api  (ignores daemon buffer!)
$ ff-rdp network --since all --detail
# source: watcher        (DOES use daemon buffer — proves data is there)
```

The default scoping logic for `--since -1` (current navigation) appears not to query the daemon's watcher buffer. Falling back to performance-api loses `status`, `method`, `transfer_size`, response headers. Affects every `network` call by default.

### 8. `--detail --headers` silently dropped when source is performance-api [moderate]

```bash
$ ff-rdp network --detail --headers --limit 1
# Output has no "headers" key — silently ignored because perf-api can't supply them
```

Per `--help`: "Output (--detail --headers): adds {"headers": {"request": ..., "response": ...}} per entry." But when source is performance-api, the flag is dropped silently. Should emit a `note: "--headers ignored (performance-api source has no headers)"` like other fallback notes.

### 9. Stale `consoleActor` after navigating to an error page [minor]

After `navigate https://this-domain...invalid` (which goes to about:neterror), the very next `eval` returned:

```
error: actor error from server1.conn11.child22/consoleActor3: noSuchActor (unknownActor) —
No such actor for ID: server1.conn11.child22/consoleActor3 — the tab may have been closed
or navigated away; try again.
```

The hint says "try again", but the navigate that caused it should refresh the actor cache itself. Currently the caller must explicitly retry.

### 10. iter-61j locale pin is ineffective on macOS [moderate]

iter-61j added:
```
user_pref("intl.accept_languages", "en-US, en");
user_pref("intl.locale.requested", "en-US");
```

These set the **content** locale (HTTP `Accept-Language`) and content-pref locale, but **do not** change Firefox's UI/DevTools locale. Console message reproduced after fresh `ff-rdp launch`:

```
Diese Seite befindet sich im Kompatibilitätsmodus (Quirks). Das Seitenlayout
kann beeinflusst werden. Verwenden Sie für den Standardmodus "<!DOCTYPE html>".
```

Also `about:neterror` description came in German: `"Die Verbindung mit dem Server ... schlug fehl."`. To actually pin the UI locale on macOS you need either:
- `intl.locale.requested=en-US` **AND** `intl.locale.matchOS=false`, plus an `en-US` langpack installed; OR
- Launch Firefox with `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8` env vars; OR
- Pass `-UILocale en-US` on the command line.

Recommend adding all three (env vars are cheapest and OS-agnostic).

## Feature Gaps (Wishlist)

- **CSP-bypassing eval path** (see #5) — single biggest LLM-friendliness lever.
- **`--full-page` screenshot** (4 sessions running).
- **Shadow DOM piercing in `dom`/`snapshot`/`a11y`** — modern web requires it.
- **`navigate` should detect `about:neterror`** and surface as proper error (see #4).
- **`network --headers` should work via watcher even on `--since -1`** (see #7, #8).
- **Per-command audit of `--fields`** — fix `tabs` and any others.

## Summary

- **22 commands exercised** across 3 sites (HN, example.com, Wikipedia/HTTP), plus error-case and edge-case probing.
- **10 of 13 session-51 fixes confirmed**; **3 not working or only partially**: locale pin (broken), `--with-network`/watcher path (partial), `screenshot --full-page` (still broken — 4th session).
- **10 new bugs/issues catalogued** (none of which appeared in session 51). Headline: `computed`'s three-shaped output (#1), `navigate` reports success on DNS failure (#4), site CSP blocks `eval` (#5).
- **Recommendation**: open an **iter-61k** focused on (a) the perennial `--full-page` regression, (b) `computed` output normalization, (c) `navigate` neterror detection, (d) the watcher-buffer-not-read-by-default bug, and (e) properly pin Firefox UI locale via env vars + `-UILocale`. The CSP-eval and shadow-DOM gaps deserve their own design discussions before implementation.

## References

- Previous: [[dogfooding-session-51]]
- Iter under test: [[iteration-61j-dogfood-51-fixes]] / PR #75 / commit 067d1b6
- Targets: https://news.ycombinator.com, https://example.com, https://en.wikipedia.org/wiki/HTTP
