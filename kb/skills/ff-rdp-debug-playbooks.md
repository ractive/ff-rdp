---
title: "ff-rdp-debug — Playbook Catalog (v0 draft)"
type: design
date: 2026-05-13
status: draft
tags:
  - skill
  - ff-rdp-debug
  - playbooks
  - debug
---

# `ff-rdp-debug` — Playbook Catalog (v0 draft)

Candidate playbooks for a `/ff-rdp-debug` Claude Code skill. Each playbook
is a symptom-routed probe: 2–5 ff-rdp commands that converge on the
failing layer, plus the evidence pattern that lets the agent conclude
without overdrilling.

Sources tagged per playbook:
- `dog-NN` — extracted from `kb/dogfooding/dogfooding-session-NN.md`
- `synth-bug` — synthesized from common GitHub-issue / Stack-Overflow patterns
- `synth-tax` — synthesized from devtools/MDN taxonomy as coverage

A playbook earns its slot only if it terminates pointing at a *specific*
failing layer (e.g. "Set-Cookie stripped at edge", "CSP `connect-src`
violation"), not a generic "check network."

---

## Conventions

**Symptom phrasing is non-expert.** Trigger matching should hit common
user phrasings like "login doesn't work", not jargon like "session cookie
not persisted." Each playbook lists 2–4 paraphrases.

**Probe sequences assume a fresh tab on the failing URL.** If not, the
skill prepends `launch --headless URL` or asks the user to point it at a
tab.

**Skill exits early.** If step N produces a signal that concludes the
diagnosis, steps N+1…end are skipped. The skill is a hypothesis tree, not
a checklist.

**"Capture diff" pattern.** Several playbooks (auth, storage, consent)
require *before/after* snapshots of console + cookies + storage around a
user action. Treat this as a primitive: `capture_pre()` → `action` →
`capture_post()` → diff.

---

## A. Auth, sessions, cookies

### A1. Set-Cookie stripped at the edge (CDN/proxy)
**Symptoms:** "login submits but I'm still logged out" / "auth works in
dev, breaks in prod" / "stays on login page after correct password"
**Layer:** CDN / reverse proxy strips `Set-Cookie` on cached path.
**Probe:**
1. `cookies` (baseline; capture session-cookie names if any)
2. `click <submit> --wait-for-network <auth-endpoint>` (iter-57 B3) — or `click` + `network --filter <endpoint> --detail --headers`
3. `cookies` (after)
**Conclude:** auth request `status=2xx`, response headers contain no
`Set-Cookie`, post-cookies == pre-cookies → **edge stripped it**. Confirm
by comparing to `curl -i` against the same endpoint (will also be empty
if CDN strips; **not** empty if Firefox dropped it for SameSite/Secure
reasons).
**Red herrings:** unrelated CSP `eval` console errors (Zod, etc.);
intermediate 302s that drop the Set-Cookie chain.
**Source:** `dog-42`.

### A2. SameSite / Secure dropping cookie before send
**Symptoms:** "logged in but every API call returns 401" / "cookie shows
in DevTools but server doesn't see it" / "works in Chrome, breaks in
Firefox"
**Layer:** browser refuses to send cookie due to SameSite, Secure-on-http,
or partitioned-cookies.
**Probe:**
1. `cookies` → check `same_site`, `secure`, `host_only`, `path`
2. `network --filter <api> --detail --headers` → response/request
3. `eval 'document.cookie'` (only non-httpOnly cookies; if mismatch with
   `cookies` output, the cookie is HttpOnly — expected)
**Conclude:** cookie present in `cookies` output but absent from request
`Cookie:` header → SameSite/Secure/path mismatch. Look for cross-site
context, `http:` origin with `Secure`, or `path` not covering the request
URL.
**Red herrings:** cookie is HttpOnly so `document.cookie` doesn't show it.
**Source:** `synth-bug`.

### A3. CSRF token mismatch
**Symptoms:** "form returns 403" / "CSRF token invalid" / "double-submit
cookie failing"
**Layer:** CSRF cookie not set, rotated mid-session, or missing header.
**Probe:**
1. `cookies` → look for `csrf`/`xsrf`/`_token`
2. `eval 'document.querySelector("meta[name=csrf-token]")?.content'` (Rails/Django pattern)
3. `network --filter <submit-url> --detail --headers` → check
   `X-CSRF-Token` / `X-XSRF-TOKEN` request header
**Conclude:** cookie token ≠ header/meta token, or header missing
entirely → CSRF flow broken.
**Source:** `synth-bug`.

### A4. Auth redirect loop
**Symptoms:** "infinite redirect to login" / "page never finishes
loading" / "URL keeps bouncing"
**Layer:** middleware loop — auth check redirects to login, login page
re-checks auth, redirects back.
**Probe:**
1. `network --filter "302|303|307|308" --limit 20 --detail`
2. Look at the `Location` chain
**Conclude:** ≥3 hops between two URLs in <2s → loop. Identify the pair
and check the auth middleware on each side.
**Source:** `synth-bug`.

---

## B. Network, API, CORS, mixed content

### B1. CORS preflight failure
**Symptoms:** "CORS error in console" / "blocked by CORS policy" / "OPTIONS
request fails"
**Layer:** server missing/incorrect CORS response headers, or preflight
returns non-2xx.
**Probe:**
1. `console --level error --filter "CORS|blocked"`
2. `network --filter "OPTIONS|<endpoint>" --detail --headers`
**Conclude:** preflight `OPTIONS` returns ≠ 204/200, or
`Access-Control-Allow-Origin` missing / wildcard with credentials. Name
the specific missing header.
**Red herrings:** confusing CORS with same-origin redirect (no
preflight); browser's CORS error message often misleads.
**Source:** `synth-tax`.

### B2. CSP blocking script / fetch / style
**Symptoms:** "Content Security Policy" / "refused to execute inline" /
"script-src violation"
**Layer:** CSP header rejects a resource the page tries to load.
**Probe:**
1. `console --level error --filter "Content-Security-Policy|CSP"`
2. `network --filter <main-document> --detail --headers` → read CSP
   header
3. Cross-reference violation source against directives
**Conclude:** named directive (e.g. `connect-src`) doesn't permit the
resource. Distinguish *functional* CSP violations from *probing* ones —
Zod v4's `try { new Function("") } catch` fires a violation event but the
code still works. Rule of thumb: if the violation message names a script
the page actually depends on (chunk hash, framework runtime), it's real.
**Red herrings:** Zod/feature-detect probes; browser-extension scripts.
**Source:** `dog-42`, `synth-tax`.

### B3. Mixed content
**Symptoms:** "image not loading on prod" / "request blocked" / "page
shows padlock with warning"
**Layer:** HTTPS page references HTTP resource.
**Probe:**
1. `console --filter "Mixed Content|insecure"`
2. `network --filter "http:" --limit 20` (note: many `http:` may be
   data URIs; filter on the `url` field)
**Conclude:** blocked URL is `http://` on an `https://` page → upgrade
required.
**Source:** `synth-tax`.

### B4. Wrong API base URL baked at build time
**Symptoms:** "production app calls localhost" / "API returns 404 in
prod" / "calls dev API from prod"
**Layer:** env-var inlined into bundle is wrong for the environment.
**Probe:**
1. `network --filter "/api|graphql|trpc" --limit 5 --detail`
2. Look at request URLs — compare to current `tab` origin
**Conclude:** request URL points to wrong host (localhost, staging) →
env var misconfigured. Check `NEXT_PUBLIC_*`, `VITE_*`, etc.
**Source:** `synth-bug`.

### B5. Request never fires
**Symptoms:** "button does nothing" / "no network activity on click" /
"submit goes to void"
**Layer:** click handler throws / preventDefault swallows / form has no
`action` / SPA router intercepts.
**Probe:**
1. `console --level error` (before action)
2. `click <selector> --wait-for-network ".*" --network-timeout 3000`
3. If no request matched: `eval 'getEventListeners?.(document.querySelector(SEL))'` (Firefox lacks `getEventListeners`; fall back to inspecting `onclick`/React fiber)
**Conclude:** zero new network entries + zero new console errors →
handler is a no-op. New console error → that's your bug.
**Red herrings:** stale network results (didn't drain); SPA-route
navigation looks like "no request" but is real.
**Source:** `dog-42`, `synth-bug`.

---

## C. Forms, events, interaction

### C1. React onChange not fired by direct value mutation
**Symptoms:** "I set the input but the form thinks it's empty" /
"`ff-rdp type` fills the field but the dropdown doesn't appear" / "value
shows in DOM but state is stale"
**Layer:** React tracks input value via a getter-setter on the native
prototype; `input.value = "x"` bypasses it.
**Probe:**
1. `type <selector> "value"` (current behaviour)
2. `eval 'document.querySelector(SEL).value'` (confirms DOM)
3. `eval 'React-app-specific state check'` or `snapshot` to verify UI
   reaction
**Conclude:** DOM value correct but autocomplete/state didn't react →
React doesn't see the event. Recommended fix: use a `type` variant that
dispatches the synthetic `input` event via the native value setter, or
document the workaround.
**Source:** `dog-36`, `dog-42`, `synth-bug`.

### C2. Custom (div/role-based) dropdown can't be clicked
**Symptoms:** "selector doesn't match the dropdown option" / "click
finds nothing" / "downshift/headlessui combobox unclickable"
**Layer:** options are `<div role="option">` rendered into a portal; no
stable CSS class.
**Probe:**
1. `snapshot --filter role=option` (or `dom '[role=option]'`)
2. Inspect rendered tree; pick the option by `aria-label` or text
3. `click '[role=option][aria-label="X"]'` or fall back to
   `eval 'document.querySelectorAll("[role=option]")[n].click()'`
**Conclude:** no role=option in tree → portal hasn't rendered, focus
the trigger first (`click <combobox>`); role=option present but
hidden — open via `keypress ArrowDown` after focusing.
**Source:** `dog-36`.

### C3. Consent / cookie banner intercepting clicks
**Symptoms:** "clicks do nothing" / "page is unresponsive" /
"`click` succeeds but no effect"
**Layer:** invisible consent overlay (cmp_*, OneTrust, Didomi)
captures pointer events.
**Probe:**
1. `eval 'document.elementFromPoint(window.innerWidth/2, window.innerHeight/2).tagName'`
2. `dom '[id*="onetrust"], [id*="didomi"], [class*="cmp"]' --limit 3`
3. `eval 'window.cmp_noscreen = true'` then retry; or `click` the consent
   accept-all button
**Conclude:** `elementFromPoint` returns the overlay, not the target.
**Source:** `dog-29`, `synth-bug`.

### C4. Form submits full-page reload (preventDefault missing)
**Symptoms:** "page flickers and resets" / "form reloads instead of
submitting via fetch" / "URL gains `?foo=bar` query string"
**Layer:** submit handler not attached, or doesn't `preventDefault`.
**Probe:**
1. `network --filter <current-page-url> --detail` after submit
2. Look for GET request to current page with form values in query
**Conclude:** full document reload via form's native action → SPA
handler not wired.
**Source:** `synth-bug`.

---

## D. Routing, SPAs, history

### D1. 404 on direct page load (history-mode SPA without server fallback)
**Symptoms:** "refreshing the page gives 404" / "share link doesn't
work" / "deep link broken"
**Layer:** server doesn't rewrite all paths to `index.html`.
**Probe:**
1. `network --filter <current-url> --detail --headers`
2. Check the *initial document* response status
**Conclude:** initial GET returns 404 with HTML 404 page → server config
missing SPA fallback rule.
**Source:** `synth-tax`.

### D2. Trailing-slash redirect breaks fetch
**Symptoms:** "API returns HTML instead of JSON" / "JSON.parse error" /
"unexpected token < at position 0"
**Layer:** request gets 301/302 to a path that returns HTML (login, 404).
**Probe:**
1. `console --level error --filter "JSON|parse|token <"`
2. `network --filter <failing-endpoint> --detail --headers`
3. Inspect Location chain
**Conclude:** request redirected to login or another HTML response.
**Source:** `dog-43`.

---

## E. Service workers & caching

### E1. ChunkLoadError after deploy
**Symptoms:** "ChunkLoadError" / "Loading chunk X failed" / "broken after
deploy" / "blank page until refresh"
**Layer:** browser caches old `index.html` referencing chunks that no
longer exist post-deploy.
**Probe:**
1. `console --level error --filter "ChunkLoadError|Loading chunk"`
2. `network --filter "/static/|/chunks/|/_next/" --detail` → identify
   404'd chunk
3. `eval 'navigator.serviceWorker?.getRegistrations()'` → check SW
   `updateViaCache`
**Conclude:** 404 on hashed chunk + stale `index.html` cached → deploy
skew. Fix is server-side (cache-control on `index.html`), not browser-side.
**Red herrings:** assuming SW is the cause when it's plain
HTTP-cache headers.
**Source:** `dog-43`, `synth-bug`.

### E2. Service worker serves stale shell
**Symptoms:** "old version showing after deploy" / "hard refresh doesn't
help" / "stuck on previous build"
**Layer:** SW `fetch` handler returns cached `/` response.
**Probe:**
1. `eval 'navigator.serviceWorker.getRegistrations()'`
2. `eval 'caches.keys()'`
3. `network --filter "^/$|index.html" --detail` — check `x-cache`/
   `service-worker` response source
**Conclude:** registration exists, document response served from SW (no
network entry, or `source=ServiceWorker`).
**Source:** `synth-bug`.

### E3. Manifest fetch returns HTML
**Symptoms:** "Manifest: Line: 1, column: 1, Syntax error" / "PWA install
prompt missing" / "manifest.json parse error"
**Layer:** unauthenticated `/manifest.webmanifest` redirects to login HTML.
**Probe:**
1. `console --filter "Manifest|webmanifest"`
2. `network --filter "manifest" --detail --headers`
**Conclude:** manifest URL returns 302 → HTML, or 200 with `text/html`.
**Source:** `dog-43`.

---

## F. Storage

### F1. localStorage quota exceeded
**Symptoms:** "saving fails silently" / "QuotaExceededError" / "settings
don't persist"
**Layer:** ~5MB origin quota hit (often analytics caches).
**Probe:**
1. `console --level error --filter "Quota|storage"`
2. `eval 'JSON.stringify(localStorage).length'`
3. `eval 'Object.fromEntries(Object.entries(localStorage).map(([k,v])=>[k,v.length])).sort'`
**Conclude:** total > 4_000_000 chars → quota pressure. Surface top 3
keys by size.
**Source:** `synth-tax`.

### F2. IndexedDB migration / version conflict
**Symptoms:** "page hangs on load" / "VersionError" / "blocked event"
**Layer:** open DB with bumped version while another tab holds the old
version.
**Probe:**
1. `console --level error --filter "IndexedDB|VersionError|blocked"`
2. `eval 'indexedDB.databases()'`
**Conclude:** error mentions `blocked` or `versionchange` → close other
tabs of the app, or app needs blocked-event handler.
**Source:** `synth-tax`.

---

## G. Hydration, SSR, build

### G1. Hydration mismatch
**Symptoms:** "warning: text content did not match" / "hydration
failed" / "client/server HTML differ"
**Layer:** server-rendered HTML ≠ client first render (locale, date,
random, window-only logic).
**Probe:**
1. `console --filter "hydration|did not match|did not expect"`
2. The console message itself names the offending element
**Conclude:** message identifies the diverging text/attribute. Common
causes: `Date.now()` / `Math.random()` in render, `window`-conditional
content, locale formatting (`toLocaleString` without explicit locale).
**Source:** `synth-bug`.

### G2. Env var not inlined at build
**Symptoms:** "feature flag stuck off" / "config undefined in prod" /
"NEXT_PUBLIC_ var empty"
**Layer:** env var missing at build, or not prefixed correctly for the
bundler (`NEXT_PUBLIC_`, `VITE_`, `PUBLIC_`).
**Probe:**
1. `eval 'Object.entries(window).filter(([k])=>k.includes("__NEXT_DATA__"))'` (Next.js: `__NEXT_DATA__.runtimeConfig`)
2. `eval 'import.meta.env'` (Vite — only in module context; mostly N/A in eval)
3. `eval` the specific config object the app exposes
**Conclude:** key missing or empty in the runtime config blob.
**Source:** `synth-bug`.

---

## H. Third-party, analytics, consent

### H1. Tag manager / analytics blocked
**Symptoms:** "GA not firing" / "events missing" / "tag fires in dev only"
**Layer:** ad blocker, CSP `connect-src`, or consent gate.
**Probe:**
1. `console --filter "blocked|ERR_BLOCKED|ublock"`
2. `network --filter "google-analytics|gtag|segment|amplitude" --limit 5`
**Conclude:** no request fired → blocked client-side; request with
`status=0` or `ERR_BLOCKED_BY_CLIENT` → ad blocker; CSP violation in
console → server CSP.
**Source:** `synth-tax`.

### H2. Stripe / payment iframe not loading
**Symptoms:** "card field blank" / "Stripe Elements not rendering" /
"iframe is empty"
**Layer:** CSP `frame-src` or `connect-src` missing Stripe's domains.
**Probe:**
1. `console --filter "Stripe|frame-src|connect-src"`
2. `dom 'iframe[src*=stripe]' --attrs`
3. `network --filter "stripe" --detail --headers`
**Conclude:** frame-src violation in console → CSP fix; iframe missing
entirely → Stripe.js not loaded.
**Source:** `synth-bug`.

---

## I. Layout, images, fonts

### I1. Image broken / not loading
**Symptoms:** "image is broken icon" / "404 on image" / "lazy image
never loads"
**Layer:** wrong src, CORS, lazy-loading + viewport, format unsupported.
**Probe:**
1. `dom 'img[src=""], img:not([src])' --attrs --limit 5`
2. `network --filter "\\.(png|jpg|webp|avif|svg|gif)" --detail` (status≠200)
3. `eval 'Array.from(document.images).filter(i=>!i.complete||i.naturalWidth===0).map(i=>i.src)'`
**Conclude:** identifies broken `src`. Distinguish 404 vs CORS (`status=0`
+ console "CORS").
**Source:** `synth-tax`.

### I2. Font flash / FOIT / wrong glyph
**Symptoms:** "fonts swap mid-load" / "text in fallback font" / "icon
font shows boxes"
**Layer:** font 404, CORS, missing `crossorigin`, wrong unicode-range.
**Probe:**
1. `network --filter "\\.(woff2?|ttf|otf)" --detail --headers`
2. `eval 'document.fonts.status'` / `eval '[...document.fonts].map(f => [f.family, f.status])'`
3. `console --filter "OTS|font|preload"`
**Conclude:** status `unloaded` for the expected font, or 404.
**Source:** `synth-tax`.

### I3. Iframe blocked by X-Frame-Options / frame-ancestors
**Symptoms:** "embedded page won't render" / "refused to display in
frame" / "blank iframe"
**Layer:** target site sends `X-Frame-Options: DENY/SAMEORIGIN` or
`Content-Security-Policy: frame-ancestors`.
**Probe:**
1. `console --filter "frame-ancestors|X-Frame-Options|refused"`
2. `network --filter <iframe-src> --detail --headers`
**Conclude:** named response header blocks framing.
**Source:** `synth-tax`.

### I4. Mobile viewport missing → desktop layout on phone
**Symptoms:** "site renders zoomed-out on mobile" / "text tiny on phone"
/ "responsive breakpoints don't kick in"
**Layer:** `<meta name=viewport>` missing.
**Probe:**
1. `dom 'meta[name=viewport]' --attrs`
2. `responsive 'body' --widths 375 --include-hidden` (post iter-56 B2)
**Conclude:** viewport meta absent or missing `width=device-width`.
**Source:** `synth-tax`.

---

## J. Performance & vitals

### J1. Slow LCP — what's the LCP element
**Symptoms:** "page feels slow" / "LCP score bad" / "first paint OK but
biggest element is late"
**Layer:** LCP candidate is a late-loading image, font, or render-blocked.
**Probe:**
1. `perf vitals` → check `lcp_ms` and `lcp_element` (if exposed)
2. `eval 'performance.getEntriesByType("largest-contentful-paint").at(-1)'`
3. `network --filter <lcp-resource>` → see when it started
**Conclude:** identifies the LCP node and resource; suggest preload or
priority hint.
**Source:** `synth-tax`.

### J2. Excessive layout thrash on interaction
**Symptoms:** "input lag" / "scrolling janky" / "INP score bad"
**Layer:** synchronous layout in event handler.
**Probe:**
1. `perf audit`
2. `console --filter "Forced reflow|long task"`
**Conclude:** "Forced reflow while executing JavaScript" warnings name
the script; long-task entries point at the handler.
**Source:** `synth-tax`.

---

## K. Fallback

### K0. Unknown symptom — broad sweep
**Symptoms:** the user can't characterize the problem.
**Probe (in order, stop at first signal):**
1. `console --level error --limit 10`
2. `network --status "4..|5.." --limit 10 --detail`
3. `snapshot --interactive-only --limit 30` (orient the agent on the
   page)
4. Ask the user: did anything trigger this? (reload, click, route change)
**Conclude:** hand back the three results with a one-line read on
"this looks like an X-flavored bug" and prompt for a more specific
playbook.

---

## Coverage check (source-4 taxonomy)

Cross-checked against Chrome DevTools "Issues" categories + MDN error
references. Not yet covered, candidates for v0.5:

- **Web Worker / SharedWorker errors** — uncommon in app code; low priority.
- **WebSocket dropped / no reconnect** — useful for chat/realtime apps.
- **`postMessage` cross-origin** — niche, mostly embeds.
- **Permissions API denials (camera, mic, notifications)** — covered by
  `console` filter naturally.
- **Cross-Origin-Opener-Policy / Cross-Origin-Embedder-Policy** —
  high-pain when it bites (SharedArrayBuffer apps) but very rare.
- **Trusted Types violations** — only certain enterprise stacks.

## Adversarial pass (source-5)

Symptom phrasings none of the above playbooks handle well:

1. "the page is fine but feels slow only after I click around for a
   while" → **memory leak / detached DOM nodes**. No playbook. Probably
   out of scope for ff-rdp (needs heap snapshot, not RDP).
2. "the build works locally but the prod bundle is missing a route" →
   **dead-code elimination / tree-shake** issue. Static, not a
   browser-side debug. Out of scope.
3. "intermittent — works on second try" → **race condition**. Most
   relevant subcase is auth (A4) or fetch retry. Add an *episode*
   instructing the skill to re-run the failing action 3× and diff.
4. "looks different on staging vs prod" → **env-specific config / feature
   flag**. Subsumed by B4 + G2 + H1; add a top-level "env compare"
   playbook that runs B4 against both URLs side-by-side. **Open gap.**
5. "an entire section just isn't rendering" → could be feature flag,
   permission gate, conditional `null` return in React. Suggest a
   playbook: `snapshot` the expected section's parent → if children are
   `[]`, look at the *closest data fetch* in network. **Open gap.**
6. "user reports it but I can't reproduce" → not a single playbook;
   suggest the skill emit a **repro template** of the commands the user
   should run on their machine and paste back. Meta-playbook.

## Open gaps after corpus + bug-report + adversarial passes

- **Memory leak playbook** — needs heap-snapshot affordance ff-rdp doesn't
  have. Defer; possibly never; note in skill `--help`.
- **Env compare playbook** — running the same probe sequence against
  staging + prod and diffing. Worth a v0.5 slot.
- **"Section missing" playbook** — see (5). v0.5 slot.
- **Race / flaky** — see (3). Could be a `--retry N` flag on the skill
  itself rather than a playbook.

## Playbook prioritization for v0

Tier 1 (must-have, real-bug evidence in dogfooding):

A1 Set-Cookie stripped · B5 Request never fires · C1 React onChange ·
C2 Custom dropdown · C3 Consent banner · E1 ChunkLoadError ·
E3 Manifest HTML · D2 Trailing-slash JSON parse · K0 Fallback

Tier 2 (high-likelihood synthesised, easy to write fixtures for):

A2 SameSite · A3 CSRF · A4 Redirect loop · B1 CORS · B2 CSP · B4 Wrong
API URL · F1 localStorage quota · G1 Hydration · I1 Broken image

Tier 3 (less common; add as fixture availability allows):

B3 Mixed content · C4 Form full-reload · D1 SPA 404 · E2 SW shell stale ·
F2 IDB version · G2 Env var · H1 Analytics blocked · H2 Stripe iframe ·
I2 Fonts · I3 X-Frame · I4 Mobile viewport · J1 LCP element · J2 Layout
thrash

That's 32 playbooks across three tiers. Tier 1 (9) is the v0 ship target
— each has a documented dogfooding source or trivial fixture.

## References

- [[dogfooding/dogfooding-session-29]] — consent overlay (C3)
- [[dogfooding/dogfooding-session-36-ff-rdp]] — React autocomplete, custom
  dropdown (C1, C2)
- [[dogfooding/dogfooding-session-42]] — Set-Cookie strip, React event,
  fallback flow (A1, B5, C1)
- [[dogfooding/dogfooding-session-43]] — ChunkLoadError, Manifest, JSON
  redirect (D2, E1, E3)
- [[iterations/iteration-57-dogfood-42-fixes]] — `--headers` and
  `--wait-for-network` are load-bearing for A1, A2, B5
