---
title: "Dogfooding Session 41"
type: dogfooding
date: 2026-05-10
status: completed
site: "https://news.ycombinator.com + https://www.comparis.ch/hypotheken"
commands_tested: [launch, tabs, navigate, page-text, dom, snapshot, perf, network, a11y, click, back, scroll, eval, inspect, screenshot, console, cookies, storage, sources, geometry, responsive, styles, doctor, daemon-status, daemon-stop]
tags: [dogfooding, iter-54, iter-55, exit-codes, timeout-tuning, daemon, screenshot]
---

# Dogfooding Session 41

Verified the iter-54 protocol-correctness work and the iter-55 daemon-hardening / agent-docs work against two real sites. Headline: most iter-55 wins ship cleanly, but C3 (EXIT CODES) is documented but not implemented, and `network` drain through the daemon timing out at the 5 s default is real friction on heavy pages.

## What's New Since Last Session

[[iterations/iteration-54-protocol-correctness]] — length cap, `from`-only reply correlation removed (mostly), eval-on-navigation guard, longString unwrap, drop legacy startListeners. [[iterations/iteration-55-daemon-hardening-docs]] — token auth, log 0o600, `tempfile::Builder` for profile, `daemon status`/`stop`, fast-fail port probe, `Transport::split()`, `--help` output schemas, EXIT CODES doc, README link-forward.

## Regression Checks

| Item | Previous Status | Current | Notes |
|---|---|---|---|
| `navigate --wait-text` first-call `noSuchActor` (iter-53) | fixed in iter-53 | **regressed** | First call against HN failed with `noSuchActor (unknownActor)` on `consoleActor3`. Retry succeeded. Reproduced once during this session. |
| Screenshot actor unavailable (iter-53 task 3) | partial (clean error) | partial | Error message is still clean. The promised DOM-based fallback (`canvas.drawWindow` / html2canvas) never kicks in — even on a known-supported Firefox 150. Pure error path. |
| `daemon status` schema (iter-55 C2 review) | docs/code mismatched | **fixed** | `connections` populated from `stream_subscriber_count`, `buffer_sizes` surfaced, no hardcoded `firefox_connected`. |
| `is_firefox_port_open` skips localhost (iter-55 review) | broken | **fixed** | Probe now uses `ToSocketAddrs`. Verified: navigate against killed Firefox completed in **0.01 s** vs prior ~5 s daemon-spawn timeout. |
| Daemon log 0o600 (iter-55 A2 review) | only on creation | **fixed** | Existing log file with `0644` was tightened to `0600` automatically on first daemon start with the new binary. |

## Smoke Test Results

| Command | Status | Notes |
|---|---|---|
| `launch --headless --auto-consent` | ✅ | Output now wrapped in `{results,total,meta}` envelope (iter-55 C1). Profile dir uses 16-byte random suffix (iter-55 A3). |
| `tabs` | ✅ | |
| `navigate URL --wait-text` | ⚠ | First call after fresh launch occasionally `noSuchActor` (see regression). |
| `page-text` | ✅ | Text returned at `.results` directly (string), not `.results.text`. Matches schema in iter-55 docs. |
| `dom SEL --count` / `--text` / `--text-attrs` | ✅ | |
| `snapshot --format text` | ⚠ | Output uses `<html` (no closing bracket) and `(N children not shown)` — readable but unusual. |
| `eval` (positional / `--file` / `--stdin` / `--stringify`) | ✅ | Note: `--stringify` returns the JSON-stringified value, so a string title shows up double-quoted: `"\"Example Domain\""`. Confusing for an agent expecting a single quote level. |
| `eval` against CSP-blocking page | ✅ | Clean `EvalError: call to eval() blocked by CSP` surfaced via grip preview. |
| Mid-eval `location.href = ...` | ✅ | Did not hang or panic; iter-54 task 3 guard appears effective. |
| `inspect <actor>` (daemon mode) | ✅ | Resolved nested object three levels deep. |
| `inspect <actor> --no-daemon` | ✅ | Cleanly errors with `grip actor ... is no longer valid — re-run eval in the same session, or remove --no-daemon`. Excellent error UX. |
| `screenshot` / `screenshot --full-page` / `--base64` | ❌ | All three modes fail with `screenshot actor unavailable on Firefox unknown` on Firefox 150. No fallback. |
| `network` (default 5 s timeout) | ❌ | `internal error: receiving drain response from daemon: operation timed out` on heavy pages. Works with `--timeout 15000`. |
| `network --detail`, `network --no-daemon` | ✅ | |
| `perf vitals` / `perf summary` / `perf audit` | ✅ | Audit output is rich (`navigation`, `dom_stats`, `resource_by_domain`, `slowest_resources`, `third_party_summary`, `vitals`). |
| `a11y` / `a11y summary` / `a11y contrast --fail-only` | ✅ | HN: 165 contrast failures. Comparis: 0. |
| `click SEL` | ✅ | Good error when selector matches nothing or doesn't exist. |
| `back` / `forward` | ✅ | |
| `scroll bottom` / `scroll until` | ✅ | `scroll until` cleanly times out with hint to `--timeout`. |
| `console --level error` | ✅ | |
| `cookies` / `storage local` | ✅ (empty) | |
| `sources` | ⚠ | Stderr noise: `debug: sources thread actor failed ... falling back to JS DOM/Performance API`. Falls back successfully but the debug line leaks alongside JSON. |
| `doctor` | ✅ | Clean. Reports "Firefox version not advertised in the RDP greeting" for FF 150 — consistent with the screenshot-actor failure. |
| `daemon status` | ✅ | `running:true, uptime_seconds:N, connections:0, buffer_sizes:{}` — matches iter-55 docs. |
| `daemon stop` | ✅ | Graceful shutdown via RPC; status afterwards correctly reports `running:false`. |
| `geometry SEL...` | ⚠ | Returns 18 hidden zero-sized matches for `header` (Comparis page). `--visible-only` flag exists but isn't default. |
| `responsive SEL --widths W1,W2` | ⚠ | Returns `{breakpoints:[{width:320, elements:[...], viewport:{...}}, ...], original_viewport:{...}}` — but `--help` documents `{"320": [...], "768": [...]}`. **Schema/doc mismatch**. |
| `styles SEL --properties X,Y,Z` | ✅ | |

## Findings

### What Works Well

- **Daemon UX is now first-class.** `daemon status`, `daemon stop`, the auth handshake on registry — all behaved exactly as documented. Status output (`uptime_seconds: 176`, `connections: 0`) is the kind of operator-friendly data that was missing before.
- **Fast-fail port probe** is the highest-impact iter-55 win in practice: typoed port or dead Firefox now fails in 0.01 s instead of 5 s. Tight feedback loop.
- **Profile dir random suffix** is invisible in normal use, but verified by reading the path: `/var/folders/.../ff-rdp-profile-lsjyV9syZZa84YDu` — 16 bytes of entropy as designed.
- **Inspect via daemon** chained nicely: `eval` → grab `.results.actor` → `inspect` resolved 3 levels deep on the same connection. Workflow that was broken in `--no-daemon` is fluid in daemon mode and the failure mode is well-explained.
- **`perf audit` output** is the richest single-command snapshot — `navigation`, `dom_stats` (2400 nodes, 21 render-blocking, 20 inline scripts), `resource_summary`, `third_party_summary`, `slowest_resources`, full `vitals`. Genuinely actionable for an audit agent.
- **Error messages with hints** continue to be a strength: every failure ends with an actionable suggestion (`use ff-rdp dom SELECTOR --count to verify`, `increase with --wait-timeout`, etc.).

### Issues Found

1. **EXIT CODES doc/code mismatch (iter-55 C3).** Help advertises `0=ok, 1=runtime, 2=usage, 3=connection failure, 124=timeout`. Reality: timeouts return `1` (not 124), connection failures return `1` (not 3). Only `0` (ok) and `2` (usage) match. Either fix the dispatch (route timeouts to 124 and connection failures to 3) or remove the unsupported codes from `--help`. Critical for agents using exit codes for control flow.
   - Tested: wait-timeout → 1, element-not-found → 1, bad-selector → 1, missing arg → 2, connection refused → 1.

2. **`network` default timeout too tight on heavy pages.** On comparis.ch (~100 requests during initial load), `ff-rdp network` with the default 5000 ms timeout fails with `internal error: receiving drain response from daemon: operation timed out`. `--timeout 15000` works. Two problems: (a) 5 s is below the realistic worst-case for daemon drain on a typical SPA, (b) the error is classified as `internal error` rather than user-actionable. Fix: bump default to e.g. 10–15 s for `network` specifically, or surface as `error: network drain timed out — try --timeout 15000`.

3. **`responsive` output schema mismatches docs.** `--help` says `{"results": {"320": [...], "768": [...]}}`. Actual: `{"results": {"breakpoints": [{width:320, elements:[...]}], "original_viewport":{...}}}`. The actual shape is reasonable; just update the help.

4. **Screenshot fallback never fires on FF 150.** The iter-53 promise was: when the screenshot actor errors with the known module-load error, fall back to a DOM-based capture via `eval`. On Firefox 150 the error path produces a clean message but no fallback attempt. Verify that the fallback condition matches the actual error string Firefox returns now, or remove the promise from the iter-53 plan.

5. **`geometry` is noisy.** Querying `geometry "h1" "header" "main"` on Comparis returns 18 zero-sized hidden `header` matches alongside the visible one. `--visible-only` exists but defaults to off. For an agent the default should probably be visible-only with `--include-hidden` to opt in.

6. **`eval --stringify` double-quotes string results.** `ff-rdp eval --stringify "document.title"` returns `"results": "\"Example Domain\""`. The outer `"` is JSON, the inner `\"...\"` is the JSON-stringified string. So a value that's just a string ends up double-encoded. Either drop the `JSON.stringify` wrap when the result is already a string, or document this clearly.

7. **`navigate --wait-text` first-call regression (iter-53).** Reproduced once: `noSuchActor (unknownActor) — No such actor for ID: server1.conn3.child8/consoleActor3`. Retry worked. iter-53's fix (re-resolve console actor after navigation) doesn't appear to be 100% reliable. Worth a fixture-level reproduction.

8. **Stderr leaks debug noise.** `sources` (and possibly other commands with fallbacks) prints `debug: sources thread actor failed ... falling back to JS DOM/Performance API` to stderr alongside the JSON on stdout. Agents capturing 2>&1 see the noise. Either downgrade to trace-level (only with `--verbose` / `RUST_LOG`) or document it.

### Feature Gaps

- A way to **explicitly ask for the daemon to drain everything and shut down** (`daemon flush` or `daemon drain`). Currently you can `daemon stop` (kills it) or wait for idle timeout, but no "give me everything you've got and exit gracefully" workflow.
- `network` lacks a `--since <timestamp>` filter — useful when an agent wants to capture network activity from a specific point onward without `--follow`.
- `geometry --visible-only` should arguably be the default with `--include-hidden` to opt in.
- No way to query the **current viewport size** as a single command. Have to `eval "innerWidth + 'x' + innerHeight"`.

## Summary

- **23 commands tested**, 14 passed cleanly, 5 partial-pass (works but with caveats), 4 had real issues.
- **Top fix to ship**: align EXIT CODES doc with actual exit behavior (iter-55 C3 follow-up).
- **Top UX bug**: bump default `network` timeout (or improve the error message) — every heavy site hits this.
- **Big iter-55 wins confirmed in real use**: token auth, daemon status/stop, fast-fail port probe, log perms, random profile suffix, launch envelope. All shipped clean.

Linked: [[iterations/iteration-54-protocol-correctness]] · [[iterations/iteration-55-daemon-hardening-docs]] · [[dogfooding/dogfooding-session-40]] · [[backlog/future-features]]
