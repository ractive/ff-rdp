---
title: "Iteration 42: Site Audit Skill"
date: 2026-04-09
type: iteration
status: planned
branch: iter-42/site-audit-skill
tags: [iteration, feature, skill, audit, performance, accessibility, seo]
---

# Iteration 42: Site Audit Skill

Create a Claude Code skill that orchestrates ff-rdp commands to perform a comprehensive website audit. This solves the **discoverability problem**: without a skill, an LLM has no idea what ff-rdp commands exist or how to compose them.

**Key insight from [[dogfooding/dogfooding-session-36-comparison]]**: ff-rdp tested 33 commands vs Chrome MCP's 14 tools. The power is there, but only if the LLM knows the commands. A skill encodes the "playbook" so the LLM doesn't need to discover anything -- it just follows the recipe.

## What the Skill Does

When user says: `/site-audit https://www.comparis.ch` or "analyze this website" or "check the performance of this page"

The skill runs a structured audit across 6 categories, using ff-rdp commands:

### 1. Performance Audit
```bash
ff-rdp perf audit --format text        # Single-command overview with flagged issues
ff-rdp perf vitals --format text       # Core Web Vitals: LCP, CLS, FCP, TTFB, TBT
ff-rdp perf summary --format text      # Resource breakdown by domain and type
ff-rdp network --format text --limit 10 --sort duration --desc  # Slowest requests
```

### 2. Accessibility Audit
```bash
ff-rdp a11y contrast --fail-only --format text   # WCAG contrast failures
ff-rdp a11y --format text --limit 20             # A11y tree overview
ff-rdp dom stats --format text                   # Semantic HTML tag counts
ff-rdp eval 'document.querySelectorAll("img:not([alt])").length'  # Images without alt
```

### 3. SEO Audit
```bash
ff-rdp dom 'head title, head meta[name], head meta[property], head link[rel=canonical]' --format text
ff-rdp eval 'document.querySelectorAll("h1").length'          # H1 count (should be 1)
ff-rdp eval '!!document.querySelector("script[type=application/ld+json]")'  # Structured data
ff-rdp dom 'meta[property^="og:"]' --format text              # Open Graph tags
```

### 4. Security Audit
```bash
ff-rdp cookies --format text           # Cookie flags: httpOnly, secure, sameSite
ff-rdp eval 'location.protocol'        # HTTPS check
ff-rdp network --limit 5 --fields url,status,responseHeaders  # Response headers (CSP, HSTS)
```

### 5. Structure Audit
```bash
ff-rdp snapshot --format text --limit 50   # Page structure overview
ff-rdp sources --format text --limit 20    # JS sources loaded
ff-rdp dom stats --format text             # DOM complexity metrics
ff-rdp storage --format text               # localStorage/sessionStorage usage
```

### 6. Responsive Audit
```bash
ff-rdp responsive 'nav, main, .sidebar, footer' --widths 375,768,1024,1440 --format text
ff-rdp geometry 'header, nav, main, footer' --format text  # Layout geometry
```

## Skill Output Format

The skill produces a structured report with:
- **Score card**: Pass/Warn/Fail for each category
- **Key metrics**: CWV numbers, DOM size, resource count, a11y issues
- **Top issues**: Prioritized list of findings with severity
- **Recommendations**: Actionable improvement suggestions

## Eval System (Benchmarking ff-rdp)

The skill includes an eval suite to measure ff-rdp's effectiveness over time.

### Eval Design

Each eval is a **site + assertion set**:

```json
{
  "evals": [
    {
      "name": "comparis-search-results",
      "url": "https://www.comparis.ch/immobilien/result/list?...",
      "assertions": [
        {"category": "perf", "check": "ttfb_measured", "desc": "TTFB value is a positive number"},
        {"category": "perf", "check": "fcp_measured", "desc": "FCP value is a positive number"},
        {"category": "perf", "check": "cls_measured", "desc": "CLS value is a number >= 0"},
        {"category": "a11y", "check": "contrast_checked", "desc": "Contrast check returns results"},
        {"category": "seo", "check": "title_found", "desc": "Page title extracted"},
        {"category": "seo", "check": "meta_description_found", "desc": "Meta description extracted"},
        {"category": "structure", "check": "dom_stats_returned", "desc": "DOM stats available"},
        {"category": "security", "check": "cookies_listed", "desc": "Cookies enumerated"}
      ]
    },
    {
      "name": "wikipedia-article",
      "url": "https://en.wikipedia.org/wiki/Zurich",
      "assertions": [
        {"category": "a11y", "check": "heading_hierarchy", "desc": "Heading levels are sequential"},
        {"category": "seo", "check": "structured_data", "desc": "JSON-LD structured data present"},
        {"category": "structure", "check": "semantic_html", "desc": "Article/section/nav tags used"}
      ]
    },
    {
      "name": "github-repo",
      "url": "https://github.com/nickel-org/nickel.rs",
      "assertions": [
        {"category": "perf", "check": "resources_counted", "desc": "Resource count available"},
        {"category": "security", "check": "https_confirmed", "desc": "Served over HTTPS"},
        {"category": "seo", "check": "og_tags", "desc": "Open Graph tags present"}
      ]
    }
  ]
}
```

### Eval Runner

A script (or ff-rdp skill) that:
1. Launches Firefox with `ff-rdp launch --temp-profile --auto-consent`
2. For each eval: navigate to URL, run the audit skill, check assertions
3. Score: pass rate per category, overall pass rate, execution time
4. Compare: run same evals with Chrome MCP, diff the results

### What We're Measuring

Not "is the website good" but **"can ff-rdp extract the data"**:
- Can it measure TTFB? (binary: got a number or didn't)
- Can it find contrast issues? (binary: returned results or errored)
- Can it enumerate cookies? (binary: got cookie list or failed)
- How long did each command take? (performance regression detection)
- Did any commands error out? (reliability tracking)

Over time, as we fix bugs and add features, the pass rate should go up.

## "WohnungsDirekt" — Broken Page Fixture

A deliberately broken apartment listing page that serves as both a demo and an eval. The page looks real (a Swiss apartment detail page for a 3.5-room flat in Zurich Seefeld) but contains **33 planted issues** across 3 difficulty levels.

### Why This Is Powerful

The audit → fix → verify loop is the killer demo:
1. **`/site-audit`** finds all 33 issues automatically
2. LLM reads the HTML source, maps findings to specific elements
3. LLM fixes each issue with the Edit tool
4. **`/site-audit`** re-runs, confirms issue count drops to zero

This is something Chrome MCP **cannot do** — it has no equivalent of `perf audit` or `a11y contrast` to detect issues programmatically.

### Fixture Structure

```
tests/fixtures/wohnungsdirekt/
├── index.html          # Main page — the 33-bug apartment listing
├── style.css           # External stylesheet (some issues here too)
├── app.js              # Broken JS: console errors, sync blocking
├── analytics.js        # Sets insecure cookies, mixed content
├── hero.bmp            # Deliberately unoptimized image (BMP, no dimensions)
├── floor-plan.jpg      # Missing alt text target
├── serve.sh            # `python3 -m http.server 8787` one-liner
└── issues.json         # Ground truth: all 33 issues with locations
```

Must be served via HTTP (not `file://`) because cookies, Performance API timing, and mixed content detection require it.

### Issue Catalogue (33 Issues, 3 Levels)

#### Level 1 — HTML Hygiene (13 issues)
These are obvious and teach basic web quality:

| # | Category | Issue | ff-rdp Detection |
|---|----------|-------|-----------------|
| 1 | SEO | Missing `<title>` tag | `dom 'head title'` returns empty |
| 2 | SEO | No `<meta name="description">` | `dom 'head meta[name=description]'` returns empty |
| 3 | SEO | Duplicate `<h1>` tags (2 on page) | `eval 'document.querySelectorAll("h1").length'` → 2 |
| 4 | Structure | No `lang` attribute on `<html>` | `snapshot` shows missing lang |
| 5 | Structure | All-div soup (no `<main>`, `<nav>`, `<article>`, `<section>`) | `dom stats` shows 0 semantic elements |
| 6 | Structure | 50+ inline `style=""` attributes | `dom stats` counts inline styles |
| 7 | A11y | Missing alt text on 3 images | `eval 'document.querySelectorAll("img:not([alt])").length'` → 3 |
| 8 | A11y | Broken heading hierarchy: h1 → h4 → h2 → h6 | `a11y` tree shows jumps |
| 9 | A11y | Missing skip navigation link | `dom 'a[href="#main-content"]'` returns empty |
| 10 | Perf | Render-blocking `<script src>` in `<head>` (no defer/async) | `perf audit` flags it |
| 11 | Perf | No `loading="lazy"` on below-fold images | `perf audit` flags it |
| 12 | Perf | No viewport meta tag | `dom 'meta[name=viewport]'` returns empty |
| 13 | Structure | Console errors from bad JS (`ReferenceError`) | `console` shows errors |

#### Level 2 — WCAG & Web Vitals (12 issues)
Require deeper knowledge:

| # | Category | Issue | ff-rdp Detection |
|---|----------|-------|-----------------|
| 14 | A11y | Bad contrast: #999 text on #fff background | `a11y contrast --fail-only` catches it |
| 15 | A11y | Custom dropdown with zero ARIA (div-based select) | `a11y` tree shows no role |
| 16 | A11y | Form inputs without `<label>` | `a11y` shows unlabeled inputs |
| 17 | A11y | Keyboard trap: `tabindex="1"` on decorative element | `snapshot --interactive` shows bad tabindex |
| 18 | A11y | Link with `target="_blank"` missing `rel="noopener"` | `dom 'a[target=_blank]:not([rel*=noopener])'` |
| 19 | SEO | No canonical URL | `dom 'link[rel=canonical]'` returns empty |
| 20 | SEO | No Open Graph tags | `dom 'meta[property^="og:"]'` returns empty |
| 21 | Perf | Unoptimized BMP image (should be WebP/AVIF) | `perf audit` flags large image |
| 22 | Perf | Image without explicit width/height → CLS | `perf vitals` shows CLS > 0 |
| 23 | UX | Tiny click targets (12px links in footer) | `geometry 'footer a'` shows small rects |
| 24 | Security | Cookie set without `Secure` flag | `cookies` shows missing flag |
| 25 | Security | Cookie without `HttpOnly` | `cookies` shows missing flag |

#### Level 3 — Subtle & Advanced (8 issues)
Require security/perf expertise:

| # | Category | Issue | ff-rdp Detection |
|---|----------|-------|-----------------|
| 26 | Perf | Late-injected banner causing layout shift (CLS) | `perf vitals` shows CLS spike |
| 27 | Perf | `font-display: block` causing FOIT | `styles` shows font-display value |
| 28 | Perf | 800+ DOM nodes (excessive nesting) | `dom stats` shows high count |
| 29 | SEO | Accidental `<meta name="robots" content="noindex">` | `dom 'meta[name=robots]'` shows noindex |
| 30 | SEO | No JSON-LD structured data | `eval` checks for script[type=application/ld+json] |
| 31 | Security | Mixed content: HTTP image on HTTPS page | `console` shows mixed content warning |
| 32 | Security | No CSP header | `network` response headers lack CSP |
| 33 | UX | Fixed-width table causing horizontal overflow on mobile | `responsive` at 375px shows overflow |

### Ground Truth File (`issues.json`)

```json
{
  "total": 33,
  "levels": {"1": 13, "2": 12, "3": 8},
  "issues": [
    {
      "id": 1, "level": 1, "category": "seo",
      "summary": "Missing <title> tag",
      "element": "head",
      "detection_command": "ff-rdp dom 'head title'",
      "expected_result": "empty results",
      "fix": "Add <title>3.5-Zimmer-Wohnung in Zürich Seefeld | WohnungsDirekt</title>"
    }
  ]
}
```

This doubles as the eval's assertion source — run the detection command, check against expected result, score pass/fail.

### The Fix Workflow (Step by Step)

```
User: /site-audit http://localhost:8787

[Skill runs ~15 ff-rdp commands, produces report]

Report: 33 issues found
  Performance: 6 issues (render-blocking script, no lazy loading, BMP image, ...)
  Accessibility: 9 issues (missing alt, bad contrast, no ARIA, ...)
  SEO: 7 issues (no title, no description, duplicate h1, ...)
  ...

User: Fix all the issues

[LLM reads index.html, style.css, app.js]
[LLM applies fixes with Edit tool]
[LLM runs: ff-rdp reload]
[LLM runs: /site-audit http://localhost:8787 again]

Report: 0 issues found ✅
```

### Scoring the Fix

The eval measures:
- **Detection rate**: How many of the 33 issues did `/site-audit` find? (target: 100%)
- **Fix rate**: How many did the LLM successfully fix? (target: >90%)
- **Regression rate**: Did any fix introduce new issues? (target: 0%)
- **Token cost**: How many tokens for the full audit → fix → verify loop?

## Tasks

### Skill Creation
- [ ] Create `.claude/skills/site-audit/SKILL.md` with full audit playbook
- [ ] Define triggering contexts: `/site-audit`, "analyze this website", "check performance", "audit this page", "web vitals", "accessibility check"
- [ ] Include ff-rdp command reference inline (subset of `llm-help` output)
- [ ] Add example outputs for each audit category
- [ ] Add `--quick` mode (perf + a11y only) vs `--full` mode (all 6 categories)

### WohnungsDirekt Fixture
- [ ] Create `tests/fixtures/wohnungsdirekt/index.html` — realistic apartment listing with all 33 issues
- [ ] Create `style.css`, `app.js`, `analytics.js` with their respective issues
- [ ] Create `serve.sh` (one-liner python HTTP server on port 8787)
- [ ] Create `issues.json` — ground truth with detection commands and expected fixes
- [ ] Verify all 33 issues are detectable by ff-rdp commands
- [ ] Test the full audit → fix → verify loop end-to-end

### Eval Suite
- [ ] Create `.claude/skills/site-audit/evals/evals.json` with WohnungsDirekt + 3 real sites
- [ ] Create eval runner script in `.claude/skills/site-audit/scripts/run_evals.sh`
- [ ] Define assertions: detection rate, fix rate, regression rate, token cost
- [ ] Add timing measurement per command
- [ ] Create comparison mode: ff-rdp vs Chrome MCP on same sites

### Acceptance Criteria
- [ ] Skill triggers on "analyze this website", "/site-audit", "check performance"
- [ ] Produces structured report with scores for 6 categories
- [ ] All ff-rdp commands in the playbook work without errors on test sites
- [ ] WohnungsDirekt: `/site-audit` finds all 33 issues
- [ ] WohnungsDirekt: LLM can fix all issues, re-audit shows 0 remaining
- [ ] Eval suite runs end-to-end with pass/fail per assertion
- [ ] Eval results are saved as JSON for trend analysis
