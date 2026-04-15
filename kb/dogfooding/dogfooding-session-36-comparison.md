---
title: "Dogfooding Session 36: ff-rdp vs Chrome MCP Comparison"
date: 2026-04-09
type: dogfooding
tags:
  - dogfooding
  - comparison
  - ff-rdp
  - chrome-mcp
  - comparis
  - performance
status: completed
---

# Dogfooding Session 36: ff-rdp vs Chrome MCP Comparison

Both tools performed the same user journey: navigate to comparis.ch, search for 3-room apartments in Zurich, analyze performance and HTML structure.

## Side-by-Side: Apartment Search Journey

| Step | ff-rdp | Chrome MCP |
|------|--------|------------|
| Navigate | `navigate <url> --timeout 15000` | `navigate(url, tabId)` |
| Cookie consent | `eval 'window.cmp_noscreen=true'` + `--auto-consent` flag on launch | `javascript_tool(window.cmp_noscreen=true)` |
| Screenshot | `screenshot -o /tmp/file.png` | `computer(action: screenshot)` |
| Find elements | `dom 'a[href*="immobilien"]'` (CSS selector) | `find("apartment listing")` (natural language) |
| Fill form | `type 'input[name=...]' "Zurich"` | `computer(action: type, text: "Zurich")` + coordinate click |
| Custom dropdown | **Failed** -- `type` doesn't trigger React events | **Failed** -- `form_input` rejects `<button>` elements |
| Workaround | Constructed URL with query params directly | JavaScript DOM manipulation of `[role="option"]` elements |
| Scroll | `eval 'window.scrollTo(0,600)'` (no scroll cmd) | `computer(action: scroll)` (but can't target overflow containers) |
| Back | `back` | `navigate(url: "back")` |

**Verdict**: Both tools hit the same wall with React custom dropdowns. ff-rdp's workaround (direct URL) was simpler; Chrome MCP's (JS click on role=option) was more realistic but more code.

## Side-by-Side: Performance Measurement

| Metric | ff-rdp Value | Chrome MCP Value | Notes |
|--------|-------------|-----------------|-------|
| TTFB | 1,250 ms | 1,416 ms | Both flag this as needing improvement |
| FCP | 1,361 ms | 1,576 ms | Both good |
| LCP | ~0 ms* | 1,800 ms | ff-rdp estimate unreliable in headless |
| CLS | 0.0 | 0.009 | Both excellent |
| DOM Nodes | 1,989 | 2,330 | Different measurement methods |
| Total Resources | 179 | 137 | ff-rdp counts more (daemon buffering?) |
| Third-party % | 63% (113/179) | ~70% estimated | Both show heavy third-party load |
| Document Size | 657 KB | 467 KB decompressed | Different measurement points |

*ff-rdp's LCP is estimated via DOM approximation in headless Firefox -- not reliable.

### Performance Tool Comparison

| Capability | ff-rdp | Chrome MCP |
|-----------|--------|------------|
| Core Web Vitals | `perf vitals` -- single command | Custom JS via `javascript_tool` |
| Resource summary | `perf summary` -- domain breakdown | Custom JS `performance.getEntriesByType` |
| Audit/recommendations | `perf audit` -- actionable flags | None -- manual analysis only |
| Network waterfall | `network` (with daemon) | `read_network_requests` (post-activation only) |
| Render-blocking detection | `perf audit` includes it | Custom JS to check `<script>` attrs |
| Navigation timing | `perf` subcommands | Custom JS |

**Verdict**: ff-rdp wins decisively on performance analysis. `perf audit` is a single command that gives actionable insights. Chrome MCP requires writing custom JavaScript for every metric.

## Side-by-Side: HTML/Accessibility Analysis

| Capability | ff-rdp | Chrome MCP |
|-----------|--------|------------|
| Accessibility tree | `a11y` -- full tree | `read_page` -- accessibility-oriented DOM |
| Contrast check | `a11y contrast` -- WCAG AA/AAA | None built-in |
| Semantic HTML | `dom stats` -- tag counts | Custom JS `document.querySelectorAll` |
| Page snapshot | `snapshot` -- LLM-optimized | `read_page` -- tree with refs |
| CSS inspection | `styles <selector>` | Custom JS `getComputedStyle` |
| Geometry/layout | `geometry <selector>` | Custom JS `getBoundingClientRect` |
| Responsive testing | `responsive <selectors> --widths` | `resize_window` + screenshot |
| Element discovery | CSS selectors only | Natural language `find` tool |

**Verdict**: ff-rdp has deeper built-in analysis (contrast, geometry, responsive). Chrome MCP has better element discovery via natural language `find`.

## What ff-rdp Does Better

1. **Dedicated performance commands** -- `perf audit`, `perf vitals`, `perf summary` are purpose-built and immediately useful. No JS writing needed.
2. **Accessibility analysis** -- `a11y contrast`, `a11y` tree inspection with WCAG ratings.
3. **`--format text`** -- consistent human-readable tables across all commands.
4. **`--jq` filtering** -- pipe-friendly JSON filtering built into every command.
5. **`recipes`** -- curated jq one-liners for common tasks. Great for discoverability.
6. **`snapshot`** -- LLM-optimized page dump with semantic roles.
7. **`cookies` and `storage`** -- dedicated commands, not JS workarounds.
8. **`sources`** -- list all loaded scripts in one command.
9. **`geometry` and `responsive`** -- specialized layout analysis tools.
10. **`--auto-consent`** on launch -- one flag to handle cookie banners.
11. **Single binary CLI** -- no browser extension, no MCP server. Just `ff-rdp <command>`.

## What Chrome MCP Does Better

1. **Natural language `find`** -- "search bar", "login button", "apartment listing with price" just works. ff-rdp requires knowing CSS selectors.
2. **Visual interaction model** -- screenshot → identify coordinates → click. More intuitive for GUI-heavy pages.
3. **`read_page` with interactive filter** -- clean list of all form controls with refs for subsequent actions.
4. **Element refs** -- `ref_14` can be passed to `form_input`, `scroll_to`, `click`. ff-rdp uses CSS selectors everywhere (more precise but requires DOM knowledge).
5. **GIF recording** -- can record browser sessions as animated GIFs for documentation/demos.
6. **Tab management** -- multiple tabs in a group, easy to switch between.
7. **Resize window** -- instant viewport resize for responsive testing (ff-rdp uses `responsive` command which is more powerful but different UX).
8. **`computer` tool** -- full mouse/keyboard simulation including coordinates, drag, hover. More like a real user.

## Shared Pain Points (Both Tools)

1. **React custom dropdowns** -- neither tool can natively interact with ARIA combobox/listbox patterns built from `<button>`/`<div>` elements. Both require workarounds.
2. **Autocomplete triggers** -- typing into a React-controlled input doesn't trigger the autocomplete dropdown in either tool. Both set the value but don't simulate native keyboard events that React listens for.
3. **Scroll targeting** -- neither can reliably scroll inside an overflow container (only the main viewport).

## ff-rdp Improvement Ideas (Informed by Chrome MCP)

| Gap | Chrome MCP Has | ff-rdp Could Add |
|-----|---------------|-----------------|
| Natural language element finding | `find("login button")` | `find "login button"` -- use LLM or heuristics to match elements |
| Element refs for chaining | `ref_14` passed between tools | `--ref` flag that assigns IDs to query results for use in subsequent commands |
| GIF/video recording | `gif_creator` | `record` command that captures a sequence of screenshots into GIF |
| Scroll command | `computer(scroll)` | `scroll` / `scroll-to <selector>` |
| Native keyboard events | `computer(type)` (still imperfect) | `type --native` flag dispatching KeyboardEvent sequences |
| Compact DOM output | `read_page` is concise | `dom --compact` mode stripping SVGs, srcsets, inline styles |
| `text` alias | N/A | `text` as alias for `page-text` |

## comparis.ch Findings (Agreed by Both Tools)

### Performance Issues
- **TTFB 1,250-1,416ms** -- server response is slow, needs caching or edge rendering
- **Massive third-party load** -- 40+ domains, 63-70% of requests are third-party (ads, tracking, analytics)
- **At least 15 tracking systems** -- TikTok, Facebook, Bing, Google, Hotjar, Clarity, Reddit, Lytics, etc.
- **Slow API endpoints** -- `/node-api/decrypt` takes 1,066ms, favorites API 559ms
- **Large `__NEXT_DATA__`** -- 139 KB of SSR payload

### HTML/Accessibility Issues
- Missing `<article>` elements on listing cards
- Missing `<section>`, `<aside>` semantic elements
- No skip navigation link
- 43 `<header>` elements (excessive)
- 260 inline `<style>` tags from CSS-in-JS
- No WebP/AVIF despite CDN support
- No Open Graph meta tags on results page

### What comparis.ch Does Well
- Good CLS (0) -- stable layout
- Proper `<h1>`, landmarks, breadcrumbs
- All images have alt text
- Good mobile responsive adaptation
- `de-CH` language attribute set correctly
- Images use srcset for responsive sizing
- All scripts use async/defer (no render-blocking JS)

## Session Stats

| Metric | ff-rdp Agent | Chrome MCP Agent |
|--------|-------------|-----------------|
| Total tokens | 84,691 | 89,421 |
| Tool calls | 77 | 61 |
| Duration | 6m 21s | 6m 59s |
| Commands tested | 33 | 14 |

ff-rdp tested more than twice as many distinct commands because it has purpose-built tools for each analysis task. Chrome MCP used fewer tools but relied heavily on `javascript_tool` as a general-purpose escape hatch.
