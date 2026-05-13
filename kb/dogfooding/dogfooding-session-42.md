---
title: "Dogfooding Session 42"
type: dogfooding
date: 2026-05-12
status: completed
site: "https://admin.wardrobe-assistants.ch"
commands_tested: [doctor, tabs, navigate, wait, snapshot, dom, type, click, network, console, cookies, daemon]
tags: [dogfooding, real-bug-hunt, cookies, network, csp, friction-click-selector-flag]
---

# Dogfooding Session 42

Used ff-rdp to chase a real production bug — admin login was silently failing for a user. "Click submit → nothing happens, no error." Ran end-to-end through the tool to isolate the failure point. Bug root-caused in ~12 commands, but the path surfaced two friction points and one nice-to-have gap.

## The Bug (Subject of the Debug Session)

`admin.wardrobe-assistants.ch` login form does nothing on submit for a real squad-member account. Symptoms reported by the user were vague ("Nothing happens, no error"). Hypothesis space at start: JS error blocking the submit handler, CORS rejection, cookie-set failure, CSRF, rate limiting (already ruled out by user via pod restart).

## What ff-rdp Did Well

**`snapshot` is the single most useful command for "I just landed on an unfamiliar page."** First call after `navigate` gave me the full semantic DOM tree with `interactive` flags, input names, button labels — exactly what I needed to find the submit button and credential fields without scraping HTML manually. Felt like reading a page through an a11y lens.

**`console` immediately surfaced the wrong-tree hypothesis.** Inside 1 call I saw:

```json
{
  "level": "error",
  "message": "Content-Security-Policy: ... blocked a JavaScript eval ...
   script-src 'self' 'nonce-...' 'strict-dynamic' (missing 'unsafe-eval')",
  "source": "/_next/static/chunks/0qyq6ist3tdw6.js"
}
```

I almost stopped there and reported "CSP regression broke the bundle." Turned out to be a red herring (Zod v4's `allowsEval` probe — they `try`/`catch` the `new Function("")` but the `securitypolicyviolation` event fires regardless), but the speed of pulling that signal was great.

**`network --detail --filter` after the second submit was decisive.** With watcher source (daemon mode auto-active), POST `/api/auth/sign-in/email` came back as method=`POST`, status=`200`. Server accepts. So the bug had to be client-side / cookie-layer.

**`cookies` (zero results) was the breakthrough.** Listing the Firefox cookie jar after a 200 OK login → empty. That + a curl against the same endpoint showed there is **no `Set-Cookie` header in the response at all** (bunny.net CDN strips it on the cached path). Real bug isolated in ~5 minutes of clicking around.

## Friction Points

### Inconsistent `--selector` ergonomics across commands

- `wait --selector 'input[name="email"]'` ✓
- `type --selector 'input[name="email"]' --text 'foo'` ✓
- `click --selector 'button[type="submit"]'` ✗ — errors with `unexpected argument '--selector'`. `click` takes the selector **positionally** (`click 'button[type=submit]'`).

The error message helpfully suggested `-- --selector`, but the right tip is "selector is positional here." Three commands that take CSS selectors, two named the flag, one didn't. Small thing, but it broke flow.

**Suggestion:** either accept `--selector` everywhere as an alias for the positional arg, or document the divergence prominently in the top-level `--help`.

### `network` Performance-API fallback gives misleading method/status

First time I queried `network --filter sign-in --detail` (before the click had drained through the watcher), I got:

```json
{
  "method": "GET",
  "status": null,
  "source": "performance-api",
  "url": ".../api/auth/sign-in/email"
}
```

That sent me looking at routing/CORS for several seconds — was the form doing a GET instead of POST? Then I noticed `source: "performance-api"` and realized the Resource Timing API doesn't carry method/status, just shape. The help text *does* explain this, but the `method: "GET"` value is sticky in your eye.

**Suggestion:** when `source=performance-api`, output `method` as `null` or `"unknown"` rather than the Resource Timing default "GET". A subtle visual nudge would prevent the misread. Alternatively, hoist a `warning` field into `meta` saying "values from performance-api lack HTTP method/status fidelity."

### `wait --selector 'input[type="email"]' --timeout 10` timed out on a page that had the input

After `navigate https://admin.wardrobe-assistants.ch/login` the wait command returned `internal error: operation timed out` even though the next call (`snapshot`, then `dom 'input'`) showed both inputs were present in the DOM. Possibly the page hadn't fully attached the script context, possibly a tab-targeting issue (the launched session had a localhost storybook tab still selected). I didn't repro a second time — moved on with `dom` directly. Worth noting that the failure mode (silent timeout error) doesn't tell you whether the selector was wrong, the target tab was wrong, or the page wasn't ready. Would help to surface "selector not found yet on tab X" vs "tab not responsive."

## Gaps I Hit

**`network --detail` doesn't surface response headers.** To confirm `Set-Cookie` was absent I had to fall back to `curl -i`. Not a tool failure — ff-rdp's network actor probably exposes the raw response, but the CLI summary doesn't show headers. For cookie / auth debugging this would close a real gap. A `--headers` flag on `network --detail` would have saved one mode-switch.

**No "follow new requests live" feel for a one-shot click.** I `click`ed the submit, slept 4s, then queried `network`. Worked, but felt manual. A `--wait-for-network <url-pattern> --timeout 5` chained off `click` would be the obvious shape — "click X, return when the resulting request resolves." (`network --follow` exists per `--help`, but the use case is "give me the next matching request" not "stream forever.")

## Commands That Worked Without Comment

`doctor`, `tabs`, `navigate`, `snapshot`, `dom`, `type`, `console`, `cookies`. All single-shot, single-call, gave usable JSON I could pipe through `grep` / `jq` without ceremony. JSON-first output kept me in the shell instead of opening DevTools.

## Headline Impact

Real bug surfaced in roughly 6 minutes of interactive use. Compared to the original "open DevTools, manually fill the form, scrub the network tab, dig through console" path that the user would otherwise have walked me through, this is a clear win — most of the cycle was me deciding what to query next, not waiting for tools.

## Verdict

Two-tier improvement queue from this session:

1. **High value, low effort:**
   - Unify `--selector` ergonomics across `wait` / `type` / `click` (current divergence is the rough edge most likely to confuse a new user).
   - Suppress or relabel `method: "GET"` on `source=performance-api` entries.

2. **Useful next step:**
   - `network --detail --headers` (or just include response headers in `--detail` by default).
   - "click and wait for network" composition so end-to-end form-submit debugging doesn't need a manual `sleep`.

ff-rdp earned its keep here. The bug ended up being a bunny.net CDN config issue (Set-Cookie stripped at the edge) — the tool can't fix that, but it pinpointed the layer with surgical clarity. That's the right scope.
