---
title: Dogfooding Session 44 — daemon console stream regression + Radix click gap
type: dogfooding
date: 2026-05-15
status: completed
site: https://admin.wardrobe-assistants.ch
commands_tested: [launch, doctor, tabs, navigate, page-text, dom, wait, type, click, console, network, daemon, eval, perf, a11y, cookies, storage, snapshot, screenshot, back]
tags: [dogfooding, daemon, console-stream-bug, radix-click, csp, screenshot-version-detection]
---

# Dogfooding Session 44

Login flow on the Wardrobe Assistants admin app, focused on the user's open
question: *does daemon mode properly forward console logs and network events?*
**Finding: network streams fine via daemon, console --follow is completely silent.**

Previous: [[dogfooding-session-43]] · [[dogfooding-session-42]] (same site)

## TL;DR

- ✅ Login worked end-to-end (`navigate` → `type email` → `type password` → `click submit`).
- ✅ `network --follow` via daemon captured the `POST /api/auth/sign-in/email` and 90+ subsequent requests cleanly.
- ❌ **`console --follow` via daemon emits zero bytes** even when console events are actively being produced (verified by polling `console` without `--follow`, which returns them fine). Same command with `--no-daemon` works (385 B captured). **Daemon-side console bridge is broken.**
- ❌ Could not log out via the UI — `click` on the Radix dropdown trigger (`aria-haspopup="menu"`) does not open the menu (state stays `closed`). No logout link exposed elsewhere.
- ⚠ Misleading error: `wait` timeout returns `"daemon auth rejected or connection closed (wrong token?): operation timed out"`. There's no auth problem — it's just a plain timeout.
- ⚠ `ff-rdp launch` (non-headless) fails after 5 s timeout even though Firefox does come up ~8 s later. Headless mode is unaffected.
- ⚠ `screenshot` errors with `"Firefox unknown; minimum supported version: 120"` because the RDP greeting on this Firefox build doesn't advertise a version (the same warning already flagged in `doctor`). Hard refusal is too aggressive.

## What's New Since Last Session

Recent: iter-57 fixed dogfood-42 issues, iter-58 added the `ff-rdp-debug` skill. No new ff-rdp commands landed.

## Login flow (worked)

```text
navigate → wait input → type email → type password → click submit
```

Cookie verified via `cookies`: single `__Secure-better-auth.session_token` httpOnly cookie. No `localStorage` data — pure cookie auth.

## Issues Found

### 1. `console --follow` is silent through the daemon (regression)

Setup: daemon auto-started by previous commands. Run:

```sh
ff-rdp console --follow > /tmp/c.log 2>&1 &
# trigger events: navigate, click around, run an eval that fails CSP (logs an error)
```

Result: `/tmp/c.log` is **0 bytes**, process alive. `ff-rdp console` (no `--follow`) returns the messages immediately, so they exist in the daemon buffer. Same command with `--no-daemon`:

```sh
ff-rdp --no-daemon console --follow > /tmp/c-nd.log 2>&1 &
# trigger an eval CSP error
# -> 385 bytes of valid JSONL captured
```

Network streaming on the same daemon worked simultaneously (98 events captured). This narrows the regression to the console-event path inside the daemon's pub/sub fan-out.

Likely culprit: the daemon's `console-message` (or whatever the canonical resource type is) is buffered for `console` but not flushed to `--follow` subscribers. Compare with the `network-event` resource — `daemon status` showed `buffer_sizes: { "network-event": 485 }` but **no console buffer entry**, suggesting the daemon may not even be subscribing to console resources at all until a non-follow request asks.

### 2. Misleading "daemon auth rejected" error on plain timeout

Both `wait --selector` (when the selector never matches) and `wait` after a failed navigation produced:

```
error: daemon auth rejected or connection closed (wrong token?): operation timed out
hint: stop the running daemon or use --no-daemon.
```

There is no auth issue. The token is fine — subsequent commands work. The error is the daemon's RPC client mis-classifying a stream-level timeout as an auth/connection error. The hint actively misleads users to mess with `--no-daemon` when their real problem is just an unmet wait condition.

### 3. `ff-rdp launch` (non-headless) gives up too early

```
$ ff-rdp launch --port 6000
error: Firefox started (pid 88288) but debug port 6000 is not reachable after 5s — is the port already in use?
```

Firefox actually bound port 6000 about **8 seconds** after spawn (`lsof -iTCP:6000 -sTCP:LISTEN` showed a fresh `firefox 88422` listening). The cold-start with visible UI on macOS is just slower than 5 s. Headless mode hits the port in <2 s so it's never seen.

Suggested fix: bump the visible-mode timeout to ~15 s, or poll for up to N seconds with backoff before declaring failure. The current 5 s number makes the very first `launch` for a non-headless workflow look broken.

### 4. `screenshot` refuses when Firefox version is "unknown"

```
error: screenshot: screenshot actor unavailable on Firefox unknown; minimum supported version: 120.
hint: upgrade Firefox or run `ff-rdp doctor` for the full compatibility report.
```

`doctor` already warns: `"Firefox version not advertised in the RDP greeting"`. The current Firefox is post-120 in reality. Refusing the call entirely on "unknown" is too aggressive — better to attempt the screenshot actor and surface its actual error if it doesn't exist.

### 5. `click` doesn't open Radix dropdown menus

`click 'button[aria-haspopup="menu"]'` on a Radix `DropdownMenu.Trigger` reports `clicked: true` but the button's `data-state` stays `closed` and no `[role="menuitem"]` ever appears. Probably because ff-rdp dispatches a synthetic `click` only — Radix opens on `pointerdown`. This bit hard here: there's no other logout route in this app, so I literally couldn't sign out via the UI.

Workaround: would need `eval` (blocked by CSP on this site) or a real pointer-event sequence. Worth considering whether `click` should dispatch a `pointerdown`+`pointerup`+`click` triple when an element has `aria-haspopup` / `data-state="closed"`, or expose `pointer` as a separate verb.

### 6. `cookies` --help schema doesn't match actual output

`cookies --help` documents the result as `{"name", "value", "domain", "path", ...}`. Actual output uses **`host`** (not `domain`):

```json
{"host":"admin.wardrobe-assistants.ch","hostOnly":true,"isHttpOnly":true,"isSecure":true,"name":"__Secure-better-auth.session_token",...}
```

Also: `isHttpOnly`/`isSecure` are camelCase prefixed with `is` — inconsistent with documented `httpOnly`/`secure`. Pick one convention and update both the schema in `--help` and the JSON output.

### 7. `--format text` and `--jq` are mutually exclusive

```
error: --format text and --jq are mutually exclusive
```

Reasonable as a one-line rule, but I reached for `dom 'nav a' --jq '...' --format text` twice during this session expecting `--format text` to operate on the *post-jq* result. The current behavior forces a choice between "tabular but unfiltered" or "filtered JSON" — when what an LLM agent often wants is "filter, then make it terse." Consider rendering jq output as text when both are passed (`-r`-like behavior).

### 8. Eval errors print to stdout as JSON object, not stderr / non-zero exit

`eval 'location.href'` (CSP-blocked on this page) returns a JSON object with `"name": "EvalError"` on **stdout** and exit code 0. This makes it easy to miss in scripted flows. Either non-zero exit + stderr, or at least an envelope flag like `"results": {"error": ...}` that callers can branch on.

## What Works Well

- `network --follow` is genuinely useful — clean JSONL, easy to grep for the auth POST.
- `dom <sel> --jq '<jq>'` is a great combo for "what's on this page" exploration once you accept it returns HTML strings (not nodes).
- `wait --selector` returns immediately when the selector is already matched (`elapsed_ms: 1`), no fixed minimum poll delay.
- `perf vitals` came back instantly with sensible FCP/TTFB numbers. The flag-up that LCP is DOM-approximated is good — but see Issue #9 below.

### 9. (Minor) LCP "good" rating on 0 ms approximate

`perf vitals` returned `lcp_ms: 0.0, lcp_approximate: true, lcp_rating: "good"`. When the value is approximate/unknown, the rating shouldn't be "good" — it should be `"unknown"` or omitted. A user glancing at the JSON will believe LCP is fine when in fact it wasn't measurable.

## Performance Snapshot for admin.wardrobe-assistants.ch

| Metric | Value | Notes |
|---|---|---|
| FCP | 343 ms | good |
| TTFB | 131 ms | good |
| CLS | 0.0 | good |
| LCP | n/a | approximate, see #9 |
| Resources | 78 / 561 KB | of which **43 JS / 498 KB** |
| Render-blocking | 27 | high — most of those are JS chunks |
| DOM nodes | 377 | small |

The Next.js code-split chunks are tiny (often <2 KB each) but there are ~43 of them — likely worth a chunk-merging or `loadable`-style boundary review on the app side. Render-blocking count of 27 is the most actionable item.

## Suggested ff-rdp follow-ups

Ranked by user impact during this session:

1. **Fix daemon `console --follow`** — silent streams are worse than no streams (Issue #1).
2. **Reclassify the daemon timeout error** — the "daemon auth rejected" string is actively wrong and pushes users toward `--no-daemon` for no reason (Issue #2).
3. **Bump non-headless `launch` timeout** to ~15 s or poll with backoff (Issue #3).
4. **Treat eval errors as errors** (non-zero exit + stderr) so agents notice CSP / syntax failures (Issue #8).
5. **Pointer-event option on `click`** for Radix-style triggers (Issue #5) — would have unblocked the logout flow here.
6. Soften screenshot's "unknown version" hard-refusal to a try-and-degrade (Issue #4).
7. Sync the `cookies` --help schema with the actual JSON shape (Issue #6).
8. Allow `--jq` + `--format text` together (Issue #7).
9. Use `"unknown"` instead of `"good"` for approximate LCP at 0 ms (Issue #9).

## Verdict

ff-rdp drove the entire login flow without drama and the network streaming through the daemon is the kind of feature that makes this tool genuinely useful for real apps. But the **silent console stream** is a real regression — and the **misleading daemon-auth error** plus the **Radix click gap** turned what should have been a quick "log in, click around, log out" into a session where the logout half wasn't reachable at all. None of the issues are showstoppers; all of them are the kind a real user/agent would hit immediately on a Next.js + Radix + better-auth stack, which is increasingly common.
