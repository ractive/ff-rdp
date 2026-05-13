---
title: "Dogfooding — when to use ff-rdp vs code-only debugging"
type: guide
date: 2026-05-14
tags:
  - dogfooding
  - process
  - ff-rdp
---

# When ff-rdp pays for itself

## The short answer

Use ff-rdp whenever the bug manifests in a running browser, and skip it when
the root cause is already obvious from source code.

## Decision table

| Situation | ff-rdp useful? | Alternative |
|-----------|---------------|-------------|
| Reproduce a real bug against a live page | **Yes** | curl / reading logs |
| Confirm a cookie or header is absent (like Set-Cookie) | **Yes — session 42 confirmed in <1min** | curl -i |
| Diagnose a CDN / auth / CORS issue | **Yes** | curl, browser DevTools |
| Suspect a React rendering bug | **Yes** (console errors, DOM state) | Console log spelunking |
| Fix is obvious from source and doesn't need browser state | No | Code review, grep |
| Pure refactor with no visible behavior change | No | cargo test |
| ChunkLoadError / deploy-skew symptoms | Maybe — captures the console error fast | DevTools |

## Session 42: the archetypal ff-rdp win

[[dogfooding-session-42]] had a real prod bug against admin.wardrobe-assistants.ch:
`Set-Cookie` was being stripped by bunny.net CDN. The full trace from suspicion
to root cause took ~6 minutes using ff-rdp. The key commands were:

```sh
ff-rdp navigate https://admin.wardrobe-assistants.ch/login
ff-rdp network --detail --filter /api/login
ff-rdp cookies
```

Without ff-rdp (e.g. with curl), the cookie stripping was also reproducible,
but `curl` wasn't available in the agent context and the auth flow required
a full browser session with CSP headers respected.

## Session 43: when ff-rdp was skipped unnecessarily

[[dogfooding-session-43]] implemented iter-39 (squad-member + auth onboarding)
without using ff-rdp. The §A.1 and §A.2 items were pure source-code changes
and needed no browser — that was the right call. But §A.3 (ChunkLoadError) and
§A.4 (Manifest syntax error) were diagnosed by inference from code alone,
without live console/network evidence. A 5-minute ff-rdp pass would have been
faster and more authoritative.

**Rule of thumb from session 43:** if an acceptance criterion says "verify in
browser" or the issue involves runtime behavior (network requests, cookies,
console errors, rendering), open a tab and capture a trace before spelunking
in source.

## In-loop checklist

When implementing a task whose ACs include browser verification:

1. `ff-rdp launch --headless --auto-consent` (or reuse a running instance)
2. `ff-rdp navigate <url>`
3. Capture evidence: `console`, `network --detail --filter <endpoint>`, `cookies`
4. Then write the code fix with authoritative evidence in hand

Skip steps 1–3 only when:
- The fix is purely additive/refactoring with no externally visible behavior
- The test suite provides adequate coverage without a live browser

## Skills integration

The `site-audit` and `dogfood` skills already invoke ff-rdp automatically.
The `impl` skill does not trigger ff-rdp automatically — that is intentional,
since most code changes don't need live browser evidence. But if a task's ACs
explicitly involve browser behavior, add `ff-rdp navigate + capture` to the
first step of the implementation plan.
