---
title: Dogfooding Session 62 — batch 125–127 verification + Guardian exploratory
type: dogfooding
date: 2026-07-19
status: completed
site: www.theguardian.com, www.comparis.ch, github.com, news.ycombinator.com, example.com, en.wikipedia.org, httpbin.org
commands_tested: [launch, navigate, tabs, page-text, snapshot, screenshot, dom, geometry, computed, responsive, perf, network, a11y, eval, cookies, storage, click, scroll, wait, reload, back, forward, sources, daemon, emulate, throttle, manifest]
tags: [dogfooding]
---

# Dogfooding Session 62

Two-agent session (regression re-tests on ports 6200 + exploratory on 6300, fresh main
build post-iter-127): **all five batch-fix targets from iters 121–127 verified fixed in
the wild**; the Guardian exploratory pass surfaced one major new blocker
(`--auto-consent` fails on Sourcepoint CMPs → site-wide scroll lock) plus seven smaller
findings. Same-day full live sweep context: [[iteration-128-network-hint-always-present]]
was filed for the one deterministic test escape, and this session confirmed the escape is
exactly the `hint` key, nothing more.

## What's New Since Last Session ([[dogfooding-session-61]])

Iterations 121–127 merged (PRs #158–#167 range, batch of 2026-07-19):
[[iteration-121-cookies-storage-actor-enumeration]],
[[iteration-122-navigate-dom-complete-ff152]] (+ [[iteration-124-live-sweep-followups]]),
[[iteration-123-daemon-autostart-and-per-port-registry]],
[[iteration-125-perf-audit-lcp-unavailable]],
[[iteration-126-network-json-shape-consistency]],
[[iteration-127-a11y-contrast-fail-only-total]].

## Regression Checks (vs session-61 issue numbers)

| # | Issue (session 61) | Prev | Current | Evidence |
|---|---|---|---|---|
| 1 | cookies StorageActor dead — httpOnly missed, flags null | MAJOR | **FIXED** | github.com: 6 cookies incl. 2 httpOnly, `isHttpOnly/isSecure/sameSite` populated; `--storage-only` total 6 |
| 2 | ~7 s navigate penalty | MAJOR | **FIXED** | example.com navigate wall 0.37 s, `elapsed_ms:348` |
| 3 | `elapsed_ms` off by ~7000× | MOD | **FIXED** | matches wall clock |
| 4 | perf audit false "good" 0 ms LCP | MOD→MAJ | **FIXED** | comparis: vitals AND audit both `null`/`"unavailable"`, keys real |
| 5 | `committed_url` = `about:blank` | MOD | **STILL BROKEN (SPA-only)** | static pages (example/wikipedia/github/httpbin) now correct; comparis SPA routes still `about:blank` |
| 6 | network object-vs-array shape flip | MOD | **FIXED** | quiet+busy identical canonical key set |
| 7 | a11y contrast `--fail-only` total = sample size | MOD | **FIXED** | HN: total 453 == results len, sampled 506; passing page: total 0, sampled 4 |
| 8 | daemon never starts / single global slot | MOD | **FIXED** | per-port `~/.ff-rdp/daemon.6200.json` + `daemon.6300.json` coexisting, both live |
| 9 | `snapshot --max-chars` near-no-op | MIN | **STILL BROKEN** | 100 vs 5000 vs default → 1741/1742/1743 B |
| 10 | `dom` `attrs.value` static, not live | MIN | **STILL BROKEN** | live `.value="42"` invisible, attr "0" reported |
| 11 | `responsive` media-query warning never emitted | LOW | **FIXED** | explicit warnings array when `matches:false` |
| 12 | `wait --timeout` deprecation warning | LOW | UNCHANGED (by design) | intended deprecation path |
| 13 | malformed `--jq` leaks Rust Debug struct | COSM | **STILL BROKEN (cosmetic)** | `Lex(...Delim...)` inside JSON error envelope; exit 1 is correct (s61's "exit 0" was a head-pipe artifact) |

iter-128 escape confirmed: truncated network object adds exactly `["hint"]` and nothing
else — [[iteration-128-network-hint-always-present]] covers it fully.

## Findings

### What Works Well

- URL/scheme validation errors: actionable, `error_type:"User"`, lists allowed schemes and `--allow-*` flags.
- `dom` ergonomics: ref handles, "showing N of M" summaries, next-command hints.
- `network --security`: full TLS version, cipher, HSTS, certificate chain.
- `screenshot --full-page`: exactly `body.scrollHeight × DPR` (6942 px verified with real content).
- `emulate --color-scheme dark` applied and verifiable via matchMedia.
- perf's LCP note clearly explains the Firefox platform limitation; vitals/audit now agree.
- Daemon per-port registries make parallel multi-instance work (two agents, two ports) boring — the session-61 confound class is structurally gone.

### Issues Found (new this session)

1. **[MAJOR] `--auto-consent` does not dismiss Sourcepoint CMPs → site-wide scroll lock.**
   Guardian: `sp_message_iframe_*` modal persists on every page, `html.sp-message-open`
   sets `overflow:hidden`, so `scroll bottom` silently no-ops (`atEnd:true`,
   `scrollHeight` == viewport) and content is covered. Consent-O-Matic loads but never
   records consent (`_sp_user_consent` actions empty). Removing the class via eval
   restores scrolling. Combined with (6) there is no CLI-native way to accept consent —
   an agent is fully blocked on Sourcepoint news sites.
2. **[MODERATE] `network --format text` unreadable — no URL truncation.** ~900-char
   Sourcepoint URLs expand the url column; table becomes thousands of columns wide.
   `sources --format text` has the same problem. Expected middle-ellipsis around ~80 cols.
3. **[MODERATE] `perf summary` per-resource `transfer_size` all 0 → misleading aggregate.**
   93 resources, every `transfer_size:0` (cross-origin without Timing-Allow-Origin), yet
   an aggregate `total_transfer_size:6386` is reported as if real. Omit or flag the
   aggregate when resource timing is opaque.
4. **[MODERATE] `network` fidelity depends on output mode.** Text/summary path: 18 rows,
   `source:watcher`, methods populated. `--detail`/`--jq` path: 93 rows,
   `source:performance-api`, method/status/content_type null — although the daemon holds
   840+ buffered watcher events. Agents piping `--jq` silently get the worst-fidelity
   source. Also `content_type` null even on watcher rows despite `--help` promising it.
5. **[MODERATE] `responsive` reports width-matching rects while media queries never fire.**
   `rect.width` equals the requested 320/768/1024 but `inner_width` stays 1366 and
   `matches:false` — layout looks responsive while `@media`-dependent UI is never
   exercised. The honest signal exists (warnings, from iter-98/s61 fix) but the matching
   rect widths actively invite over-trust. (Platform limit: no viewport actor — see
   [[project_viewport_protocol]]-class constraint; this is about presentation honesty.)
6. **[MODERATE] `click` cannot reach cross-origin iframe targets; generic 10 s timeout.**
   Selector matching only the CMP iframe's content times out with "not ready" instead of
   hinting the match exists only in a cross-origin frame.
7. **[LOW] `back`/`forward` return only `{"action":"back"}`** — no
   `committed_url`/`ready_state`/`elapsed_ms` like navigate; needed an extra eval to know
   where I landed.
8. **[LOW] perf right after `reload` races** (`total_resources:0`, populated moments
   later), and a slow-3g `throttle` effect could not be confirmed on reload (~50 ms
   fetch; may be cache/measurement artifact — unverified, not confirmed broken).
9. **[LOW/housekeeping] `~/.ff-rdp/` accumulates stale zero-byte `daemon.*.spawn.lock`
   files** (~50 from past runs) — never cleaned, grows unbounded.
10. **[INFO] routed commands don't self-identify** — `meta` is empty on daemon-routed
    commands; routing is only visible via `daemon status` + registry file.

### Minor friction

- `scroll --bottom` and `dom --stats` are natural-but-wrong guesses for the subcommand
  forms `scroll bottom` / `dom stats`; the `dom --stats` error tip points to `--attrs`
  which misleads.
- Top-level `await` in `eval` throws SyntaxError despite `eval_path:page-await`;
  `.then()` works. Agents reach for `await fetch(...)` naturally.

### Feature Gaps

1. CLI-native consent acceptance that can reach into cross-origin CMP iframes
   (Sourcepoint) — single biggest blocker on real news sites.
2. navigate-style landing envelope for `back`/`forward`.
3. `network --detail`/`--jq` should consume the richer watcher buffer.
4. URL truncation in `--format text` tables (network, sources).
5. Louder `responsive` signal when media queries don't apply.

## Summary

- ~28 commands exercised across 2 agents on 7 sites. **8 of 13 session-61 issues fixed**
  (including every iter-121/122/123/125/126/127 target, each verified against real
  sites); 4 still broken (1 narrowed to SPA-only, 1 cosmetic); 1 unchanged by design.
- **10 new findings** (1 major: Sourcepoint consent lock; 4 moderate) + 5 feature gaps.
- Key takeaway: the data-correctness batch landed cleanly end-to-end; the next
  highest-leverage themes are consent handling on CMP-gated sites and network output
  fidelity/readability.
