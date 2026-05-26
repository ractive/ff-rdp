---
title: "Dogfooding Session 56 — verify iter-83 + chaos training sites"
type: dogfooding
date: 2026-05-26
status: completed
site: tennis-sepp.ch, example.com, news.ycombinator.com, httpbin.org, w3.org/WAI/demos/bad, dequeuniversity.com/demo/mars, the-internet.herokuapp.com
commands_tested: [navigate, cascade, screenshot, cookies, styles, perf vitals, perf audit, perf summary, a11y, a11y contrast, dom stats, snapshot, network, computed, click, wait]
tags: [dogfooding, iter-83, regression-verification, chaos]
---

# Dogfooding Session 56

One-line summary: Iter-83 fixed 3 of 7 themes (D, F, G); A/B/C/E remain broken or partial — and chaos exploration surfaced 4 new issues including dom-stats/perf-audit count mismatch, stale-tab snapshot, inconsistent `wait` flag naming, and a confusing 0ms timeout after `data:` URL block.

Linked: [[dogfooding-session-55]], [[iteration-83-dogfood-55-real-fixes]]

## What's New Since Last Session (iter-83 themes A–G)

- **A — cascade**: Promised to return rules with matched_selectors; **still returns `rules: []`** — raw data IS present via `--debug-raw`, so the parsing/filter layer is dropping it.
- **B — screenshot**: Promised PNG output; **still errors out** with "screenshot actor not found in Firefox 151 root form."
- **C — navigate default wait**: Now waits (no longer instant) but **times out at 10s** even on `example.com`. Partial regression: the default is too strict / event subscription appears not to fire.
- **D — cookies default**: ✅ `--include-document-cookie` flag is gone; cookies now include document.cookie by default (flag inverted to `--storage-only`).
- **E — styles dedupe**: ❌ Still shows duplicate rule entries (e.g. two `::after, ::before`, two `h1` entries with `column: 28837` vs `5245`).
- **F — perf vitals**: ✅ `lcp_rating: "unavailable"` is emitted when LCP missing.
- **G — cookie help**: ✅ `cookies --help` is clean.

## Part A — Iter-83 regression table

| Theme | Command | Verdict | Evidence | Notes |
|-------|---------|---------|----------|-------|
| A cascade | `ff-rdp cascade 'h1' --prop color` on tennis-sepp.ch | ❌ broken | `"rules": []` | `--debug-raw` shows real `matchedSelectorIndexes: [0]` + `authoredText` payload from Firefox; the cascade aggregator is filtering them out. Iter-84 fix should be in `crates/ff-rdp-core/src/cascade.rs` (or wherever `cascade` parses `getApplied`). |
| B screenshot | `ff-rdp screenshot -o /tmp/iter-83.png` | ❌ broken | `error: screenshot: screenshot actor not found in Firefox 151 root form.` | File not created. Either screenshot actor moved in FF 151 or root-form probe is wrong path. |
| C navigate default | `ff-rdp navigate https://example.com` | ⚠️ partial | `error: navigate: timed out after 10032ms waiting for document events and no remaining budget for readystate fallback` | Default DOES wait (no longer instant) but the document-events listener never fires on a trivial page — and the readystate fallback budget is exhausted. |
| D cookies default | `ff-rdp cookies` after navigate | ✅ works | `--include-document-cookie` flag removed; behavior inverted to `--storage-only` | Help text clean. |
| E styles dedupe | `ff-rdp styles 'h1' --applied` on tennis-sepp.ch | ❌ broken | Two `::after, ::before` entries (col 26798 twice), two `h1` entries (col 28837 + 5245) | Dedupe isn't keying on rule actor or doesn't dedupe at all. |
| F perf vitals | `ff-rdp perf vitals` on example.com | ✅ works | `"lcp_rating": "unavailable"` present alongside `lcp_ms: null` | Good. |
| G cookies help | `ff-rdp cookies --help` | ✅ works | Help text reads cleanly, no formatting leaks | n/a |

**Verdict: 3/7 iter-83 themes actually fixed (D, F, G). A, B, E flat-out broken; C partial.**

## Part B — Chaos exploration

### w3.org/WAI/demos/bad/before/home.html (intentional a11y violations)

- `ff-rdp a11y contrast --fail-only` → `total: 0`, `aa_fail: 0`. **The WAI "Before" demo is famously full of contrast issues — our contrast scanner found nothing.** Either contrast extraction isn't pairing fg/bg correctly, or it's only sampling computed-style on a tiny subset of nodes.
- `ff-rdp a11y` tree returned a useful structure with `fallback: true, fallback_method: "js-eval"` — at least the fallback path works.
- **Mismatch**: `ff-rdp dom stats` → `images_without_lazy: 9`, while `ff-rdp perf audit`'s embedded `dom_stats` says `images_without_lazy: 42`. Same page, same load — two answers. (Probably perf audit counts `<img>` from the Resource Timing list including subdoc/CSS image-set, while dom-stats counts only `document.images`.) Either way, surprising for a user.

### dequeuniversity.com/demo/mars/

- `perf vitals` → `fcp_ms: 3295 (poor)`, `lcp_ms: 5151 (poor)` — vitals classification works on a heavy SPA.
- `a11y contrast --fail-only` → `aa_pass: 12, aa_fail: 0`. Mars Commuter has known a11y bugs; finding zero is suspicious (small sample size, capping?).
- `computed h1 --prop color` returned `rgb(255, 255, 255)`, but `cascade h1 --prop color` returned `rules: []` — confirms cascade is broken across sites, not just tennis-sepp.

### the-internet.herokuapp.com/dynamic_loading/1

- `ff-rdp wait` flag inconsistency: command does **not** accept positional selector (must use `--selector`) and uses `--timeout` not `--timeout-ms`, while other commands like `navigate` use `--timeout-ms`. Friction.
- First `snapshot` call after navigate returned the **previous page** (mars page) DOM — race condition. Second call returned the right page.
- `click '#start button'` first attempt failed with `noSuchActor` on `consoleActor3` — the consoleActor cache survived past a tab navigation. After re-running it worked.
- `wait --selector '#finish h4' --timeout 8000` → matched in 1ms after click. ✅
- `dom stats` immediately after navigate also returned stale counts (the WAI page's 9 / 331 nodes) — same race window.

### httpbin.org

- `navigate 'https://httpbin.org/cookies/set/session/abc123'` followed by `cookies` → `results: []`. Whether cookies were actually set is unclear; this might be a httpbin redirect quirk or our cookies command not picking up Set-Cookie redirects.
- The unquoted `https://httpbin.org/cookies/set?a=1` is interpreted by zsh as a glob — not our problem, but a docs hint to always quote URLs with `?` would help.
- `data:text/html,<h1>` → fails with `URL scheme 'data:' is not allowed by default; pass --allow-unsafe-urls to opt in` — good default. But adding `--allow-unsafe-urls` then errored with `operation timed out after 0ms (phase: recv)`. "0ms timeout" is meaningless to users.

### example.com (baseline)

- After the `data:` URL failure the daemon connection appears wedged: subsequent `network --format text` reported all requests as `0ms, 0, 0b`. Either timing wasn't recorded because the connection state was off, or the network actor stripped timing fields. Suspect collateral damage from the earlier failed-data-url state.

## Findings

### What Works Well

- `perf vitals` / `perf audit` / `perf summary` are stable across simple and heavy pages.
- `a11y` tree (js-eval fallback) returns useful semantic data on all sites tested.
- `computed <sel> --prop` works reliably (returns correct color values).
- `cookies` no longer needs an opt-in flag for document.cookie — UX win.
- `wait --selector` works fast (1ms match) and is good for interactive flows.
- Defensive guardrails (data: URL block, hint messages, error_type tagging) are user-friendly.

### Issues Found

1. **Cascade returns `rules: []` despite raw data being present.** `--debug-raw` shows Firefox returns `matchedSelectorIndexes: [0]` and full `authoredText`/`declarations`. The post-processing layer drops them. (iter-83 Theme A — claim unfulfilled.) High priority.
2. **Screenshot still errors out** ("screenshot actor not found in Firefox 151 root form"). (iter-83 Theme B — claim unfulfilled.) Either Firefox 151 moved the actor or our root-form probe regex is wrong.
3. **Styles `--applied` still has duplicate rule entries.** Same rule actor surfaces multiple times with empty `properties`. (iter-83 Theme E — claim unfulfilled.)
4. **Default `navigate` wait-strategy times out at 10s** even on example.com. Document-events listener path isn't firing; readystate fallback budget is exhausted before fallback runs. (iter-83 Theme C — partial.)
5. **`dom stats` and `perf audit`'s embedded `dom_stats` disagree** (9 vs 42 images_without_lazy on the same page in the same load). One of them is counting from Resource Timing, the other from `document.images` — pick one definition.
6. **Stale-tab race**: first `snapshot` / `dom stats` call immediately after `navigate` returns the *previous* page. Subsequent calls are correct. Suggests internal page/document handle caches aren't invalidated synchronously on navigate.
7. **`consoleActor` cache survives navigation**: first `click` after navigate fails with `noSuchActor`. Should be transparently retried.
8. **`wait` flag inconsistency**: uses `--timeout` (no positional selector) while `navigate`/others use `--timeout-ms`. Pick one convention.
9. **"operation timed out after 0ms (phase: recv)"** error message after `data: + --allow-unsafe-urls` is meaningless. Either fix the underlying behavior or improve the message.
10. **a11y contrast scanner reports `total: 0` on a page intentionally designed to fail contrast** (WAI Before demo). Either the sampling is too narrow or the pairing logic isn't catching cases.
11. **Network timing all zeros** after a daemon-state hiccup. Suspect a stale connection isn't reset; users get useless timing data without warning.

### Feature Gaps

- No way to record a screenshot at all on Firefox 151 (regression-class blocker for any visual auditing).
- No "force-refresh tab handles" command after navigate; users have to call twice and hope.
- `cookies` doesn't seem to pick up cookies set during 302-style flows on httpbin — needs investigation.
- No consistent flag taxonomy (`--timeout` vs `--timeout-ms`, `--selector` vs positional) — a `cargo xtask check-flag-naming` lint would help.

## Summary

**17 commands tested across 7 sites, 11 issues found (3 are iter-83 regressions still live).**

Key takeaway: **iter-83 only delivered 3 of its 7 promised fixes (D, F, G). A, B, E are still broken; C is partial.** This is a repeat of the iter-82 pattern dogfood-55 caught — claims outpacing reality. A follow-up iteration (iter-84) should focus on **(a) cascade aggregator parsing the raw `getApplied` reply that we already have in hand, (b) screenshot actor probe for Firefox 151, (c) styles dedupe by rule actor, (d) navigate default fallback budget**, then re-run dogfood-56 commands as part of its AC. Beyond iter-83, the **dom-stats/perf-audit count mismatch, stale-tab race, and `consoleActor` cache-after-navigate** are independent quality issues worth a separate cleanup pass.
