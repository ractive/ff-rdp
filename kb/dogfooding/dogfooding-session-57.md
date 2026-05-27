---
title: "Dogfooding Session 57 — verify iter-84 real-real fixes"
type: dogfooding
date: 2026-05-27
status: completed
site: tennis-sepp.ch, example.com, news.ycombinator.com, dequeuniversity.com/demo/mars, w3.org/WAI/demos/bad, the-internet.herokuapp.com, httpbin.org
commands_tested: [launch, navigate, tabs, snapshot, page-text, cascade, screenshot, styles, dom stats, perf audit, a11y contrast, click, wait, cookies]
tags: [dogfooding, iter-84, regression-verification]
---

# Dogfooding Session 57

One-line summary: Iter-84 actually landed 5 of 9 themes (E, H, I, J, K-canonical) on real
sites; the four headline regressions iter-83 carried in (A cascade, B screenshot, C
navigate <3s, L cookies Set-Cookie merge) are **still broken** — and the iter-84 plan's
own task list openly admits B/I-client-cache/K-warning/L-merge were not landed. This is
the 4th attempt at A/B/C; we need iter-85.

Linked: [[dogfooding-session-56]], [[iteration-84-dogfood-56-real-real-fixes]],
[[iteration-83-dogfood-55-real-fixes]].

## Setup

- Binary: `ff-rdp 0.2.0 (84ef8a10f575+dirty 2026-05-27)` (rebuilt from `main` at f30f695
  via `cargo install --path crates/ff-rdp-cli --offline`).
- Firefox: launched via `ff-rdp launch --headless --auto-consent`.
- Method: ran each command from the iter-84 `dogfood_path` against the same sites
  dogfood-56 used, so the comparison is apples-to-apples.

## Iter-84 theme verification table

| Theme | Promise | Command | Verdict | Evidence |
|-------|---------|---------|---------|----------|
| **A — cascade** | `rules: []` no more on tennis-sepp; matched_selectors populated | `ff-rdp cascade 'h1' --prop color --jq '.results[0].rules \| length'` | ❌ **STILL BROKEN** | Returns `0` on tennis-sepp.ch AND dequeuniversity.com/demo/mars. `--debug-raw` shows entries with `rule.type: 100` and `className: "CSSStyleRule"`; the iter-84 fix in `parse_applied_entry` accepts `type` absent or `== 1`, but the real wire shape is `type: 100`. Wrong sentinel — fix doesn't match Firefox 151 reality. |
| **B — screenshot** | PNG written on FF 151 | `ff-rdp screenshot -o /tmp/iter-84.png` | ❌ **STILL BROKEN** | `error: screenshot: screenshot actor not found in Firefox 151 root form.` Both `-o` and `--full-page` fail identically. Plan task list openly admits "Route the request… **not landed**" — only the diagnostic helper landed. |
| **C — navigate default** | Completes <3s on example.com | `time ff-rdp navigate https://example.com` | ⚠️ **PARTIAL** | Now completes (exit 0) in ~7.2s on example.com, ~7.1s on news.ycombinator.com, ~7.1s on tennis-sepp.ch. No more 10s "no remaining budget" error → big win over dogfood-56. But the AC promised <3000ms — that target is missed by 2.4×. `--debug-events` flag does not exist on `navigate`. |
| **E — styles dedupe** | No duplicate rule_actor_id | `ff-rdp styles 'h1' --applied --jq '[.results[].rule_actor_id] \| unique \| length'` | ✅ **FIXED** | On tennis-sepp.ch: `n=9 u=9 equal=true`. The duplicate `::after,::before` / `h1` rows from dogfood-56 are gone (selectors visible now: distinct domstylerule29/31/32/33/34/36/37/38). `body --applied` returns 5 rules. |
| **H — dom-stats / perf-audit parity** | Same `images_without_lazy` | `dom stats` vs `perf audit` on WAI bad demo | ✅ **FIXED** | Both report `images_without_lazy=9`. (Dogfood-56 had 9 vs 42.) |
| **I — stale-tab race** | First call after navigate hits new tab | `navigate HN → navigate example.com → tabs / page-text` | ✅ **FIXED** | `tabs --jq '.results[0].url'` → `"https://example.com/"`; `page-text` → "Example Domain…". No stale HN content on first call. Note: `snapshot` has no top-level `.results.url` field at all — the plan's jq path `.results.url` was wrong, but the race itself is gone. Client-side tab-handle invalidation in `client.rs` was NOT landed (the plan admits this); the fix is the console-actor refresh hack. |
| **J — a11y contrast on WAI bad** | `aa_fail >= 1` | `a11y contrast --fail-only` on WAI bad demo | ✅ **FIXED** | `.meta.summary` → `{"aa_fail":11,"aa_pass":108,"total":119}`. (Dogfood-56 saw `total: 0, aa_fail: 0`.) NOTE: aggregate counts live at `.meta.summary.aa_fail`, NOT `.results.aa_fail` as the iter-84 plan's `dogfood_path` claimed. |
| **K — wait flag unification** | `--timeout-ms` canonical, `--wait-timeout` deprecated alias warns | `wait --selector '#finish h4' --timeout-ms 8000` and `--wait-timeout 8000` | ⚠️ **PARTIAL** | Both flags work (exit 0, matched in 1ms on herokuapp dynamic_loading). But the deprecation warning on `--wait-timeout` is silent — plan admits it was descoped. Functional renaming achieved; UX nudge not. |
| **L — cookies Set-Cookie merge** | `session` cookie appears after navigating `httpbin.org/cookies/set?session=abc123` | `ff-rdp cookies --jq '[.results[].name]'` | ❌ **STILL BROKEN** | Returns `[]`. Plan openly admits the network-actor merge "not landed" — only a 250ms retry was added, which is insufficient because httpbin's redirect happens before any StorageActor flush. |

## Score: 4 fixed, 2 partial, 3 still broken (out of 9)

- ✅ Fixed: E, H, I, J
- ⚠️ Partial: C (now completes but slower than promised), K (canonical name works, deprecation warning missing)
- ❌ Still broken: A (cascade), B (screenshot), L (cookies merge)

## Detailed evidence

### Theme A — cascade (still broken)

```text
$ ff-rdp navigate https://tennis-sepp.ch                  # OK
$ ff-rdp cascade 'h1' --prop color --jq '.results[0].rules | length'
0
$ ff-rdp cascade 'h1' --prop color --debug-raw | head -50
[cascade --debug-raw] raw getApplied reply:
{
  "entries": [
    { ... "rule": { "actor": "…/domstylerule29", "type": 100,
                    "className": "CSSStyleRule",
                    "authoredText": "\n  margin: 0 0 0.15em;\n  …\n  color: var(--pico-primary);\n  …",
                    "declarations": [ … ] },
      "matchedSelectorIndexes": [0] }, …
  ]
}
$ ff-rdp navigate https://dequeuniversity.com/demo/mars/
$ ff-rdp cascade 'h1' --prop color --jq '.results[0].rules | length'
0
```

Root cause: iter-84 changed `parse_applied_entry` to accept `rule.type` absent OR
`== 1`. But Firefox 151's wire shape is `rule.type: 100` (with
`className: "CSSStyleRule"`). The new filter still drops every real entry. The
fix is one literal away from working — either accept `type == 100`, or filter
on `className == "CSSStyleRule"`, or stop filtering on `rule.type` and instead
filter on `matchedSelectorIndexes.length > 0`. Confirmed broken on TWO sites
(tennis-sepp.ch + dequeuniversity.com/demo/mars), so this is not a one-off.

### Theme B — screenshot (still broken)

```text
$ ff-rdp navigate https://example.com
$ ff-rdp screenshot -o /tmp/iter-84.png
error: screenshot: screenshot actor not found in Firefox 151 root form. Run `ff-rdp doctor` for the full compatibility report (minimum supported: 120).
$ ff-rdp screenshot --full-page -o /tmp/iter-84-full.png
error: screenshot: screenshot actor not found in Firefox 151 root form. …
```

Neither file is created. The plan's task 2/3 ("Route the request to
WindowGlobalTarget / fallback method") is explicitly marked "not landed";
only the diagnostic helper went in. Screenshot remains a 100% regression
for FF 151 users.

### Theme C — navigate default (partial)

```text
$ time ff-rdp navigate https://example.com           # 7.206s, exit 0
$ time ff-rdp navigate https://news.ycombinator.com  # 7.161s, exit 0
$ time ff-rdp navigate https://tennis-sepp.ch        # 7.147s, exit 0
```

No more "no remaining budget" error → big improvement vs dogfood-56's 10s
timeout. But the AC says <3000ms on example.com; we're at 7.2s. The
readystate-fallback split helped, but the events strategy still appears to
wait for its full ~70% slice before falling through. Also note: `--debug-events`
does not exist on navigate (the dogfood_path command would have failed if anyone
actually ran it).

### Theme E — styles dedupe (FIXED)

```text
$ ff-rdp styles 'h1' --applied --jq '[.results[].rule_actor_id] | length as $n | unique | length as $u | "n=\($n) u=\($u) equal=\($n==$u)"'
"n=9 u=9 equal=true"
```

Distinct selectors (`.site-header hgroup h1`, `::after,::before`, `::selection` ×2 from
different stylesheets, `h1` ×2 from different stylesheets, `h1,h2,h3,h4,h5,h6` ×2,
`hgroup > *`) — all 9 have distinct `rule_actor_id`. Real dedupe.

### Theme H — dom-stats/perf-audit parity (FIXED)

```text
$ ff-rdp dom stats --jq '.results.images_without_lazy'        → 9
$ ff-rdp perf audit --jq '.results.dom_stats.images_without_lazy'  → 9
```

### Theme I — stale-tab race (FIXED)

```text
$ ff-rdp navigate https://news.ycombinator.com
$ ff-rdp navigate https://example.com
$ ff-rdp tabs --jq '.results[0].url'         → "https://example.com/"
$ ff-rdp page-text | head -3                 → "Example Domain ..."
```

### Theme J — a11y contrast (FIXED, but plan's jq path wrong)

```text
$ ff-rdp navigate https://www.w3.org/WAI/demos/bad/before/home.html
$ ff-rdp a11y contrast --fail-only --jq '.meta.summary'
{"aa_fail":11,"aa_pass":108,"capped":false,"total":119}
```

Scanner is finding real failures now. Plan's `.results.aa_fail` jq doesn't exist —
aggregate counts live at `.meta.summary.aa_fail` (the `results` array contains
the per-element fail entries).

### Theme K — wait flag (partial)

```text
$ ff-rdp wait --selector '#finish h4' --timeout-ms 8000   → matched, elapsed_ms: 0
$ ff-rdp wait --selector '#finish h4' --wait-timeout 8000 → matched, elapsed_ms: 0 (no deprecation warning)
```

### Theme L — cookies Set-Cookie (still broken)

```text
$ ff-rdp navigate 'https://httpbin.org/cookies/set?session=abc123'
$ ff-rdp cookies --jq '[.results[].name]'
[]
```

Plan admits the network-actor merge wasn't landed; the 250ms StorageActor retry
isn't sufficient when httpbin issues a redirect immediately.

## New / surfaced bugs

1. **iter-84's own `dogfood_path` jq paths are wrong** in three places (`.results.rules`
   should be `.results[0].rules`; `.results.aa_fail` should be `.meta.summary.aa_fail`;
   `snapshot --jq '.results.url'` has no such field). This is direct evidence that
   nobody ran the dogfood_path before ticking the ACs — a repeat of the iter-82/83
   verification-theater pattern the iteration was specifically meant to prevent.
2. **`navigate --debug-events` is undocumented / missing**: the iter-84 dogfood_path
   uses it (`ff-rdp navigate ... --debug-events`), but the binary rejects it as
   "unexpected argument".
3. **Cascade fix one-character off**: `parse_applied_entry` accepts `type == 1`, but
   FF 151 returns `type: 100` for `CSSStyleRule`. Easy fix, but worth noting that the
   author chose `1` without checking what the wire actually sends — the `--debug-raw`
   evidence was right there.

## What works well (regression-positive)

- The previously-broken duplicate-rule output in `styles --applied` is genuinely fixed.
- a11y contrast scanner now finds real failures on the canonical WAI bad-contrast demo.
- dom-stats / perf-audit numerical agreement is restored.
- Stale-tab race is gone — back-to-back navigates now resolve to the right page on the
  first follow-up call.
- `wait --timeout-ms` canonical name works without a special flag.
- `navigate` no longer 10s-times-out → page loads complete reliably (just slower than
  the AC promised).

## Summary

- 9 themes tested, **4 fixed (E, H, I, J), 2 partial (C, K), 3 still broken (A, B, L)**.
- This is the **third consecutive iteration** to ship fake ACs for cascade (A) and
  screenshot (B), and the **second** for cookies Set-Cookie (L). The iter-84 plan's
  hard rule ("AC evidence must be the user-visible command output, not actor reply or
  proxy signal") was itself violated by ticking A's AC despite live `cascade` output
  being `rules: []`.
- Cascade fix is **one line away** from working — wrong sentinel value for `rule.type`.
- An iter-85 is clearly needed for: cascade rule.type filter, screenshot WindowGlobalTarget
  routing, cookies Set-Cookie merge via network actor, and ideally tighter navigate
  budget to actually reach <3s on example.com.
