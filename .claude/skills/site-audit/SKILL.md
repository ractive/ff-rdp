---
name: site-audit
user_invocable: true
description: >
  Perform a comprehensive website audit using ff-rdp commands. Orchestrates Performance,
  Accessibility, SEO, Security, Structure, and Responsive checks into a structured report.
  Trigger on: /site-audit <url>, "analyze this website", "audit this page",
  "check performance", "web vitals check", "accessibility check", "SEO audit", "site audit".
  Accepts a URL (navigates if given) or operates on the currently-focused tab.
  Supports --quick (perf + a11y only) and --full (all 6 categories, default).
---

# Site Audit Skill

Run a structured website audit across 6 categories using ff-rdp commands. Produces a
score card, key metrics, top issues (severity-sorted), and recommendations.

## Usage

```
/site-audit <url>           # Navigate to URL and audit (full mode)
/site-audit --quick <url>   # Performance + Accessibility only
/site-audit                 # Audit the currently-focused tab
```

If a URL is provided, navigate first:
```bash
ff-rdp navigate <url> --wait-timeout 10000
```

## Audit Categories

### 1. Performance Audit

```bash
ff-rdp perf audit --format text        # Overview with flagged issues
ff-rdp perf vitals --format text       # Core Web Vitals: LCP, CLS, FCP, TTFB, TBT
ff-rdp perf summary --format text      # Resource breakdown by domain and type
ff-rdp network --format text --limit 10 --sort duration --desc  # Slowest requests
```

Example output (perf vitals):
```json
{"results": {"lcp_ms": 1250, "cls": 0.12, "fcp_ms": 480, "ttfb_ms": 85, "tbt_ms": 30},
 "meta": {"ratings": {"lcp": "needs-improvement", "cls": "poor", "fcp": "good"}}}
```

Score: PASS if LCP<2500ms, CLS<0.1, FCP<1800ms. WARN if borderline. FAIL otherwise.

### 2. Accessibility Audit

```bash
ff-rdp a11y contrast --fail-only --format text   # WCAG contrast failures
ff-rdp a11y --format text --limit 20             # A11y tree overview
ff-rdp dom stats --format text                   # Semantic HTML tag counts
ff-rdp eval 'document.querySelectorAll("img:not([alt])").length'  # Images without alt
```

Additional checks:
```bash
ff-rdp dom 'a[href="#main-content"]'              # Skip nav link
ff-rdp dom '[tabindex]:not([tabindex="0"]):not([tabindex="-1"])' --attrs  # Bad tabindex
ff-rdp dom 'a[target=_blank]:not([rel*=noopener])' --attrs  # Unsafe blank targets
ff-rdp eval 'Array.from(document.querySelectorAll("h1,h2,h3,h4,h5,h6")).map(h=>h.tagName).join(",")'  # Heading hierarchy
ff-rdp eval 'document.querySelectorAll("label").length'  # Label count vs inputs
```

Example output (a11y contrast --fail-only):
```json
{"results": [{"selector": "p.low-contrast", "ratio": 2.85, "required": 4.5, "level": "AA"}],
 "total": 1}
```

Score: PASS if 0 contrast failures + all images have alt + headings sequential. FAIL otherwise.

### 3. SEO Audit

```bash
ff-rdp dom 'head title' --text                    # Page title
ff-rdp dom 'head meta[name=description]' --attrs  # Meta description
ff-rdp eval 'document.querySelectorAll("h1").length'  # H1 count (should be 1)
ff-rdp dom 'link[rel=canonical]' --attrs          # Canonical URL
ff-rdp dom 'meta[property^="og:"]' --attrs        # Open Graph tags
ff-rdp dom 'meta[name=robots]' --attrs            # Robots directive
ff-rdp eval '!!document.querySelector("script[type=\"application/ld+json\"]")'  # Structured data
```

Example output (dom 'head title' --text):
```json
{"results": ["3.5-Zimmer-Wohnung in Zürich Seefeld | WohnungsDirekt"], "total": 1}
```

Score: PASS if title present + exactly 1 h1 + meta description + no noindex. WARN if missing
canonical/OG/JSON-LD. FAIL if no title or noindex.

### 4. Security Audit

```bash
ff-rdp cookies --format text                      # Cookie flags: httpOnly, secure, sameSite
ff-rdp eval 'location.protocol'                   # HTTPS check
ff-rdp network --fields url,responseHeaders --detail --limit 1  # Response headers (CSP, HSTS)
```

Example output (cookies):
```json
{"results": [{"name": "session", "secure": false, "httpOnly": false, "sameSite": "None"}],
 "total": 1}
```

Score: PASS if HTTPS + all cookies secure/httpOnly + CSP present. WARN if missing CSP/HSTS.
FAIL if insecure cookies on HTTPS.

### 5. Structure Audit

```bash
ff-rdp snapshot --format text --limit 50           # Page structure overview
ff-rdp sources --format text --limit 20            # JS sources loaded
ff-rdp dom stats --format text                     # DOM complexity metrics
ff-rdp storage --format text                       # localStorage/sessionStorage
ff-rdp console --level error --format text         # JS errors
ff-rdp eval 'document.documentElement.lang'        # Language attribute
```

Example output (dom stats):
```json
{"results": {"node_count": 1247, "element_count": 623, "inline_styles": 518,
 "tags": {"div": 210, "li": 120, "span": 45, "p": 12, "h1": 2}}}
```

Score: PASS if lang set + semantic HTML used + <1500 DOM nodes + 0 JS errors. WARN if
800+ nodes or missing semantics. FAIL if JS errors.

### 6. Responsive Audit

```bash
ff-rdp responsive 'nav, main, .sidebar, footer, table' --widths 375,768,1024,1440 --format text
ff-rdp geometry 'header, nav, main, footer' --format text  # Layout geometry
```

Example output (responsive):
```json
{"results": {"breakpoints": [
  {"width": 375, "elements": [{"selector": "table", "rect": {"width": 2000}, "overflow": true}]},
  {"width": 1440, "elements": [{"selector": "table", "rect": {"width": 2000}}]}
]}}
```

Score: PASS if no overflow at 375px + elements reflow properly. FAIL if horizontal overflow.

## Quick Mode (--quick)

Run only categories 1 (Performance) and 2 (Accessibility). Skip SEO, Security, Structure,
Responsive. Useful for rapid iteration during development.

## Report Format

After running all commands, synthesize findings into this structure:

### Score Card

| Category | Status | Issues |
|----------|--------|--------|
| Performance | FAIL | 6 issues |
| Accessibility | FAIL | 9 issues |
| SEO | FAIL | 7 issues |
| Security | WARN | 3 issues |
| Structure | FAIL | 5 issues |
| Responsive | FAIL | 1 issue |

### Key Metrics

- LCP: 1250ms (needs improvement)
- CLS: 0.12 (poor)
- DOM nodes: 1247 (warning)
- Images without alt: 3
- Contrast failures: 2
- JS errors: 1

### Top Issues (severity-sorted)

1. **CRITICAL** [SEO] `<meta name="robots" content="noindex">` blocks indexing
2. **HIGH** [Perf] Render-blocking script in `<head>` without defer/async
3. **HIGH** [A11y] 3 images missing alt text
4. **HIGH** [SEO] Missing `<title>` tag
5. ... (continue for all found issues)

### Recommendations

Actionable steps grouped by priority. Example:
- **Immediate**: Remove noindex, add title/description, fix render-blocking script
- **Important**: Add alt text, fix contrast, add viewport meta
- **Nice-to-have**: Add JSON-LD, canonical URL, optimize images

## WohnungsDirekt Fixture (Built-in Demo)

A deliberately broken apartment listing page with 33 planted issues. Located at
`tests/fixtures/wohnungsdirekt/`. Serve it:

```bash
cd tests/fixtures/wohnungsdirekt && ./serve.sh
# Then:
ff-rdp navigate http://localhost:8787
```

### The Fix Loop

1. Run `/site-audit http://localhost:8787` — finds ~33 issues
2. Read index.html, style.css, app.js, analytics.js
3. Fix each issue with the Edit tool
4. Run `ff-rdp reload` then `/site-audit` again
5. Confirm issue count drops toward 0

Ground truth is in `tests/fixtures/wohnungsdirekt/issues.json` (all 33 issues with
detection commands and expected results).

## ff-rdp Command Reference (Subset)

### Navigation & Page
| Command | Description |
|---------|-------------|
| `navigate <url>` | Navigate to URL. Flags: `--wait-text`, `--wait-selector`, `--with-network` |
| `reload` | Reload current page |
| `tabs` | List open tabs |

### DOM & Content
| Command | Description |
|---------|-------------|
| `dom <selector>` | Query DOM elements. Flags: `--text`, `--attrs`, `--inner-html`, `--outer-html`, `--count` |
| `dom stats` | DOM statistics: node count, tag breakdown, inline styles |
| `eval <js>` | Evaluate JavaScript expression |
| `snapshot` | Structured page snapshot. Flags: `--depth`, `--interactive` |
| `page-text` | Extract visible text |

### Performance
| Command | Description |
|---------|-------------|
| `perf audit` | Performance audit with flagged issues |
| `perf vitals` | Core Web Vitals: LCP, CLS, FCP, TTFB, TBT |
| `perf summary` | Resource breakdown by domain and type |
| `network` | Network requests. Flags: `--filter`, `--method`, `--fields`, `--sort`, `--desc`, `--limit`, `--detail` |

### Accessibility
| Command | Description |
|---------|-------------|
| `a11y` | Accessibility tree. Flags: `--depth`, `--selector`, `--interactive` |
| `a11y contrast` | WCAG contrast check. Flags: `--fail-only`, `--selector` |

### Security & Storage
| Command | Description |
|---------|-------------|
| `cookies` | List cookies with flags (secure, httpOnly, sameSite) |
| `storage` | Read localStorage/sessionStorage |
| `console` | Console messages. Flags: `--level`, `--pattern` |

### Layout
| Command | Description |
|---------|-------------|
| `geometry <selectors>` | Element bounding rects, visibility, overlap |
| `responsive <selectors>` | Test layout across widths. Flags: `--widths` |
| `styles <selector>` | CSS styles. Flags: `--applied`, `--layout` |
| `sources` | JS/WASM sources loaded |

### Global Flags
| Flag | Description |
|------|-------------|
| `--format text` | Human-readable table output |
| `--jq <expr>` | jq filter on JSON output |
| `--limit N` | Limit results |
| `--all` | Return all results |
| `--sort <field>` | Sort by field |
| `--desc` / `--asc` | Sort direction |
| `--timeout <ms>` | Operation timeout (default 5000) |
