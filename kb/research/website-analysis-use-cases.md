---
title: Website Analysis Use Cases for AI Coding Assistants
status: completed
created: 2026-04-09
tags:
  - research
  - skill-design
  - use-cases
---

# Website Analysis Use Cases for ff-rdp Skill Design

Research into what developers and AI coding assistants actually do when analyzing websites, to inform the design of an ff-rdp skill.

## Category 1: Code/Structure Analysis

**Importance:** Medium-High. Developers frequently ask AI to inspect page structure for debugging, reverse-engineering, and understanding unfamiliar codebases.

**Typical workflow:** Load page -> inspect DOM tree -> identify framework -> analyze component hierarchy -> check HTML semantics

**What people want:**
- HTML structure: heading hierarchy (H1-H6), semantic elements (`<nav>`, `<main>`, `<article>`, `<section>`)
- JS framework detection (React, Vue, Angular, Svelte — detectable via globals, DOM attributes like `data-reactroot`, `__vue_app__`)
- Component tree / shadow DOM inspection
- CSS framework detection (Tailwind classes, Bootstrap grid, etc.)
- Bundle analysis: how many JS/CSS files, sizes, third-party scripts
- Inline vs external resources
- Console errors and warnings

**ff-rdp data sources:** DOM inspection, `Runtime.evaluate`, network request list, console messages

---

## Category 2: Performance Analysis

**Importance:** Very High. This is the #1 automated audit category. Lighthouse alone has 38 performance audits.

**Typical workflow:** Load page -> measure Core Web Vitals -> analyze resource waterfall -> identify render-blocking resources -> check image optimization -> report

**What people want:**
- **Core Web Vitals** (the big 3):
  - LCP (Largest Contentful Paint): target <= 2.5s
  - INP (Interaction to Next Paint): target <= 200ms
  - CLS (Cumulative Layout Shift): target <= 0.1
- **Supporting metrics:**
  - TTFB (Time to First Byte): target < 800ms
  - FCP (First Contentful Paint): target < 1.8s
  - Speed Index: how quickly visible content populates
  - Total Blocking Time (TBT): JS execution blocking main thread
  - DOM size (node count)
- **Resource analysis:**
  - Request waterfall (what loads when, dependencies)
  - Render-blocking CSS/JS
  - Unminified/uncompressed resources
  - Image optimization (format, dimensions, lazy loading)
  - JS/CSS bundle sizes (budget: JS < 300KB compressed, images < 500KB above-fold)
  - Third-party script impact
  - Cache headers
  - Compression (gzip/brotli)

**ff-rdp data sources:** Performance API (`performance.getEntries()`), network events, DOM metrics, `Runtime.evaluate` for CWV library

---

## Category 3: Accessibility Analysis

**Importance:** High. axe-core detects ~57% of WCAG issues automatically. The rest needs human judgment.

**Typical workflow:** Load page -> run automated checks -> report violations with severity -> suggest fixes

**What people want (axe-core rules map to WCAG 2.0/2.1/2.2 A/AA/AAA):**
- **Structure:** page has H1, heading hierarchy is logical, landmarks present (`<main>`, `<nav>`, `<banner>`)
- **Images:** all `<img>` have alt text, decorative images have `alt=""`
- **Color:** sufficient contrast ratios (4.5:1 for normal text, 3:1 for large text)
- **ARIA:** correct roles, valid attributes, labels present, no misuse
- **Keyboard:** all interactive elements focusable, visible focus indicator, logical tab order
- **Forms:** labels associated with inputs, error messages accessible, required fields indicated
- **Language:** `lang` attribute on `<html>`
- **Links:** descriptive link text (not "click here")
- **Tables:** proper `<th>`, `<caption>`, `scope` attributes

**ff-rdp data sources:** Accessibility actor (a11y tree), DOM inspection, computed styles, `Runtime.evaluate` for contrast checking

---

## Category 4: SEO Analysis

**Importance:** High for marketing/content sites. The seo-audit-skill project has 251 rules across 20 categories.

**Typical workflow:** Load page -> check meta tags -> validate structured data -> check links -> report issues

**What people want:**
- **Meta tags:** title (50-60 chars), description (150-155 chars), viewport, charset
- **Headings:** exactly one H1 with primary keyword, logical H2-H3 structure
- **Canonical URL:** present, correct, no conflicts
- **Open Graph:** og:title, og:description, og:image, og:url
- **Twitter Cards:** twitter:card, twitter:title, twitter:description
- **Structured data:** JSON-LD schema.org markup (Organization, Product, FAQ, BreadcrumbList, etc.)
- **Robots:** meta robots tag, robots.txt, X-Robots-Tag header
- **Links:** internal link structure, broken links, anchor text quality, nofollow usage
- **Images:** alt text, descriptive filenames, dimensions specified
- **URL structure:** clean, descriptive, no query strings for canonical content
- **Mobile:** viewport configured, font sizes readable, tap targets sized correctly

**ff-rdp data sources:** DOM inspection (`<head>` elements), network headers, `Runtime.evaluate` for structured data extraction

---

## Category 5: Security Analysis

**Importance:** Medium-High. Often part of "best practices" audits. 16 rules in the seo-audit-skill security category.

**Typical workflow:** Load page -> check response headers -> analyze cookies -> check HTTPS -> report

**What people want:**
- **HTTPS:** all resources loaded over HTTPS, no mixed content
- **Security headers:**
  - `Content-Security-Policy` (CSP): present, properly restrictive
  - `Strict-Transport-Security` (HSTS): present, max-age sufficient
  - `X-Frame-Options`: DENY or SAMEORIGIN
  - `X-Content-Type-Options`: nosniff
  - `Referrer-Policy`: appropriate value
  - `Permissions-Policy`: restricting APIs
- **Cookies:** Secure flag, HttpOnly flag, SameSite attribute
- **Mixed content:** HTTP resources on HTTPS pages
- **Form security:** forms submit over HTTPS, autocomplete attributes
- **SSL/TLS:** valid certificate, not expired

**ff-rdp data sources:** Network response headers, cookie inspection, `Runtime.evaluate` for mixed content detection

---

## Category 6: UX Analysis

**Importance:** Medium. Harder to automate — many aspects need human judgment. But some signals are measurable.

**Typical workflow:** Load page -> check responsive behavior -> analyze interactive elements -> check loading states -> report

**What people want:**
- **Responsive design:** viewport meta, media queries present, content readable at various widths
- **Interactive elements:** buttons/links have adequate tap targets (>= 48x48px), hover states
- **Forms:** proper labels, validation, helpful error messages, autofill support
- **Loading states:** skeleton screens, spinners, progressive loading
- **Navigation:** clear hierarchy, breadcrumbs, sitemap
- **Typography:** readable font sizes (>= 16px body), line height, contrast
- **Layout shifts:** elements jumping around during load (CLS)
- **Print stylesheet:** presence and quality

**ff-rdp data sources:** DOM inspection, computed styles, viewport emulation, screenshot comparison

---

## Output Formats People Want

Based on existing tools (seo-audit-skill, Lighthouse, web-quality-skills):

1. **JSON** — for programmatic consumption, CI/CD pipelines, `--jq` filtering
2. **Markdown** — human-readable reports
3. **Console/terminal** — colorized summary with pass/fail/warning
4. **LLM-optimized** — structured text that AI can reason about (what seo-audit-skill calls "XML format")

**Issue severity levels:** Critical / Warning / Info (or Pass/Fail)

**Key pattern:** Tools produce structured findings with:
- Category
- Rule name
- Severity
- Current value vs. expected value
- Actionable recommendation

---

## Eval Frameworks & Benchmarks

How people measure if a browser automation tool is good:

### WebArena
- 812 tasks across 4 domains (e-commerce, forums, code collaboration, CMS)
- Natural language intents ("Find when your last order was shipped")
- Measures functional correctness (did it achieve the goal?)
- Top LLMs: ~54.8% success rate

### BU Bench (Browser Use)
- 100 hand-selected tasks from 5 established sources
- Binary true/false verdicts (rubric scoring unreliable)
- Multiple runs per task for statistical significance

### Web Bench (Skyvern)
- 5,750 tasks on 452 websites
- Expanded from WebVoyager benchmark

### Key Quality Metrics
- **Success rate:** Did it accomplish the goal?
- **Efficiency:** Steps/time taken
- **Safety:** Did it avoid mistakes?
- **Reliability:** Consistency across runs
- **Zero false positives:** axe-core's standard — only report real issues

---

## Relevance to ff-rdp Skill Design

### What ff-rdp can uniquely provide (via Firefox RDP):
1. **DOM inspection** — full tree, attributes, computed styles
2. **Network monitoring** — request/response headers, timing, sizes
3. **Console messages** — errors, warnings, logs
4. **Accessibility tree** — via a11y actor
5. **JavaScript evaluation** — run arbitrary JS to extract data
6. **Screenshots** — visual state capture
7. **Cookie/storage inspection** — via storage actor
8. **Performance metrics** — via Performance API evaluation

### Suggested skill subcommands (priority order):
1. `audit` — comprehensive multi-category audit (like Lighthouse but via RDP)
2. `perf` — Core Web Vitals + resource analysis
3. `a11y` — accessibility audit
4. `seo` — meta tags, structured data, link analysis
5. `security` — headers, cookies, HTTPS, CSP
6. `structure` — DOM tree, framework detection, component hierarchy
7. `screenshot` — visual state capture (already exists)

### What NOT to build (better done by existing tools):
- Full Lighthouse equivalent (too complex, their scoring is proprietary)
- Link crawling / multi-page audits (ff-rdp is single-page focused)
- Network throttling simulation (Lighthouse does this with calibration)

---

## Sources

- [Browser automation tools for Claude Code](https://dev.to/minatoplanb/i-tested-every-browser-automation-tool-for-claude-code-heres-my-final-verdict-3hb7)
- [Chrome DevTools MCP](https://developer.chrome.com/blog/chrome-devtools-mcp)
- [Lighthouse overview](https://developer.chrome.com/docs/lighthouse/overview/)
- [axe-core](https://github.com/dequelabs/axe-core)
- [SEO audit skill (251 rules)](https://github.com/seo-skills/seo-audit-skill)
- [Web quality skills (Addy Osmani)](https://github.com/addyosmani/web-quality-skills)
- [WebArena benchmark](https://webarena.dev/)
- [Browser Use benchmark](https://browser-use.com/posts/ai-browser-agent-benchmark)
- [Web Bench (Skyvern)](https://www.skyvern.com/blog/web-bench-a-new-way-to-compare-ai-browser-agents/)
- [Website usability audit skill](https://mcpmarket.com/tools/skills/website-usability-auditor)
- [Claude SEO skill](https://github.com/AgriciDaniel/claude-seo)
- [WebPageTest metrics](https://www.debugbear.com/software/webpagetest)
- [csp-toolkit](https://chs.us/2026/03/csp-toolkit/)
