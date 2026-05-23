---
title: Dogfooding Session 51 — security audit on Altoro Mutual (intentionally-vulnerable bank)
type: dogfooding
date: 2026-05-23
status: completed
site: https://demo.testfire.net (OWASP-style vulnerable bank demo)
commands_tested: [doctor, launch, tabs, navigate, page-text, screenshot, cookies, storage, eval, click, type, dom, snapshot, a11y, perf, network, console, scroll, computed, daemon]
tags: [dogfooding, security-audit, owasp, altoro-mutual, regression-verification]
---

# Dogfooding Session 51

Pointed ff-rdp at the deliberately-vulnerable [demo.testfire.net](https://demo.testfire.net) (Altoro Mutual / IBM AppScan demo bank) and used it as a security-audit harness. Confirmed reflected XSS, cookie-based balance disclosure, weak session-cookie flags, and Tomcat version disclosure — entirely through ff-rdp primitives. Also re-verified session 49/50 regression items: same-URL navigate ✓ fixed, dom array shape ✓ fixed, `screenshot --full-page` still broken.

## TL;DR

- ff-rdp is **genuinely usable as a security-audit tool** — `cookies`, `eval --stringify`, `navigate ?query=<payload>`, `eval --file` together can drive form-fuzzing, header inspection, and DOM-side vulnerability scanning. Found 5+ classic OWASP issues in one session.
- The CLI is largely LLM-friendly: `--help` per subcommand is rich, examples are present, output structure is documented. But `eval --stringify` returns the JSON-as-string (double-encoded), which forces every LLM-driven workflow to re-parse — annoying.
- **The dogfood skill itself is stale**: mentions `ff-rdp llm-help` and `ff-rdp recipes` as commands (neither exists). Skill example uses `computed ".sel" display,font-size,color` — wrong syntax.
- **`screenshot --full-page` still broken** (session 48 #2 / 49 #3): on a 21092-px wikipedia page, PNG came back 683-px tall.
- **`--with-network` does not engage the WatcherActor**: every `network` call falls back to performance-api (status=null, method=null, no headers) even after a fresh navigate with the flag.

## What's New Since Last Session

- iter-61i merged (PR #74): same-URL navigate, dom array shape, `--stringify` hint suppression.
- No new iterations since session 50.

## Regression Checks

| Item | Previous Status | Current Status | Notes |
|---|---|---|---|
| Same-URL navigate hangs (s49 #2) | major | ✓ **fixed** | Re-navigating to current URL: 0.084s |
| `dom` polymorphic shape (s48 #3 / s49 #4) | major | ✓ **fixed** | Single match now returns array `[{...}]` |
| `eval --stringify --format text` hint (s49 #6) | minor | ✓ **fixed** | Suppressed |
| `screenshot --full-page` viewport-only (s48 #2 / s49 #3) | major | ❌ **still broken** | scrollHeight=21092 → PNG height=683 |
| `computed --prop` single-valued (s48 #5 / s49 #5) | moderate | ❌ **still single-valued** | No `--props a,b,c` form; positional comma-list rejected |
| Iter-60 ref IDs not always registered (s48 #4 / s49 #8) | moderate | ⚠ **partially** | `dom 'title'` (single match) has `refs_registered:false` and no `ref` field; multi-match has refs. Inconsistent. |

## Smoke Test Results

| Command | Status | Notes |
|---|---|---|
| `doctor` | ✓ | Clean JSON, daemon detected |
| `launch --headless --port 6000` | ✓ | Temp profile, PID + path returned |
| `tabs` | ✓ | Clean |
| `navigate <url>` | ⚠ | Hangs against slow real sites until --timeout fires; need `--no-wait` for cross-internet sites. `demo.testfire.net` did NOT commit within 20s; `--no-wait` worked. |
| `navigate --no-wait` | ✓ | Returns immediately; tab updates within ~3s |
| `page-text` | ⚠ | **Both `results` and `text` keys hold the identical full page text** — output is duplicated, wasting tokens (a Wikipedia page would double the cost) |
| `screenshot` | ✓ | Default viewport capture works |
| `screenshot --full-page` | ❌ | Still produces viewport-only PNG (683 px on a 21092 px page) |
| `cookies` | ✓ | Surfaces `isHttpOnly`, `isSecure`, `sameSite`, `expires` — exactly what a security tool needs |
| `storage localStorage` / `sessionStorage` | ✓ | Clean empty-object output |
| `eval` (positional) | ✓ | Works |
| `eval --file` | ✓ | Works once file path is correct (initial heredoc didn't create the file, error message was clear: "No such file or directory") |
| `eval --stringify` | ⚠ | Returns `"results": "{\"foo\":...}"` — caller must parse the string. Fine for human eyes; friction for LLM pipelines. |
| `click` / `type` | ✓ | Compose well — drove a full SQLi-style login attempt |
| `dom 'sel'` | ✓ | Array shape now consistent |
| `snapshot` | ✓ | Deep DOM tree with `truncated` markers; sensible depth=6 default |
| `a11y` | ✓ | Returns tree |
| `a11y contrast --fail-only` | ✓ | Returned 0 entries on bank page (plausible) |
| `perf vitals` | ✓ | Plausible numbers; LCP note about headless approximation present |
| `network` default | ⚠ | Always falls back to performance-api (see Findings #5) |
| `network --format text` | ⚠ | Shows different fields than JSON for same data (status/transfer_size populated in text, null in JSON — different code paths?) |
| `console --level error` | ✓ | Captures quirks-mode warning |
| `scroll bottom` | ✓ | Returns `atEnd: true`, scrollHeight |
| `computed h1` | ✓ | Returns non-default props |
| `computed h1 display,font-size,color` | ❌ | "unexpected argument" — multi-prop not supported |
| `daemon status` | ✓ | PID, uptime, buffer sizes |

## Findings — security exploitation via ff-rdp

Used ff-rdp as the only tool (no curl-driven scanning) to find real OWASP-class vulns on demo.testfire.net.

### 🎯 Real vulnerabilities surfaced

1. **Reflected XSS** via `/search.jsp?query=`. Payload `<script>window.XSSED=1</script>` executed — confirmed by `ff-rdp eval 'window.XSSED'` returning `1` after navigate. Two ff-rdp calls, end-to-end.

2. **Sensitive data in a non-secure, non-httpOnly cookie**. `ff-rdp cookies` revealed `AltoroAccounts=ODAwMDAwfkNvcnBvcmF0ZX41LjI0NDkwMDg2MUU3...` with `isHttpOnly:false, isSecure:false, sameSite:""`. Base64-decoded:
   ```
   800000~Corporate~5.244900861E7|800001~Checking~50731.44|
   ```
   Account balances ($52M / $50K) in a cookie that JS can read. Classic. ff-rdp made this discoverable in one command.

3. **Weak session-cookie SameSite**. `JSESSIONID` has `sameSite:""` (no explicit attribute → defaults to Lax in modern Firefox but should be explicit `Strict` for a banking session).

4. **CSRF absent on every form**. `eval --stringify` audit of all `<form>` elements: zero forms with a CSRF token (search.jsp, login.jsp, showAccount). Trivial to enumerate via the DOM.

5. **Information disclosure**. `index.jsp?content=../../../etc/passwd` returned `HTTP 500` with full Tomcat stack trace including `Apache Tomcat/7.0.92` version + Jasper class paths. Tomcat 7 is EOL; 7.0.92 has known CVEs.

6. **SQLi-style auth bypass via default creds**. `admin/admin` accepted (intentional on this site). The `' OR '1'='1` payload didn't bypass — suggests they patched the literal-injection bug while leaving everything else. Ran the whole flow as `type → type → click → tabs` chain — ff-rdp's `type`/`click` chain is solid for credential testing.

**Takeaway**: ff-rdp is a credible web-security harness for LLM agents. With one helper script (an `eval --file` that returns `{forms, cookies, mixedContent, csp, scripts, comments}`) you have a working DOM-side scanner.

### Bugs / friction found in ff-rdp itself

1. **`page-text` duplicates output**. Returns both `"results": "<full text>"` and `"text": "<full text>"` — same content twice. Wikipedia would double the JSON payload. Either drop `text` or make `results` the metadata wrapper only.

2. **`eval --stringify` returns string-encoded JSON**. `"results": "{\"foo\":1}"` instead of `"results": {"foo": 1}`. For LLM pipelines you have to JSON.parse the string back. Suggest parsing on the ff-rdp side and returning a proper object (or add `--stringify=parsed`).

3. **`screenshot --full-page` still captures viewport only**. wiki/HTTP: scrollHeight=21092 → PNG=683. Unfixed since session 48.

4. **`computed` doesn't support multiple properties**. `computed h1 color,font-size` rejected. `--prop` only accepts one. No `--props a,b,c`. Would be the most natural CSS-debugging UX. (s48/49 #5).

5. **`--with-network` doesn't actually engage the WatcherActor in this session**. Every `network` call after a `navigate --with-network` returned `source: performance-api` with `method:null, status:null, transfer_size:null`. Daemon was running (`daemon status` confirmed PID + uptime) but `buffer_sizes: {}` even after navigations. Result: response headers (e.g. CSP, HSTS, X-Frame-Options) cannot be inspected — a major gap for a security workflow.

6. **`network` JSON vs text formats disagree**. `--format text` reported `200, 7181b, 142ms` for a request; the same request via JSON reported `status: null, transfer_size: null`. The two formats appear to draw from different sources or merge differently.

7. **`dom 'title'` (single match) returns `refs_registered: false` and no `ref` field**. Multi-match `dom 'a'` has `ref: "e1"` per result. Single-match should also register or explicitly explain why not.

8. **`navigate` against slow real sites times out with default 5s**. The error is good (`use --no-wait to skip commit check or increase --timeout`) but the default is too low for any cross-internet test. Bumping the default to 10s would save every newcomer 1 retry.

9. **Two different timeout messages**. First retry said "page did not commit within Xms"; second retry on same site said "operation timed out". Same root cause, different prose.

10. **Firefox locale leaks into console output**. The quirks-mode warning came back in German (`"Diese Seite befindet sich im Fast-Standards-Modus..."`). Headless profile inherited macOS locale — LLM string matching on console will break across users. Consider forcing `LANG=C` or `intl.accept_languages=en-US` in launched profiles.

11. **`navigate --no-wait` after Firefox died silently navigated to nothing**. When an external process killed Firefox, the very first `navigate --no-wait` returned `{"navigated":"..."}` with `total:1` (success-shaped). Only the next `tabs` call exposed the disconnect. A pre-flight ping in `navigate` would catch this.

### Skill-side issues

12. **The dogfood skill references commands that don't exist**: `ff-rdp llm-help` and `ff-rdp recipes`. Both `error: unrecognized subcommand`. Skill should be updated.

13. **The dogfood skill's `computed` example is wrong**: shows `computed ".some-element" display,font-size,color` — that exact form is rejected by the CLI.

### LLM-friendliness of `--help`

**Good**:
- Top-level `--help` opens with "Quick start" examples, then a one-line description per subcommand.
- Per-subcommand help has clear "Output: {...}" stanzas describing the JSON shape (this is gold for LLMs).
- `eval --help` documents the three input modes, when to use `--file` vs positional, when to use `--stringify`, and why (Firefox's actor grip metadata).
- `network --help` documents the watcher vs performance-api split, including which fields are null in fallback.

**Could be better**:
- No `--help-all` / man-page-style flag that dumps every subcommand at once for a single `ff-rdp --help-all` LLM context. Would replace `llm-help` (referenced by skills but missing).
- Output examples could include a representative *value*, not just the key shape — e.g. `{"results": <true|false>}` is less useful than `{"results": true}`.

## Feature Gaps

- **Response-header inspection** — there is no path to read response headers in this session, because the watcher fallback strips them. Security work needs CSP/HSTS/X-Frame-Options/Set-Cookie attributes from response. Either fix `--with-network`, or add a dedicated `ff-rdp headers <url>` that fetches via the watcher.
- **`screenshot --full-page` fix** is overdue (3 sessions).
- **Bulk computed-style props**: `computed sel color,font-size,display` or `--props a,b,c`.
- **Cookie decoder hints**: `cookies` could flag base64-looking values (`(decoded: ...)`) — would have surfaced the AltoroAccounts leak even faster.
- **A `--parsed` flag for `eval --stringify`** that returns a real JSON object instead of a string, so LLMs don't have to JSON.parse twice.

## Summary

- ~22 commands exercised on a real, intentionally-vulnerable bank app.
- 6 real OWASP-class vulnerabilities found in the target using only ff-rdp.
- 3 prior regression items confirmed fixed (iter-61i ✓✓✓), 2 still broken (`--full-page`, `computed --prop`).
- 13 ff-rdp/skill issues catalogued, 5 of them new.
- Key takeaway: **ff-rdp is a viable security-audit tool for an LLM agent today** — closing the response-header gap and the `--full-page` regression would make it a great one.

## References

- Previous: [[dogfooding-session-50]]
- Fixed-in: PR #74 / iter-61i (same-URL nav, dom shape, --stringify hint)
- Target site: <https://demo.testfire.net> (HCL / IBM AppScan demo)
