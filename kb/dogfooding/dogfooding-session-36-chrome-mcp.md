---
title: "Dogfooding Session 36: Chrome MCP - comparis.ch"
date: 2026-04-09
type: dogfooding
tags:
  - dogfooding
  - chrome-mcp
  - comparis
  - performance
  - accessibility
status: completed
---

# Dogfooding Session 36: Chrome MCP Browser Automation on comparis.ch

## Objective

Test Chrome MCP browser automation tools by performing a real user journey on comparis.ch: searching for a 3-room apartment in Zurich, then analyzing page performance and HTML structure.

## Phase 1: Navigation and Apartment Search

### Steps Taken

1. **Navigate to comparis.ch** -- Used `navigate` tool. Page loaded successfully ("Vergleichen und sparen -- comparis.ch"). Took ~2s.
2. **Suppress cookie consent** -- `window.cmp_noscreen = true` via `javascript_tool`. Worked immediately, no consent banner appeared.
3. **Screenshot homepage** -- `computer` screenshot action. Clean homepage with nav bar, hero section, and quick links including "Immobilie finden".
4. **Click "Immobilie finden"** -- Used `computer` left_click on the quick link card. Navigated to `/immobilien/default`.
5. **Fill search form** -- This was the most complex step:
   - Location field (`ref_14`): Clicked and typed "Zurich" using `computer` type action. No autocomplete dropdown appeared (interesting -- may need delay or specific trigger).
   - Zimmer dropdowns: `form_input` failed because the comboboxes are custom `<button>` elements, not native `<select>`. Had to use `javascript_tool` to find the `[role="listbox"]` elements and click the correct `[role="option"]` programmatically.
   - Set "Zimmer von" = 3 and "Zimmer bis" = 3 via JS DOM manipulation.
6. **Submit search** -- Clicked "Inserate anzeigen" button. Navigated to results page.
7. **Results loaded** -- 194 results for 3-Zimmer apartments in Zurich. First listing: furnished apartment at CHF 2,500/month.

### Pain Points (Phase 1)

- **Custom comboboxes**: `form_input` does not support button-based custom dropdowns (returns "Element type BUTTON is not a supported form input"). Required JavaScript workaround via DOM querying of `[role="listbox"]` and `[role="option"]`.
- **Dropdown scrolling**: The dropdown lists were clipped to the viewport. Scrolling inside the dropdown was unreliable with `computer` scroll -- the page scrolled instead of the dropdown. JS click was the only reliable approach.
- **No autocomplete interaction**: The location field had no visible autocomplete, though the search still worked with plain text input.

### What Worked Well (Phase 1)

- `navigate` was fast and reliable.
- `computer` screenshot gave clear, high-res images for verification.
- `javascript_tool` was the escape hatch for anything the higher-level tools could not handle.
- `find` tool was excellent at locating semantic elements like "apartment listing with price" and "pagination or next page button".
- `read_page` with `filter: interactive` gave a clean list of all form controls.

## Phase 2: Performance Analysis

### Core Web Vitals

| Metric | Value | Rating |
|--------|-------|--------|
| TTFB | 1,416 ms | Needs improvement (>800ms) |
| FCP | 1,576 ms | Good (<1,800ms) |
| LCP | 1,800 ms | Good (<2,500ms) |
| CLS | 0.009 | Good (<0.1) |
| FID | Not measured (no user input during observation) |
| DOM Interactive | 1,563 ms | |
| DOM Content Loaded | 1,714 ms | |
| Load Complete | 3,974 ms | |

### Navigation Timing Details

- **Document transfer size**: 133 KB compressed, 467 KB decompressed (3.5x compression ratio)
- **No redirects** on the search results page
- **DNS/Connect**: 0ms (cached/keep-alive)
- **Response download**: 27ms (fast)

### Resource Loading

| Resource Type | Count | Avg Duration |
|---------------|-------|-------------|
| Scripts (src) | 45 | 45ms |
| Links (preload/prefetch) | 23 | 12ms |
| Images | 36 | 200ms |
| Fetch/XHR | 21 | ~180ms |
| Iframes | 5 | 122ms |
| Beacons | 6 | 51ms |
| CSS | 1 | 2ms |

**Total resources**: 137

### Script Loading Strategy

- **0 render-blocking scripts** -- all scripts use `async` (15) or `defer` (31)
- **0 module scripts** -- classic script loading only
- **15 inline scripts** -- used for configuration/initialization
- **2 stylesheets**: main app CSS from BunnyCDN, plus Lytics pathfora CSS

### Third-Party Domain Analysis (Top 10)

| Domain | Requests | Purpose |
|--------|----------|---------|
| nfahomefinder.b-cdn.net | 48 | App bundles (Next.js chunks) |
| dt.adsafeprotected.com | 23 | Ad safety verification |
| www.comparis.ch | 8 | First-party API calls |
| assets-comparis.b-cdn.net | 5 | Image CDN |
| c.lytics.io | 5 | Analytics |
| t.teads.tv | 4 | Ad network |
| contentpages-prd.b-cdn.net | 3 | CMS content |
| securepubads.g.doubleclick.net | 3 | Google ads |
| a.teads.tv | 3 | Ad network |
| troubadix.data.comparis.ch | 3 | Internal analytics |

### Network Traffic Post-Load

After initial page load, 17 network requests captured were almost entirely ad-related:
- Google ad sodar beacons
- adsafeprotected.com viewability tracking (majority of requests)
- DataDome bot protection
- No significant first-party API calls after initial load

### Key API Endpoints

- `/immobilien/api/v1/singlepage/portallist` (11 KB) -- main listing data
- `/Comparis/api/V1/TargetingContext/context` (1 KB) -- ad targeting
- `/immobilien/node-api/decrypt` -- took 1,066ms (slow!)
- `/immobilien/api/v1/singlepage/favorites` -- took 559ms

### Console Errors

Only 1 error from third-party ad script: `adnz.co` -- "oneId Nt: Promise awaiting time exceed"

## Phase 3: HTML/Structure Analysis

### Framework Detection

- **Next.js** application (confirmed via `__NEXT_DATA__` script tag)
- Build ID: `Wx5yFCc3XosoW9clf5Fii`
- Page route: `/result`
- `__NEXT_DATA__` size: 139 KB (large -- contains full initial result data SSR'd)
- Runtime config includes: Optimizely, GPT ads, Lytics, feature flags, Teads

### DOM Statistics

| Metric | Value |
|--------|-------|
| Total elements | 2,330 |
| Scripts | 60 (45 external, 15 inline) |
| Inline styles | 260 |
| Images | 12 |
| Iframes | 10 |
| Forms | 1 |
| Buttons | 35 |
| Links | 171 |
| Inputs | 29 |

### Semantic HTML Usage

| Tag | Count | Assessment |
|-----|-------|------------|
| h1 | 1 | Correct -- single h1 |
| h2 | 12 | Good usage |
| h3 | 4 | |
| nav | 7 | Excessive -- likely includes mobile/desktop variants |
| main | 1 | Good |
| article | 0 | Missing -- listings should use article tags |
| section | 0 | Missing -- could improve page structure |
| header | 43 | Excessive -- likely each listing card has a header element |
| footer | 1 | Good |
| aside | 0 | Missing -- sidebar filters could use aside |

**Issues**:
- No `<article>` elements for apartment listings (missed semantic opportunity)
- No `<section>` elements at all
- No `<aside>` for the filter sidebar
- 43 `<header>` elements seems excessive
- 260 inline `<style>` tags (CSS-in-JS overhead)

### Accessibility

| Check | Result |
|-------|--------|
| Language attribute | `de-CH` (correct) |
| ARIA roles | 201 |
| ARIA hidden | 185 (high -- many hidden elements) |
| ARIA labels | 9 |
| Tabindex elements | 153 |
| Images without alt | 0 (all images have alt) |
| Labeled inputs | 6 |
| Skip links | 0 (missing!) |
| Focusable elements | 257 |

**Issues**:
- No skip navigation link
- Only 9 aria-labels for 257 focusable elements
- 185 aria-hidden elements (aggressive hiding)
- `robots` meta tag is `noindex,follow` (search result pages not indexed)

### Image Optimization

- Images served via BunnyCDN with Cloudinary-style transforms: `c_fill,f_jpg,h_344,q_auto,w_458`
- **No WebP/AVIF**: All images served as JPG despite CDN capability
- **Only 3 of 12 images use lazy loading** -- above-the-fold images eagerly loaded (correct)
- **8 images have srcset** -- responsive images partially implemented
- All images have alt text (good)

### Meta Tags

- Title: "3 bis 3 Zimmer Immobilien mieten in Zurich"
- Description present (truncated)
- **No Open Graph tags** on search results page
- Apple iTunes app banner configured
- `noindex,follow` robots directive
- Viewport: `width=device-width` (no initial-scale specified)

### Responsive Behavior

Tested at 375x812 (iPhone size):
- Navigation collapses to hamburger menu
- Filter sidebar becomes floating "Filter (5)" button
- Listings stack vertically, full-width images
- Breadcrumb remains but truncates
- Ad placeholder visible with "Werbeanzeige entfernt" (ad removed)
- Overall good mobile adaptation

## Phase 4: Chrome MCP Tool Assessment

### Tools Used and Effectiveness

| Tool | Used For | Rating | Notes |
|------|----------|--------|-------|
| `navigate` | Page navigation | Excellent | Fast, reliable |
| `computer` (screenshot) | Visual verification | Excellent | Clear images, proper viewport |
| `computer` (left_click) | Button/link clicks | Good | Works well with both coordinates and refs |
| `computer` (type) | Text input | Good | Typed correctly including umlauts |
| `computer` (scroll) | Page scrolling | Partial | Page scrolled but not dropdown internals |
| `computer` (wait) | Timing | Good | Simple and effective |
| `javascript_tool` | DOM manipulation, perf measurement | Excellent | Essential escape hatch |
| `read_page` | Accessibility tree | Good | Interactive filter very useful |
| `find` | Element discovery | Excellent | Natural language queries worked great |
| `form_input` | Form filling | Limited | Only works with native form elements |
| `read_network_requests` | Network analysis | Partial | Only captures post-activation requests |
| `read_console_messages` | Error detection | Good | Error-only filter useful |
| `resize_window` | Responsive testing | Good | Instant resize, screenshot reflects change |
| `tabs_context_mcp` | Tab management | Good | Needed for initial setup |

### Chrome MCP Strengths

1. **Natural language `find`** is remarkably good at locating elements semantically.
2. **`javascript_tool`** provides full page context access for anything the UI tools cannot handle.
3. **Screenshot quality** is high and captures the actual rendered viewport.
4. **`read_page` with interactive filter** gives a clean, actionable list of form controls.
5. **Tool composition** works well -- screenshot to orient, find to locate, click/type to interact.
6. **Responsive testing** with `resize_window` is seamless.

### Chrome MCP Pain Points

1. **`form_input` does not support custom dropdowns** -- most modern web apps use custom select/combobox components (ARIA role="combobox") that are `<button>` or `<div>` based. `form_input` only works with native `<input>`, `<select>`, `<textarea>`. This is the biggest gap.
2. **`read_network_requests` only captures requests made after first call** -- cannot analyze initial page load network waterfall. Had to rely on `performance.getEntriesByType('resource')` via JS instead.
3. **Scrolling inside overflow containers** does not work reliably with `computer` scroll -- the main page scrolls instead of the dropdown/container at the cursor position.
4. **Cookie/query string data blocking** -- some JS results containing URL query strings or cookies were blocked, requiring restructuring the query to avoid sensitive data in output.
5. **No built-in performance measurement** -- had to write custom JS for all Web Vitals. A dedicated performance tool would be valuable.
6. **`update_plan` failed without tab context** -- had to call `tabs_context_mcp` first, but the error message was not helpful.

### Suggestions for Chrome MCP Improvement

1. Add `form_input` support for ARIA combobox/listbox patterns (click to open, then select option by value).
2. Add a `performance` tool that automatically collects Core Web Vitals, navigation timing, and resource summary.
3. Start network request capture automatically when a tab is first used, so initial page load is captured.
4. Improve scroll targeting to detect and scroll overflow containers under the cursor.
5. Add a `wait_for` tool that waits for a selector or condition (instead of fixed time waits).

## Comparis.ch Improvement Recommendations

### Performance
- **Reduce TTFB** (1,416ms is slow) -- investigate server-side caching, edge rendering
- **`/immobilien/node-api/decrypt` takes 1,066ms** -- this API call is very slow and should be optimized or parallelized
- **`__NEXT_DATA__` is 139 KB** -- consider streaming/partial hydration to reduce initial payload
- **260 inline style tags** -- CSS-in-JS extraction to external CSS would reduce DOM size
- **Serve WebP/AVIF** -- CDN supports format negotiation but all images served as JPG

### HTML/Accessibility
- Add `<article>` elements for each listing card
- Add `<section>` and `<aside>` for page structure
- Add skip navigation link
- Increase ARIA label coverage (only 9 for 257 focusable elements)
- Add Open Graph meta tags for social sharing
- Add `initial-scale=1` to viewport meta
- Reduce the 43 `<header>` elements to semantically appropriate usage

### Third-Party Impact
- 48 requests to CDN for app bundles (chunking seems aggressive)
- 23 requests to adsafeprotected (ad viewability tracking overhead)
- Heavy ad tech stack: Google GPT, Teads, adnz.co, DataDome
- Consider lazy-loading ad scripts to improve initial load
