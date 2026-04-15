---
title: "Broken Test Fixture Page: Design Document"
date: 2026-04-09
status: draft
type: research
tags: [test-fixture, audit, dogfooding, skill-design]
---

# Broken Test Fixture Page: Design Document

A deliberately broken HTML page that serves as a controlled test fixture for ff-rdp's website audit capabilities. The page looks like a real (poorly built) apartment listing site, but contains catalogued issues across performance, accessibility, SEO, structure, and security.

## Page Concept: "WohnungsDirekt.ch"

A fictional Swiss apartment listing page — a single property detail page for a 3.5-room apartment in Zurich Seefeld. This ties directly into the comparis.ch dogfooding context and feels natural. The page has a header with navigation, hero image, property details, a photo gallery, an inquiry form, a map placeholder, and a footer.

The name "WohnungsDirekt" suggests a scrappy startup that skipped quality engineering — a believable origin for a page full of issues.

## Serving Strategy

**Recommended: Python `http.server` (or `npx serve`)**

A local HTTP server is necessary because:
- `file://` URLs break CORS, cookie APIs, `fetch()`, Performance API timing data, and CSP headers
- ff-rdp's `navigate` command works with any `http://` URL
- `cookies` command requires HTTP (cookies aren't set on `file://`)
- Mixed content detection requires an HTTP origin
- `python3 -m http.server 8080` is a one-liner, zero-install on macOS/Linux

The fixture directory structure:
```
fixtures/broken-page/
├── index.html          # The main broken page
├── style.css           # Render-blocking CSS (intentionally separate for perf issue)
├── app.js              # Render-blocking sync JS
├── analytics.js        # Fake third-party tracker
├── hero.bmp            # Unoptimized image (BMP instead of WebP/AVIF)
├── photo1.png          # Large unoptimized PNG
├── photo2.png          # Large unoptimized PNG
└── serve.sh            # One-liner: python3 -m http.server 8080
```

Alternative: a `ff-rdp serve-fixture` subcommand could be added later, but that's overengineering for now.

## Issue Catalogue

Each issue is tagged with:
- **Category**: perf / a11y / seo / structure / security / ux
- **Level**: 1 (obvious), 2 (moderate), 3 (subtle)
- **Detectable by**: which ff-rdp command finds it
- **Fix**: what the LLM should do

---

### Level 1: Obvious Issues (13 issues)

#### L1-01: Missing `<title>` tag
- **Category**: seo
- **HTML**: `<head>` has no `<title>` element
- **Detectable by**: `dom 'title'` returns empty / `snapshot` shows no title / `eval 'document.title'`
- **Fix**: Add `<title>3.5-Zimmer-Wohnung Seefeld, Zürich | WohnungsDirekt</title>`

#### L1-02: Missing `lang` attribute on `<html>`
- **Category**: a11y, seo
- **HTML**: `<html>` instead of `<html lang="de-CH">`
- **Detectable by**: `a11y` tree audit / `dom 'html' --jq '.results[].attributes'`
- **Fix**: Add `lang="de-CH"`

#### L1-03: Missing `alt` text on images
- **Category**: a11y
- **HTML**: `<img src="hero.bmp">` (no alt attribute at all), `<img src="photo1.png" alt="">` (empty alt on meaningful image)
- **Detectable by**: `a11y` (images without names) / `dom 'img' --fields attributes`
- **Fix**: Add descriptive alt text: `alt="Wohnzimmer mit Seesicht, 3.5-Zimmer-Wohnung Seefeld"`

#### L1-04: Broken heading hierarchy (h1 -> h4 -> h2)
- **Category**: a11y, seo
- **HTML**:
  ```html
  <div style="font-size:28px"><h1>WohnungsDirekt</h1></div>
  ...
  <h4>Objektdetails</h4>
  ...
  <h2>Fotogalerie</h2>
  <h1>Kontakt</h1>  <!-- duplicate h1! -->
  ```
- **Detectable by**: `a11y` (heading level audit) / `dom 'h1,h2,h3,h4,h5,h6'`
- **Fix**: Restructure to h1 (page title) -> h2 (sections) -> h3 (subsections)

#### L1-05: Duplicate `<h1>` tags
- **Category**: seo
- **HTML**: Two `<h1>` elements (site name + "Kontakt" section)
- **Detectable by**: `dom 'h1'` returns 2 results
- **Fix**: Change site name to `<div>` with CSS, keep only property title as `<h1>`

#### L1-06: No `<meta name="description">`
- **Category**: seo
- **HTML**: Missing entirely from `<head>`
- **Detectable by**: `dom 'meta[name="description"]'` returns empty
- **Fix**: Add `<meta name="description" content="3.5-Zimmer-Wohnung in Zürich Seefeld, CHF 2'850/Mt. 78m², Balkon mit Seesicht.">`

#### L1-07: No `<meta name="viewport">`
- **Category**: ux, perf
- **HTML**: Missing from `<head>`
- **Detectable by**: `dom 'meta[name="viewport"]'` returns empty / `responsive` command shows no scaling
- **Fix**: Add `<meta name="viewport" content="width=device-width, initial-scale=1">`

#### L1-08: All `<div>` soup — no semantic HTML
- **Category**: structure, a11y
- **HTML**: Navigation is `<div class="nav">`, main content is `<div class="content">`, footer is `<div class="footer">` — zero `<nav>`, `<main>`, `<article>`, `<section>`, `<header>`, `<footer>`
- **Detectable by**: `snapshot` (all DIVs) / `dom 'nav,main,article,section,header,footer'` returns empty / `a11y` (no landmarks)
- **Fix**: Replace divs with semantic elements

#### L1-09: Inline styles everywhere
- **Category**: structure, perf
- **HTML**: `<div style="color:red;font-size:12px;margin:10px;padding:5px;float:left;width:300px">` on 20+ elements
- **Detectable by**: `dom '[style]'` returns many results / `dom stats` (inline style count)
- **Fix**: Move to CSS classes

#### L1-10: Render-blocking `<script>` in `<head>`
- **Category**: perf
- **HTML**: `<script src="app.js"></script>` in `<head>` without `defer` or `async`
- **Detectable by**: `perf` (slow load) / `sources` (lists loaded scripts) / `dom 'script:not([defer]):not([async])[src]'`
- **Fix**: Add `defer` attribute or move to end of `<body>`

#### L1-11: Render-blocking CSS (no preload)
- **Category**: perf
- **HTML**: `<link rel="stylesheet" href="style.css">` — large CSS file with unused rules
- **Detectable by**: `perf` resource analysis / `sources`
- **Fix**: Inline critical CSS, async-load the rest

#### L1-12: Images without `loading="lazy"`
- **Category**: perf
- **HTML**: Below-fold images load eagerly
- **Detectable by**: `dom stats` (images_without_lazy) / `dom 'img:not([loading="lazy"])'`
- **Fix**: Add `loading="lazy"` to below-fold images

#### L1-13: Console errors from bad JS
- **Category**: structure
- **HTML**: `app.js` contains `document.getElementById('nonexistent').textContent = 'hello'` which throws TypeError
- **Detectable by**: `console --level error`
- **Fix**: Add null check or fix the selector

---

### Level 2: Moderate Issues (12 issues)

#### L2-01: Bad color contrast
- **Category**: a11y
- **HTML**: `<span style="color: #999; background-color: #fff">CHF 2'850 / Monat</span>` — light gray on white, ratio ~2.8:1 (fails WCAG AA 4.5:1)
- **Detectable by**: `a11y contrast`
- **Fix**: Change to `color: #595959` or darker

#### L2-02: Tiny click targets
- **Category**: ux, a11y
- **HTML**: `<a href="/rooms" style="font-size:10px;padding:0">Zimmer</a>` — target size < 24x24px (fails WCAG 2.5.8)
- **Detectable by**: `geometry 'a'` (bounding rect too small) / `a11y --interactive`
- **Fix**: Add padding, min-height: 44px

#### L2-03: Custom widget without ARIA
- **Category**: a11y
- **HTML**:
  ```html
  <div class="dropdown" onclick="toggleDropdown()">
    <div class="dropdown-label">Sortieren nach</div>
    <div class="dropdown-items" style="display:none">
      <div onclick="sort('price')">Preis</div>
      <div onclick="sort('size')">Fläche</div>
    </div>
  </div>
  ```
  No `role`, no `aria-expanded`, no `aria-haspopup`, no keyboard support
- **Detectable by**: `a11y` (generic elements with click handlers) / `a11y --interactive`
- **Fix**: Add `role="listbox"`, `aria-expanded`, keyboard handlers, `tabindex`

#### L2-04: Form inputs without labels
- **Category**: a11y
- **HTML**: `<input type="text" placeholder="Ihr Name">` — no `<label>`, no `aria-label`
- **Detectable by**: `a11y` (inputs without names) / `dom 'input:not([aria-label])' `
- **Fix**: Add `<label for="name">Name</label>` and `id="name"` on input

#### L2-05: Missing `rel="noopener"` on external links
- **Category**: security
- **HTML**: `<a href="https://maps.google.com" target="_blank">Karte anzeigen</a>`
- **Detectable by**: `dom 'a[target="_blank"]:not([rel*="noopener"])'`
- **Fix**: Add `rel="noopener noreferrer"`

#### L2-06: No canonical URL
- **Category**: seo
- **HTML**: Missing `<link rel="canonical">`
- **Detectable by**: `dom 'link[rel="canonical"]'` returns empty
- **Fix**: Add `<link rel="canonical" href="https://wohnungsdirekt.ch/inserate/12345">`

#### L2-07: No Open Graph tags
- **Category**: seo
- **HTML**: No `og:title`, `og:description`, `og:image` meta tags
- **Detectable by**: `dom 'meta[property^="og:"]'` returns empty
- **Fix**: Add OG tags for social sharing

#### L2-08: Unoptimized image format (BMP)
- **Category**: perf
- **HTML**: `<img src="hero.bmp">` — BMP is uncompressed, ~500KB for a small image
- **Detectable by**: `perf` (large transfer_size for hero.bmp) / `sources` / `dom 'img[src$=".bmp"]'`
- **Fix**: Convert to WebP/AVIF, add `srcset` for responsive sizes

#### L2-09: No `width`/`height` on images (CLS source)
- **Category**: perf
- **HTML**: `<img src="hero.bmp">` — no dimensions, causes layout shift when image loads
- **Detectable by**: `perf vitals` (CLS > 0.1) / `dom 'img:not([width])'`
- **Fix**: Add `width="800" height="600"` or use CSS `aspect-ratio`

#### L2-10: Fake "third-party" tracking script
- **Category**: perf, security
- **HTML**: `<script src="analytics.js"></script>` — a sync script that does `document.cookie = "tracker=abc123"` (no Secure, no HttpOnly flag possible from JS, no SameSite)
- **Detectable by**: `cookies` (insecure cookie flags) / `sources` / `perf` (blocking script)
- **Fix**: Remove or set cookies server-side with proper flags, load async

#### L2-11: `tabindex` creating keyboard trap
- **Category**: a11y
- **HTML**: `<div tabindex="1">` on the photo gallery, positive tabindex disrupts natural tab order. Combined with the custom dropdown that captures focus.
- **Detectable by**: `a11y` / `dom '[tabindex]'` — check for positive values
- **Fix**: Use `tabindex="0"` or `-1`, ensure natural DOM order

#### L2-12: Missing skip-to-content link
- **Category**: a11y
- **HTML**: No skip link before the navigation
- **Detectable by**: `a11y` (no skip link landmark) / `dom 'a[href="#main"]'` returns empty
- **Fix**: Add `<a href="#main" class="skip-link">Zum Inhalt springen</a>`

---

### Level 3: Subtle Issues (8 issues)

#### L3-01: Cookie without Secure/SameSite flags
- **Category**: security
- **HTML**: `analytics.js` sets `document.cookie = "tracker=abc123; path=/"` — no `Secure`, no `SameSite`
- **Detectable by**: `cookies --jq '[.results[] | select(.secure == false)]'`
- **Fix**: Set server-side with `Secure; HttpOnly; SameSite=Lax`

#### L3-02: Mixed content reference
- **Category**: security
- **HTML**: `<img src="http://placekitten.com/400/300">` — HTTP resource on HTTPS-capable page
- **Detectable by**: `dom 'img[src^="http://"]'` / `console` (mixed content warning)
- **Fix**: Change to `https://` or use local asset

#### L3-03: Layout shift from late-injected content
- **Category**: perf
- **HTML**: `app.js` injects a banner after 500ms: `setTimeout(() => { document.body.insertBefore(banner, document.body.firstChild) }, 500)` — pushes all content down
- **Detectable by**: `perf vitals` (high CLS) / `console` (if logged)
- **Fix**: Reserve space with a placeholder or include in initial HTML

#### L3-04: Excessive DOM depth and node count
- **Category**: perf
- **HTML**: Nested wrapper divs 8+ levels deep, 800+ DOM nodes from repeated empty divs used as spacers
- **Detectable by**: `dom stats` (node_count > 800) / `snapshot --depth 10`
- **Fix**: Flatten DOM, use CSS margins/padding instead of spacer divs

#### L3-05: No `robots.txt` / broken `<meta name="robots">`
- **Category**: seo
- **HTML**: `<meta name="robots" content="noindex, nofollow">` — accidentally blocking search engines
- **Detectable by**: `dom 'meta[name="robots"]'` — check content value
- **Fix**: Change to `content="index, follow"` or remove

#### L3-06: Missing structured data (JSON-LD)
- **Category**: seo
- **HTML**: No `<script type="application/ld+json">` for RealEstateAgent / Apartment schema
- **Detectable by**: `dom 'script[type="application/ld+json"]'` returns empty
- **Fix**: Add JSON-LD with `@type: Apartment` schema

#### L3-07: No `font-display: swap` on custom font
- **Category**: perf
- **HTML**: `style.css` has `@font-face { font-family: 'WDFont'; src: url('...'); }` without `font-display: swap` — causes invisible text during load (FOIT)
- **Detectable by**: `styles 'body' --applied` (check font-face rules) / `perf` (font loading time)
- **Fix**: Add `font-display: swap`

#### L3-08: Horizontal overflow on mobile widths
- **Category**: ux
- **HTML**: Fixed-width table `<div style="width:900px">` inside the property details, no overflow handling
- **Detectable by**: `responsive '.property-table' --widths 375,768` (element wider than viewport) / `geometry '.property-table'`
- **Fix**: Use responsive CSS, `max-width: 100%`, `overflow-x: auto`

---

## Full Issue Summary

| Category     | L1  | L2  | L3  | Total |
|-------------|-----|-----|-----|-------|
| Performance  | 4   | 2   | 3   | 9     |
| Accessibility| 4   | 5   | 0   | 9     |
| SEO          | 3   | 2   | 2   | 7     |
| Structure    | 3   | 0   | 1   | 4     |
| Security     | 0   | 2   | 2   | 4     |
| UX           | 1   | 1   | 1   | 3     |
| **Total**    |**13**|**12**|**8**|**33** |

## ff-rdp Command Coverage

Every ff-rdp command finds at least one issue:

| Command | Issues Found |
|---------|-------------|
| `a11y` | L1-02, L1-03, L1-04, L1-08, L2-01, L2-03, L2-04, L2-11, L2-12 |
| `a11y contrast` | L2-01 |
| `dom` | L1-01, L1-04, L1-05, L1-06, L1-07, L1-08, L1-09, L1-10, L1-12, L2-05, L2-06, L2-07, L2-08, L2-09, L2-11, L3-02, L3-05, L3-06 |
| `dom stats` | L1-09, L1-12, L3-04 |
| `snapshot` | L1-01, L1-04, L1-08, L3-04 |
| `perf` | L1-10, L1-11, L2-08, L2-10, L3-07 |
| `perf vitals` | L2-09, L3-03 |
| `console` | L1-13, L3-02, L3-03 |
| `cookies` | L2-10, L3-01 |
| `sources` | L1-10, L1-11, L2-08, L2-10 |
| `styles` | L3-07 |
| `geometry` | L2-02, L3-08 |
| `responsive` | L1-07, L3-08 |
| `eval` | L1-01 |

## Page Structure (Visual Layout)

```
┌─────────────────────────────────────────────────┐
│ <div class="nav">  (should be <nav>)            │
│   WohnungsDirekt <h1>  (duplicate h1)           │
│   [Mieten] [Kaufen] [Inserieren] (tiny links)   │
│   [Sortieren ▾] (custom dropdown, no ARIA)      │
│  ── no skip link ──                             │
├─────────────────────────────────────────────────┤
│ <div class="content">  (should be <main>)       │
│                                                  │
│  ┌─────────────────────────────────────┐        │
│  │ hero.bmp (no alt, no width/height,  │        │
│  │ BMP format, causes CLS)             │        │
│  └─────────────────────────────────────┘        │
│                                                  │
│  <h4>Objektdetails</h4>  (skips h2, h3!)       │
│  ┌─────────────────────────────────────┐        │
│  │ CHF 2'850/Mt  (bad contrast #999)   │        │
│  │ 3.5 Zimmer | 78 m² | 2. OG         │        │
│  │ Seefeldstrasse 42, 8008 Zürich      │        │
│  │ ┌──────────────────── width:900px ─┐│        │
│  │ │ Property details table            ││        │
│  │ │ (causes horizontal overflow)      ││        │
│  │ └──────────────────────────────────┘│        │
│  └─────────────────────────────────────┘        │
│                                                  │
│  <h2>Fotogalerie</h2>                          │
│  [photo1.png] [photo2.png]  (no lazy, no alt)  │
│  <div tabindex="1">  (keyboard trap)           │
│                                                  │
│  <h1>Kontakt</h1>  (second h1!)                │
│  ┌─────────────────────────────────────┐        │
│  │ [Ihr Name     ] (no <label>)        │        │
│  │ [Email        ] (no <label>)        │        │
│  │ [Nachricht    ] (no <label>)        │        │
│  │ [Senden]                            │        │
│  └─────────────────────────────────────┘        │
│                                                  │
│  <a href="https://maps.google.com"              │
│     target="_blank">Karte</a> (no noopener)     │
│  <img src="http://placekitten.com/...">         │
│     (mixed content)                              │
│                                                  │
├─────────────────────────────────────────────────┤
│ <div class="footer">  (should be <footer>)      │
│   © 2026 WohnungsDirekt                         │
│   <meta name="robots" content="noindex">        │
│   (wait, this is in the body... also wrong)     │
└─────────────────────────────────────────────────┘

<head>:
  - NO <title>
  - NO <meta name="description">
  - NO <meta name="viewport">
  - NO <link rel="canonical">
  - NO <meta property="og:*">
  - <meta name="robots" content="noindex, nofollow">
  - <script src="app.js"> (blocking, no defer)
  - <script src="analytics.js"> (blocking, sets insecure cookie)
  - <link rel="stylesheet" href="style.css"> (render-blocking)
```

## Audit Workflow: How an LLM Uses ff-rdp

### Phase 1: Discovery (gather data)

```bash
# Quick structural overview
ff-rdp navigate http://localhost:8080
ff-rdp snapshot --depth 6
ff-rdp dom stats

# Performance
ff-rdp perf audit
ff-rdp perf vitals

# Accessibility
ff-rdp a11y --depth 8
ff-rdp a11y contrast

# SEO essentials
ff-rdp dom 'title, meta[name="description"], meta[name="viewport"], link[rel="canonical"], meta[property^="og:"], meta[name="robots"]'

# Security
ff-rdp cookies
ff-rdp dom 'a[target="_blank"]:not([rel*="noopener"])'
ff-rdp dom 'img[src^="http://"]'

# Console errors
ff-rdp console --level error

# UX on mobile
ff-rdp responsive 'a, .property-table, img' --widths 375,768,1024
```

### Phase 2: Read Source

The LLM reads `index.html` with the Read tool to understand the full source.

### Phase 3: Fix (edit source)

The LLM uses the Edit tool to fix issues, mapping each finding to a specific line. Example fixes:

1. Add `lang="de-CH"` to `<html>` tag
2. Add `<title>`, `<meta name="description">`, `<meta name="viewport">` to `<head>`
3. Replace `<div class="nav">` with `<nav>`, etc.
4. Fix heading hierarchy
5. Add `alt` attributes to images
6. Add `defer` to scripts
7. Add `loading="lazy"` to below-fold images
8. Fix contrast values
9. Add ARIA attributes to custom dropdown
10. Add `<label>` elements to form inputs

### Phase 4: Verify

```bash
# Reload and re-audit
ff-rdp reload
ff-rdp a11y
ff-rdp a11y contrast
ff-rdp perf vitals
ff-rdp dom stats
ff-rdp console --level error
```

Compare before/after. The LLM reports which issues are resolved and which remain.

## Gamification: Level Progression

The fixture works as a skill-building exercise:

- **Level 1 challenge**: "Fix all 13 obvious issues" — good for beginners, teaches basic HTML hygiene
- **Level 2 challenge**: "Fix all issues detectable by `a11y` and `perf`" — teaches WCAG and Core Web Vitals
- **Level 3 challenge**: "Achieve zero findings across all ff-rdp audit commands" — requires deep knowledge of security headers, structured data, responsive design

Each level can be verified by re-running the relevant ff-rdp commands and checking that the issue count drops to zero.

## Next Steps

- [ ] Build the actual HTML page (`index.html`) with all 33 issues
- [ ] Create the supporting files (`style.css`, `app.js`, `analytics.js`)
- [ ] Create minimal image files for the fixture (small BMP/PNG files, can be 1x1 or tiny placeholder images)
- [ ] Write `serve.sh` launcher script
- [ ] Test with ff-rdp: verify every command detects the expected issues
- [ ] Write the audit skill/recipe that orchestrates the workflow
