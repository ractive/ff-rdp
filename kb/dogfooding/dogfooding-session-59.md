---
title: Dogfooding Session 59 — fresh target (MDN), verify iter-88/89/91, exercise run/record/index
type: dogfooding
date: 2026-05-31
status: completed
site: developer.mozilla.org, example.com
commands_tested: [doctor, launch, navigate, page-text, screenshot, cascade, computed, styles, perf vitals, perf summary, perf audit, a11y contrast, dom stats, snapshot, network, click, type, scroll, eval, run, record, index, geometry, responsive, cookies, storage, sources, console, daemon stop]
tags: [dogfooding, iter-88, iter-89, iter-91, regression-verification, csp, screenshot-full-page, run, index]
---

# Dogfooding Session 59

Fresh target — developer.mozilla.org (React-heavy, strict CSP). Confirmed iter-88 (cascade) and iter-89 (screenshot capture) landed, but found that `--full-page` is silently ignored, `eval` is unusable on CSP-strict sites, and the `run`/`index` paths surface a `dom-complete` miss that the direct `navigate` masks.

## What's New Since Last Session (session 58)
- iter-88 — cascade: parse `CSSStyleRule` sentinel (`type: 100` / `className: "CSSStyleRule"`)
- iter-89 — screenshot: route through `WindowGlobalTarget` on FF 151
- iter-91 — persistent main worktree + SHA-keyed result cache (infra; not directly user-facing)
- New commands surfaced in `--help`: `doctor`, `run`, `record`, `index`

## Regression Checks
| Theme (origin) | Previous Status | Current Status | Notes |
|---|---|---|---|
| cascade returns rules on real site (iter-83/84/88) | broken in 57; "fixed" in 88 | ✅ **FIXED on MDN** | `cascade h1 --prop font-size` returns 1 author rule; raw shows `className: "CSSStyleRule"` + `matchedSelectorIndexes` (iter-88 parse landed) |
| screenshot captures PNG on FF 151 (iter-89) | broken | ✅ **FIXED** | `screenshot -o file` writes a valid 1366×683 PNG (240 KB) |
| `--full-page` captures full scroll height (iter-83 Theme B AC) | partial | ❌ **STILL BROKEN** | `screenshot --full-page` produces byte-identical PNG to viewport-only on a 6.6 KB-text MDN page (md5 collision) — flag is silently ignored |
| daemon stop frees port (iter-86 Theme A) | broken | ⚠️ **partial** | Reports "still listening after 3s" then port is free shortly after — error message correct but timing window remains |
| `--jq` missing-path policy (iter-86 Theme D) | n/a | ✅ **WORKS** | Default silently omits; `--jq-strict` errors with clear message |
| `perf vitals` LCP unavailable (iter-83 Theme F) | broken | ✅ **WORKS** | `lcp_ms: null, lcp_rating: "unavailable"` on MDN |
| `doctor` reports state | new | ✅ **WORKS** | Clean pass/fail table; sensible hints when port is free vs busy |

## Smoke Test Results
| Command | Status | Notes |
|---|---|---|
| `launch --headless --auto-consent` | ✅ | clean |
| `doctor` | ✅ | 5/5 pass after launch |
| `navigate https://developer.mozilla.org/...` | ⚠️ | reports `elapsed_ms: 4` but wall-clock 7.2s — likely returning on pre-existing readyState |
| `page-text` | ✅ | 6636 chars, sensible content |
| `screenshot -o` | ✅ | iter-89 fix confirmed |
| `screenshot --full-page` | ❌ | identical to viewport (see below) |
| `cascade h1 --prop font-size` | ✅ | iter-88 fix confirmed |
| `cascade h1 --prop color` | ⚠️ | empty rules — *correct* (color is inherited, no author rule) but indistinguishable from broken cascade |
| `computed h1 --prop color --prop font-size` | ✅ | clean |
| `styles h1 --applied` | ✅ | 2 rules |
| `perf vitals/summary/audit` | ✅ | all populated |
| `a11y contrast --fail-only` | ✅ | 2 real WCAG AA failures found, with selectors + ratios |
| `dom stats` | ⚠️ | `render_blocking_count: 22` but `perf audit render_blocking: 17` — same page, different numbers |
| `snapshot` | ✅ | tag=html, structured tree |
| `network --format text` (immediately post-nav) | ⚠️ | "Total transferred: 0 bytes" and bare numbers under section headers because `cause_type`/`transfer_size` are still null |
| `network --format text` (after settle) | ✅ | clean table; 81 requests grouped by type |
| `geometry h1` | ✅ | rect + computed visibility + in_viewport |
| `responsive .main-page-content --widths 320,768,1024` | ✅ | 3 breakpoints captured |
| `cookies` / `storage` / `sources` / `console` | ✅ | empty results but no errors |
| `click 'input[type=search], [aria-label*="Search" i]'` | ⚠️ | timeout — MDN search likely needs first-click to open; comma-list + `i` flag may not be supported |
| `scroll bottom` | ✅ | works |
| `scroll to 'footer'` | ❌ | "rect did not stabilise after 10000ms" — MDN footer may be lazy-loaded |
| `record start/stop` | ✅ | clean JSON output with `$schema` |
| `run <script>` | ❌ | navigate step times out at 10s on example.com (see below) |
| `index https://example.com --max-pages 5` | ❌ | same dom-complete miss → 0 pages crawled |
| `eval` on MDN | ❌ | CSP blocks (see below) |
| `eval` on example.com | ✅ | works |
| `daemon stop` | ⚠️ | "still listening after 3s" then frees |

## Findings

### What Works Well
- **`doctor`** is a delight — five clean checks, color glyphs, actionable hints. First command an agent should run on connect.
- **`record` → `run`** round-trip produces a clean `$schema`-referenced JSON. The schema URL is a nice touch.
- **`a11y contrast`** is the most polished command in the suite. Real WCAG numbers, selectors, sample text. Found 2 genuine AA failures on MDN.
- **`network --format text`** (once enriched) is genuinely readable — no need to reach for jq for a quick eyeball.
- **`responsive`** worked silently and correctly on all 3 widths.
- **`--jq-strict`** (iter-86) gives a crisp error vs the default silent-omit. Exactly what I want.

### Issues Found

1. **`screenshot --full-page` is silently ignored.**
   - Cmd: `ff-rdp screenshot --full-page -o /tmp/df59-fp.png` on MDN /Web/JavaScript
   - Result: 1366×683 PNG (282 830 bytes), **byte-identical md5 to viewport screenshot**
   - Expected: PNG ≥ scrollHeight × DPR (iter-83 Theme B AC: "live_screenshot_full_page: PNG height ≥ scrollHeight × DPR")
   - The iter-89 fix repaired the viewport capture but the `--full-page` branch is now a no-op. Likely the WindowGlobalTarget routing dropped the `fullPage: true` option.

2. **`eval` is blocked by CSP on MDN (and any strict-CSP site).**
   - Cmd: `ff-rdp eval 'document.title'` on developer.mozilla.org
   - Result: `error: call to eval() blocked by CSP` with `class: "EvalError"` at `@debugger eval code:1:36`
   - Expected: Firefox DevTools console can evaluate on MDN — the protocol *can* bypass page CSP via the debugger sandbox; ff-rdp's eval path appears to inject through a `script` element instead
   - Impact: Huge. `eval` is the universal escape hatch; without it agents can't read `window.scrollY`, custom globals, framework state on a large swath of modern sites
   - Workaround today: stick to typed commands (`computed`, `geometry`, `dom`) — but discovery becomes painful

3. **`run` and `index` hit dom-complete misses that direct `navigate` masks.**
   - `ff-rdp navigate https://example.com` returns instantly with `elapsed_ms: 0, ready_state: "complete"` after the first load (clearly observing pre-existing readyState, not the new nav)
   - `ff-rdp run` and `ff-rdp index` use a different code path that actually awaits dom-complete — and times out at 10s on the same URL/tab
   - Either (a) `navigate` is over-optimistic and should also fail, or (b) `run`/`index` are over-strict and should accept readystate complete. The mismatch makes scripts and crawls unreliable while interactive use looks fine.

4. **`dom stats render_blocking_count` ≠ `perf audit render_blocking`.**
   - Same page (MDN /Web/JavaScript), same daemon, seconds apart
   - `dom stats` → 22, `perf audit` → 17
   - iter-86 Theme C tightened `perf audit`'s filter (exclude favicons); `dom stats` evidently uses a different counting rule. Pick one and align.

5. **`cascade` empty rules indistinguishable from broken.**
   - `cascade h1 --prop color` on MDN returns `rules: []` — correct (color is inherited, no h1 author rule sets it)
   - But this is byte-identical to the iter-82/83/84/85 broken-cascade output that session 57/58 spent dozens of pages debugging
   - Suggest adding a `note: "no author rule declares this property; computed value is inherited or default"` when rules is empty but computed is non-null

6. **`network --format text` prints bare numbers when fields are null.**
   - Immediately post-nav, `cause_type` and `transfer_size` are still streaming in. Output shows `Total transferred: 0 bytes` then a `Requests by Cause Type` table with a single row `      82` (no label).
   - Either (a) suppress section if all values are null/empty, or (b) print `(unknown)` for null group keys.

7. **`navigate` reports `elapsed_ms: 0` after the first nav.**
   - Looks like it's checking the existing `document.readyState` and short-circuiting — but the page hasn't navigated yet (DOM still old).
   - Subtle correctness bug: a second `navigate` to the same URL returns "complete" before the new commit. Confuses agents using `elapsed_ms` as a perf signal.

8. **`daemon stop` race window still exists (iter-86 Theme A only partially closed).**
   - Reports "port 6000 still listening after 3s" then port is free a fraction-second later. Either bump the wait or pkill the residual process.

### Feature Gaps
- **`eval --no-csp` (or `eval --via-debugger`)** — use the DevTools console sandbox so CSP doesn't apply. Mirrors what Firefox's web console actually does.
- **`screenshot --full-page --strict`** — fail loud if the captured height < scrollHeight, instead of silently returning the viewport.
- **Composite-command parity**: `run` and `index` should reuse the same nav routine as `navigate` (or both should fail when the other does).
- **`cascade` could emit `inherited_from`** when a property is inherited rather than declared, to disambiguate empty rules.

## Summary
- ~30 commands exercised against MDN (strict CSP, React SPA) + example.com fallbacks.
- **Wins**: iter-88 cascade fix and iter-89 screenshot fix both confirmed on a fresh target. `doctor`, `record`/`run` schema, `a11y contrast` shine.
- **Top regressions to file as iter-92**: (1) `screenshot --full-page` silently no-op, (2) `eval` unusable on CSP-strict sites, (3) `run`/`index` dom-complete miss vs `navigate` masking it.

Links: [[dogfooding-session-58]] · [[iteration-88-cascade-fifth-attempt-single-theme]] · [[iteration-89-screenshot-fifth-attempt-single-theme]] · [[iteration-86-perf-field-report-fixes]]
