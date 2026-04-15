---
title: "Dogfooding Session 36: ff-rdp - comparis.ch"
date: 2026-04-09
type: dogfooding
status: completed
site: comparis.ch
commands_tested:
  - navigate
  - eval
  - screenshot
  - page-text
  - dom
  - dom stats
  - click
  - type
  - wait
  - perf vitals
  - perf summary
  - perf audit
  - a11y
  - a11y contrast
  - snapshot
  - styles
  - geometry
  - responsive
  - sources
  - cookies
  - storage
  - back
  - recipes
  - llm-help
  - console
---

# Dogfooding Session 36: ff-rdp - comparis.ch

## Objective

Full end-to-end user journey: find a 3-room apartment in Zurich on comparis.ch, then perform comprehensive performance, accessibility, and structural analysis of the results page.

## User Journey

### Phase 1: Navigation and Search

1. **Navigate to comparis.ch** -- Worked flawlessly. `navigate` with `--timeout 15000` was smooth.
2. **Suppress consent** with `eval 'window.cmp_noscreen = true'` -- Works as expected.
3. **Screenshot homepage** -- Initially failed because I passed the path as a positional arg instead of `-o`. The error message was clear though.
4. **DOM query for immobilien links** -- `dom 'a[href*="immobilien"]'` returned 22 results with full HTML. Worked well.
5. **Navigate to immobilien search page** -- Direct URL navigation worked.
6. **Type in search form** -- `type 'input[name="LocationSearchString"]' "Zurich"` successfully typed into the location field. However, the React autocomplete dropdown did NOT appear, because `type` uses value-setting rather than native keyboard events. This is a known limitation.
7. **Dropdown interaction** -- Could not interact with custom React dropdowns (Rooms, Price, etc.) via the CLI. These use `downshift` library with `combobox` role, not native `<select>` elements.
8. **Workaround: Direct URL** -- Constructed the search URL with query parameters manually and navigated directly. This worked perfectly and returned 194 results for 3-room apartments in Zurich.

### Phase 2: Browsing Results

9. **Page text extraction** -- `page-text` returned the full visible text including all 5 listings on page 1 with prices, addresses, room counts. Very useful for LLM consumption.
10. **Scroll via eval** -- `eval 'window.scrollTo(0, 600)'` worked. Screenshot confirmed scrolled view showing listings.
11. **DOM query for listing links** -- `dom 'a[href*="/immobilien/marktplatz/details/"]'` found all listing links. Output was very verbose (full HTML with images).
12. **Navigate to detail page** -- Direct navigation to a listing worked. Screenshot showed property images, floorplan.
13. **Back navigation** -- `back` command returned to results page correctly.

### Listings Found

| Property | Location | Price/month |
|---|---|---|
| Gewerbeobjekt, 3 Zi, 130 m2, EG | 8050 Zurich, Siewerdtstrasse | CHF 3'400 |
| Gewerbeobjekt, 3 Zi, 120 m2, 2. OG | 8064 Zurich, Bandlistrasse 31 | CHF 350 |
| Wohnung, 3 Zi, 103 m2, 1. OG | 8051 Zurich, Stettbacherrain 12 | CHF 3'570 |
| Wohnung, 3 Zi, 102 m2, EG | 8044 Zurich, Freudenbergstrasse 105 | CHF 3'790 |
| Wohnung, 3 Zi, 100 m2, 1. OG | 8053 Zurich, Stodolastrasse 15 | CHF 2'519 |

## Phase 3: Performance Analysis

### Core Web Vitals (Results Page)

| Metric | Value | Rating |
|---|---|---|
| FCP | 1361 ms | Good |
| LCP | 0 ms (approx) | Good* |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |
| TTFB | 1250 ms | Needs Improvement |

*LCP is estimated via DOM approximation in headless Firefox -- not fully reliable.

### Core Web Vitals (Detail Page)

| Metric | Value | Rating |
|---|---|---|
| FCP | 972 ms | Good |
| LCP | 1360 ms | Good |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |
| TTFB | 798 ms | Good |

### Resource Summary (Results Page)

- **Total requests**: 179
- **Total transfer size**: 58,797 bytes (measured by Performance API; actual likely higher due to cached resources)
- **Third-party requests**: 113 out of 179 (63%)
- **By type**: 77 JS, 52 images, 27 XHR, 12 other, 6 documents, 3 fonts, 2 CSS

### Top 5 Slowest Resources

1. comparis.ch portal list API -- 1218 ms (11 KB)
2. doubleclick.net gpt.js (ad library) -- 1094 ms
3. comparis.ch favicon.ico -- 1088 ms
4. teads.tv ad tag -- 889 ms
5. tiktok analytics pixel -- 797 ms

### Third-Party Domain Breakdown (40 unique domains!)

Heavy tracker/ad presence:
- `dt.adsafeprotected.com` -- 34 requests
- `nfahomefinder.b-cdn.net` -- 48 requests (images)
- `bat.bing.com` -- 6 requests
- `analytics.tiktok.com` -- 6 requests
- `troubadix.data.comparis.ch` -- 6 requests
- `c.lytics.io` -- 5 requests
- `connect.facebook.net` -- 2 requests
- `region1.analytics.google.com` -- 3 requests
- `securepubads.g.doubleclick.net` -- 3 requests
- And 30+ more tracking/ad domains

### DOM Statistics

| Metric | Results Page | Detail Page |
|---|---|---|
| DOM nodes | 1989 | 2099 |
| Document size | 657 KB | 658 KB |
| Inline scripts | 17 | 15 |
| Render-blocking | 6 | 33 |
| Images without lazy | 3 | 25 |

### Navigation Timing (Results Page)

- responseStart: 1100 ms
- domInteractive: 1198 ms
- domContentLoaded: 1367 ms
- domComplete: 1748 ms
- loadEvent: 1750 ms
- Transfer size: 138 KB (navigation document)
- Decoded body: 473 KB

## Phase 4: Accessibility Analysis

### Contrast Check
- All 7 tested navigation text elements pass AA (normal and large) with ratio 5.32 (#017b4f on #ffffff).
- No elements failed `--fail-only` check (note: headless mode limits what elements are "visible" for contrast checking).

### Semantic Structure
- Proper use of `<header>`, `<main>`, `<footer>`, `<nav>` landmarks.
- Breadcrumb navigation with `aria-label="breadcrumbs"`.
- Footer has named navigation: `aria-label="Footer menu"`.
- Route announcer present (`role=alert`).
- Status message area (`role=status`).
- However: many hidden `<header>` elements (visibility: hidden) detected by `geometry` -- these are likely for different viewport breakpoints but are rendered in DOM.

### Interactive Elements
- Links, buttons properly structured in the a11y tree.
- `combobox` role used for custom dropdowns.
- Form inputs have associated labels.

## Phase 5: Structural Analysis

### Page Snapshot
- Clean Next.js structure: `<div id="__next">` wrapping everything.
- 9 `ReactModalPortal` divs rendered (even when no modals are open).
- Route announcer and status areas present.

### CSS Patterns
- Emotion CSS-in-JS (class names like `css-1k1piby`).
- Font Awesome 6 (Pro, Free, Sharp, Duotone variants -- 17 font-face declarations).
- Swiper carousel integration.
- `text-size-adjust: none` applied.

### Cookies and Storage
- Extensive cookie usage with consent management (`__cmpconsentx102256_`).
- localStorage stores: consent state, Bing UET IDs, Lytics segments, ad session data, contact form field state.
- Multiple tracker IDs persisted.

## ff-rdp Tool Observations

### What Works Well

1. **`perf audit`** -- Excellent single-command overview. The "Flagged Issues" section is immediately actionable. The text format is clean and readable.
2. **`page-text`** -- Very effective for understanding page content. Great for LLM consumption.
3. **`dom` queries** -- CSS selector support is solid. Finding elements by attribute patterns (`href*=`, `data-test=`) works well.
4. **`screenshot -o`** -- Reliable. Fast. Good default viewport size.
5. **`a11y contrast`** -- Clean table output. `--fail-only` filter is practical.
6. **`perf summary`** -- Domain breakdown and resource type counts are very useful for identifying bloat.
7. **`back`/`forward`** -- Simple and work correctly.
8. **`cookies` and `storage`** -- Good for understanding what tracking is happening.
9. **`snapshot`** -- Excellent for quick page understanding with semantic structure.
10. **`recipes`** -- Very helpful for discovering jq patterns. Great onboarding tool.
11. **`--format text`** -- Consistent and readable across commands.

### Pain Points and Improvement Ideas

1. **`type` command doesn't trigger React autocomplete** -- The `type` command sets the value but React's synthetic event system doesn't see it as keyboard input. The autocomplete dropdown for location search never appeared. **Suggestion**: Add a `--native` flag that dispatches actual KeyboardEvent sequences instead of setting `.value` directly.

2. **`screenshot` positional arg confusion** -- I initially tried `screenshot /tmp/file.png` (positional) but it requires `-o /tmp/file.png`. The error message was clear, but this is a common enough pattern that a positional arg could be nice.

3. **`network` daemon timeout** -- `network --format text --timeout 10000` timed out with "receiving drain response from daemon". Had to use `--no-daemon` or switch approaches. Network via daemon seems less reliable than other commands.

4. **`wait` command timeout mismatch** -- The `--timeout` flag controls connection timeout, but `--wait-timeout` controls how long to wait for the condition. I used `--timeout` thinking it was the wait timeout. The error message mentions `--wait-timeout` which helped, but the distinction could be clearer in `--help`.

5. **`dom` output verbosity** -- When querying `a[href*="details"]`, the full HTML including SVG icons and image srcsets made the output very long. **Suggestion**: A `--compact` mode that strips inline SVGs, srcsets, and style attributes would be very useful.

6. **`eval` returns `{"type": "undefined"}`** -- When calling `window.scrollTo()` which returns void, the output is `{"type": "undefined"}`. This is technically correct but could be `null` or omitted for cleaner output.

7. **No `text` top-level command** -- I initially tried `ff-rdp text` (doesn't exist) before discovering `page-text`. The naming is slightly unintuitive.

8. **`perf vitals` jq path** -- I tried `--jq '.results.vitals'` but vitals are at `.results` directly (flat). The error message was huge because it dumped the entire results array. **Suggestion**: Truncate jq error messages to avoid dumping massive JSON payloads.

9. **`a11y` fallback message** -- "accessibility walker root methods unrecognized in this Firefox version" appears every time. Since this is a known limitation, consider suppressing the debug message or making it a `--verbose` only output.

10. **`sources` thread actor error** -- "sources thread actor failed...falling back to JS DOM/Performance API" appears on every call. Same suggestion: suppress by default.

11. **Custom dropdown interaction** -- No way to interact with React/downshift dropdowns. Would need a higher-level `select` command that opens the dropdown, waits for options, and clicks one.

12. **Missing `scroll` command** -- Had to use `eval 'window.scrollTo(0, 600)'` to scroll. A dedicated `scroll` command (or `scroll-to-element <selector>`) would be useful.

## comparis.ch Site Findings

### Performance Concerns
- **63% of requests are third-party** (113 of 179). Massive tracker overhead.
- **40 unique third-party domains** contacted. Each domain requires DNS lookup + TLS handshake.
- **TTFB of 1250ms** on results page needs improvement (server-side rendering could be optimized).
- **Detail page has 33 render-blocking resources and 25 non-lazy images** -- significant optimization opportunity.
- **Ad library (gpt.js) is in the top 5 slowest resources** at 1094ms.

### Tracking Overload
TikTok, Facebook, Bing, Google Analytics, Hotjar, Clarity, Reddit, Lytics, Convert Experiments, Teads, DataDome, AdSafe, DoubleClick -- at least 15 different tracking/analytics systems.

### Structural Issues
- 9 empty `ReactModalPortal` divs rendered in the DOM.
- Multiple hidden `<header>` elements for responsive variants (should use CSS media queries instead of rendering all variants).
- 657 KB document size is quite large for a listing results page.

### Positive Aspects
- Good semantic HTML (proper landmarks, nav labels, breadcrumbs).
- Contrast ratios pass WCAG AA.
- Next.js provides good SSR support.
- Images use srcset for responsive images.
- CLS score is 0 -- good layout stability.
