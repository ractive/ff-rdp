---
title: "comparis.ch Website Analysis"
date: 2026-04-08
tags: [dogfooding, performance, accessibility, analysis]
status: complete
site: comparis.ch
tool_version: ff-rdp 0.1.0
firefox_version: "149.0"
---

# comparis.ch Website Analysis

Analyzed using `ff-rdp` against a live headless Firefox 149 instance on 2026-04-08.

## Core Web Vitals

### Homepage (comparis.ch)
| Metric | Value | Rating |
|--------|-------|--------|
| FCP | 80 ms | Good |
| TTFB | 0 ms | Good |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |
| LCP | null | N/A |

### Search Results (3-room apartments, Zürich)
| Metric | Value | Rating |
|--------|-------|--------|
| FCP | 1245 ms | Good |
| TTFB | 1129 ms | Needs Improvement |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |
| LCP | null | N/A |

### Listing Detail Page
| Metric | Value | Rating |
|--------|-------|--------|
| FCP | 775 ms | Good |
| TTFB | 681 ms | Good |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |
| LCP | null | N/A |

### Immobilien Default Page
| Metric | Value | Rating |
|--------|-------|--------|
| FCP | 44 ms | Good |
| TTFB | 2 ms | Good |
| CLS | 0.0 | Good |
| TBT | 0 ms | Good |

**Key observation:** LCP is consistently null — Firefox headless may not fire LCP entries, or the PerformanceObserver isn't capturing them in this context.

## Performance Bottlenecks

### Navigation Timing
- **Homepage:** 518 ms total, 64 ms to interactive, 117 KB transfer
- **Search results:** 2364 ms total, 1229 ms to interactive, 130 KB transfer — **TTFB of 1129 ms is the bottleneck** (server-side search query)
- **Immobilien default:** 666 ms total, 32 ms to interactive

### Slowest Resources
1. **`/immobilien/node-api/decrypt`** — 1577 ms XHR (search results page). Backend decryption endpoint is very slow.
2. **`/immobilien/api/v1/singlepage/favorites`** — 1202 ms XHR. Favorites API call blocks rendering.
3. **`securepubads.g.doubleclick.net/tag/js/gpt.js`** — 418-761 ms. Google ad library.
4. **`suffix.data.comparis.ch/cmx/vid.js`** — 195 ms. Tracking script.
5. **`data.debugbear.com/`** — 916 ms XHR. Performance monitoring ironically being slow.
6. **TikTok analytics** — 797 ms.
7. **Bing tracking** — 645 ms.

### Third-Party Load
- Homepage: 58 of 65 resources (89%) are third-party, but only 3.6 KB transfer
- Search results: 78 of 88 resources (89%) are third-party
- Immobilien default: 79 of 87 resources (91%) are third-party

**The site loads a massive number of third-party scripts** — trackers, analytics, ad networks, consent managers, etc.

### Resource Breakdown (Homepage)
| Type | Count | Transfer Size |
|------|-------|---------------|
| JS | 36 | 16.3 KB |
| Image | 14 | 18.4 KB |
| XHR | 7 | 22.0 KB |
| CSS | 1 | 0 KB |
| Font | 4 | 0 KB |

## DOM Structure Observations

- **Next.js app** — Uses `__next` root div, `next-route-announcer`, server-side rendering
- **CSS-in-JS** — Class names like `css-1br6zhr`, `css-zjnj4j` (Emotion/Styled Components)
- **React portals** — Multiple `ReactModalPortal` divs
- **Consent Manager** — `cmpbox` consent dialog from ConsentManager.net (prominent overlay)
- **Node counts:** Homepage 2074, Search 1925, Detail 2180, Immobilien default 2481
- **Document sizes:** 600-725 KB (large HTML payloads)
- **Inline scripts:** 11-18 per page
- **Render-blocking resources:** 2-19 depending on page
- **Images without lazy loading:** 0-11 (detail page worst at 11)

## Accessibility Findings

### a11y Tree
- Firefox 149 broke the `getRootNode` RDP method — accessibility tree inspection via the AccessibilityActor is unavailable.

### Contrast Check (a11y contrast, JS-based)
- **7 elements sampled, all pass AA** (via JS evaluation fallback)
- Navigation links (`a.css-1wjfkmu`): color `#017b4f` on `#ffffff` — ratio 5.32 (passes AA but fails AAA for normal text)
- Comparis uses green-on-white throughout, which is borderline for contrast

### Structural A11y
- Proper ARIA roles: `banner` (header), `contentinfo` (footer), `dialog` (consent)
- Route announcer present for SPA navigation (`role="alert"`)
- 274 links on immobilien default page — potentially overwhelming for screen readers

## Concrete Improvement Recommendations

1. **Fix TTFB on search results** — 1129 ms server response time. Consider edge caching, pre-computed results, or async loading.
2. **Optimize `/node-api/decrypt` endpoint** — 1577 ms is unacceptable for a user-facing API. Investigate caching or moving decryption client-side.
3. **Defer third-party scripts** — 89-91% of resources are third-party. Use `async`/`defer` attributes; consider loading ad scripts after user interaction.
4. **Reduce render-blocking resources** — Up to 19 render-blocking items on some pages.
5. **Add lazy loading to images on detail page** — 11 images without `loading="lazy"`.
6. **Reduce inline scripts** — 11-18 inline scripts per page; bundle and defer where possible.
7. **Improve contrast for green text** — While passing AA, the green (#017b4f) on white is only 5.32:1. Consider darkening to improve accessibility.
8. **Reduce document size** — 600-725 KB HTML payloads are heavy; consider streaming SSR or code splitting.
