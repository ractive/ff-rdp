# WohnungsDirekt — Broken Page Fixture

A deliberately broken Swiss apartment listing page with **33 planted issues** across 3 difficulty levels, designed as a demo and eval target for the `/site-audit` skill.

See [[iterations/iteration-42-site-audit-skill]] for the full plan.

## Quick Start

```bash
# Serve the fixture
cd tests/fixtures/wohnungsdirekt
./serve.sh   # or: python3 -m http.server 8787

# Run the audit
ff-rdp navigate http://localhost:8787
ff-rdp perf audit --format text
ff-rdp a11y contrast --fail-only --format text
# ... or use /site-audit http://localhost:8787
```

## The Audit-Fix-Verify Loop

1. `/site-audit http://localhost:8787` finds all 33 issues
2. LLM reads source files, maps findings to specific elements
3. LLM fixes each issue with the Edit tool
4. `ff-rdp reload` then `/site-audit` again — confirms issue count drops to 0

## Files

| File | Purpose |
|------|---------|
| `index.html` | Main page with 33 planted issues |
| `style.css` | External stylesheet (font-display:block, low contrast, wide table) |
| `app.js` | Broken JS (ReferenceError) + late-injected CLS banner |
| `analytics.js` | Insecure cookies + mixed content tracking pixel |
| `hero.bmp` | Deliberately unoptimized BMP image |
| `floor-plan.jpg` | Target for missing alt text |
| `serve.sh` | One-liner HTTP server on port 8787 |
| `issues.json` | Ground truth: all 33 issues with detection commands |

## Issue Levels

- **Level 1 (13 issues)**: HTML hygiene — missing title, no viewport, duplicate h1, etc.
- **Level 2 (12 issues)**: WCAG & Web Vitals — bad contrast, no ARIA, insecure cookies, etc.
- **Level 3 (8 issues)**: Subtle/advanced — CLS from late injection, noindex, no CSP, etc.

## Caveats

- **Mixed content (#31)**: Only triggers browser warnings when served over HTTPS
- **HttpOnly cookies (#25)**: Cannot be set from JavaScript — the fix requires server-side code
- **CSP header (#32)**: python3 http.server doesn't set CSP — detection verifies absence
- **CLS (#26)**: Banner injects after 2 seconds — wait before measuring perf vitals
