---
title: "Dogfooding Session 29: Real-World Testing on comparis.ch"
date: 2026-04-08
tags:
  - dogfooding
  - testing
  - ux
  - bugs
status: completed
firefox_version: "149.0"
tool_version: ff-rdp 0.1.0
---

# Dogfooding Session 29: ff-rdp on comparis.ch

Systematic exercise of all ff-rdp commands against a live website (comparis.ch) using headless Firefox 149.

## Setup Notes

- Firefox auto-updater interfered with headless launch 3 times. Had to manually set `app.update.enabled=false` in `user.js` and clear the update staging directory.
- Required `devtools.debugger.remote-enabled=true` in profile prefs — Firefox 149 doesn't enable it by default even with `--start-debugger-server`.
- Once running, connection was stable throughout the session.
- **Consent suppression:** Instead of clicking the consent dialog, run `ff-rdp eval 'window.cmp_noscreen = true'` after every navigation to comparis.ch. This suppresses the overlay entirely.

## Command Results Summary

### Commands That Worked Perfectly

| Command | Time | Notes |
|---------|------|-------|
| `navigate <url>` | 17-144 ms | Fast, reliable. |
| `perf vitals` | 41-110 ms | Clean output, ratings included. |
| `perf audit` | 37-96 ms | Comprehensive audit data. Excellent. |
| `dom stats` | 52-90 ms | Concise, useful. |
| `snapshot --depth N --max-chars N` | 52-99 ms | Great for LLM consumption. Truncation messages helpful. |
| `eval <JS>` | 52-67 ms | Works well, returns raw JS values. |
| `click <selector>` | 60 ms | Returns confirmation with tag/text. |
| `type <selector> <text>` | 67 ms | Works, including --clear flag. |
| `wait --selector/--text/--eval` | 107-109 ms | All three modes work. Polling is fast. |
| `perf compare <url1> <url2>` | 455 ms | Navigates to each URL and collects data. Nice. |
| `cookies` | Fast | Full cookie details including sameSite, httpOnly. |
| `storage local` | Fast | Returns all localStorage key-value pairs. |
| `dom <selector> --text` | Fast | Returns text content. |
| `dom <selector> --count` | Fast | Returns count with selector echo. |
| `dom <selector> --attrs` | Fast | Returns attributes as objects, respects --limit. |
| `dom tree [selector] --depth N` | Fast | Native WalkerActor tree, not JS eval. |
| `page-text` | Fast | Full page text extraction. Very useful for LLMs. |
| `geometry <selector>` | Fast | Detailed geometry with viewport info, overlap detection. |
| `styles <selector>` | Fast | Full computed styles. |
| `responsive <selector> --widths` | 329 ms | Resizes and measures across breakpoints. |
| `reload` / `back` / `forward` | 9-51 ms | Simple, reliable. |
| `navigate --with-network` | 17.2 s | Captures full network during navigation (waits for idle). |
| `network --follow` | Streaming | NDJSON streaming works, captured 298 lines during navigation. |
| `a11y contrast` | Fast | JS-based contrast checking works even when a11y actor is broken. |
| `recipes` | Instant | Helpful curated jq examples. |
| `llm-help` | Instant | Complete CLI reference. Excellent for AI agents. |
| `tabs` | Fast | Lists tabs with actor IDs. |
| `perf vitals --jq '.results.fcp_ms'` | 64 ms | jq filtering works perfectly. |
| `dom stats --jq '.results.node_count'` | Fast | jq on single values works. |
| `perf vitals --format text` | 41 ms | Clean key-value output. |
| `dom stats --format text` | 60 ms | Clean key-value output. |

### Commands That Failed or Had Issues

| Command | Error | Severity |
|---------|-------|----------|
| `a11y --depth 3` | `unrecognizedPacketType: getRootNode` | **HIGH** — Firefox 149 broke AccessibilityWalker |
| `screenshot` | `unrecognizedPacketType: captureScreenshot` | **HIGH** — Firefox 149 broke screenshotContentActor |
| `sources` | `undefined passed where a value is required` | **MEDIUM** — Firefox 149 protocol change |
| `inspect <actor>` | `grip actor is no longer valid` with --no-daemon | **LOW** — expected, inspect needs persistent connection |

### UX Friction Points

1. **`click --selector` vs `click <SELECTOR>`** — I instinctively tried `--selector` (like `wait` uses). The positional argument is inconsistent with `wait --selector`. Either both should use positional or both should use flags.

2. **`storage localStorage` vs `storage local`** — Error message says `expected "local" or "session"` but a user would naturally type `localStorage` or `sessionStorage`. Should accept both forms.

3. **`--fields` flag has no visible effect** — `perf vitals --fields "fcp_ms,ttfb_ms"` returned all fields. Either the flag doesn't work for this command or it silently does nothing.

4. **`--format text` on `perf audit` outputs JSON** — The `--format text` flag works for `perf vitals` and `dom stats` but `perf audit` still outputs JSON. Inconsistent behavior.

5. **`snapshot --format text` outputs JSON** — Same issue as perf audit.

6. **`network --limit 20` returns 0 results** — Without `--follow`, there's no data to show. The command succeeds silently. Should either print a hint ("no captured requests, use --follow or navigate --with-network") or document this more clearly.

7. **`navigate --with-network` takes 17 seconds** — The network idle timeout is very conservative. For real-world pages with continuous beacon/tracking activity, it never truly goes idle. Need a shorter idle threshold or a `--network-timeout` flag.

8. **`perf compare` navigates away from current page** — After comparing, the browser is on the last compared URL. This is a side effect that may surprise users. Should document this, or offer a `--restore` flag.

9. **`tbt_ms: -0.0`** — Negative zero is technically correct but looks odd. Should normalize to `0.0`.

10. **`lcp_ms: null` always** — LCP is never populated. Either Firefox headless doesn't fire LCP entries, or the PerformanceObserver approach doesn't work. This is a significant gap for web performance analysis.

11. **Daemon connection drops after `network --follow`** — After running `network --follow` in background and killing it, the daemon port became unavailable. Had to fall back to `--no-daemon`. The daemon should handle abrupt client disconnects more gracefully.

12. **`geometry "header"` returns 43 matches** — The consent manager injects many hidden `<header>` elements. The output is overwhelmed with invisible headers. Should default to showing only visible elements, or have a `--visible-only` flag.

13. **`click` result shows raw actor/preview objects** — The click response includes low-level RDP grip info (`actor`, `class`, `preview.ownProperties`). For most users, a simple `{"clicked": true, "tag": "A", "text": "..."}` would suffice. Consider flattening the response.

14. **Console `--follow` produced no output** — Even after generating console.log/warn/error messages via `eval`, the `console --follow` stream captured nothing. May be a timing issue or a Firefox 149 protocol change.

### Missing Features / Suggestions

1. **`--visible-only` flag for `geometry`** — Filter out invisible/zero-size elements.
2. **`navigate --wait-idle`** — Wait for network idle after navigation (shorter timeout than --with-network).
3. **`screenshot --selector`** — Capture a specific element, not the whole page.
4. **`network` without `--follow`** — Should show resource timing entries from Performance API (which already works via `perf audit`). The command is effectively useless without `--follow`.
5. **`perf audit --recommendations`** — Generate actionable text recommendations based on the audit data.
6. **`dom querySelector` shorthand** — e.g., `ff-rdp qs "h1"` for quick queries.
7. **Storage type aliases** — Accept `localStorage`/`sessionStorage` as well as `local`/`session`.
8. **Profile management** — `ff-rdp launch --temp-profile` is great, but `ff-rdp launch` should also handle the `user.js` prefs automatically (debugger remote enabled, update disabled).

### Performance Assessment

- **All commands are fast** — Most complete in under 100 ms. This is excellent.
- **`navigate --with-network`** is the outlier at 17 seconds due to idle detection.
- **`--no-daemon` is comparable to daemon mode** — Overhead of daemon is negligible.
- **`responsive`** with 3 breakpoints completes in 329 ms — good for automated testing.

### Firefox 149 Compatibility

Three RDP actor types are broken in Firefox 149:
1. `accessiblewalker` — `getRootNode` unrecognized
2. `screenshotContentActor` — `captureScreenshot` unrecognized
3. `sources` — returns undefined error

These are likely protocol changes in Firefox 149 that need investigation. The `a11y contrast` command works because it uses JS evaluation as a fallback rather than the accessibility actor.

**Recommendation:** Add Firefox version detection and warn users when running against untested Firefox versions. Consider a compatibility matrix in the docs.
