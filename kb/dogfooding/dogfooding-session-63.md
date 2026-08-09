---
title: Dogfooding Session 63 — batch 128–135 verification + BBC/GOV.UK exploratory
type: dogfooding
date: 2026-08-09
status: completed
site: www.theguardian.com, www.bbc.com/news, www.gov.uk, en.wikipedia.org, news.ycombinator.com, react.dev, openlibrary.org
commands_tested: [launch, navigate, tabs, page-text, snapshot, screenshot, dom, geometry, computed, styles, responsive, perf, network, sources, a11y, eval, cookies, storage, click, type, scroll, wait, reload, back, forward, consent, daemon, emulate, doctor, index, manifest, console]
tags: [dogfooding]
---

# Dogfooding Session 63

Verification of iterations 128–135 against the sites where session 62's issues were
found, plus fresh exploration of BBC News and GOV.UK. Four parallel agents on separate
Firefox ports. **Headline: most of the batch genuinely landed, but iter-129's entire
feature set works only with `--no-daemon` — the default daemon path silently returns
zero frame targets.**

Previous: [[dogfooding-session-62]]

## What's New Since Last Session

[[iteration-128-network-hint-always-present]], [[iteration-129-consent-and-cross-origin-frames]],
[[iteration-130-navigation-truthfulness]], [[iteration-131-measurement-honesty]],
[[iteration-132-cli-polish]], [[iteration-133-viewport-emulation]],
[[iteration-135-screenshot-ff153-capture-drift]].

## Regression Checks (vs session-62 issue numbers)

| # | Session-62 issue | Iter | Verdict |
|---|---|---|---|
| 1 | Sourcepoint `--auto-consent` / scroll lock | 129 | **PARTIALLY** — works only `--no-daemon`; scroll-lock warning FIXED |
| 2 | `network`/`sources --format text` unreadable | 128 | **FIXED** (107/105 char max lines on Guardian) |
| 3 | `perf summary` transfer_size 0 aggregate | 131 | **FIXED** (opaque → null + flag + count) |
| 4 | `network` fidelity differs by output mode | 128 | **FIXED** (both paths agree; `content_type` populated) |
| 5 | `responsive` width-matching rects dishonest | 131 | **FIXED** (`simulation`, `media_queries_applied`, `--strict` exits 1) |
| 6 | `click` cannot reach cross-origin iframes | 129 | **PARTIALLY** — works only `--no-daemon` |
| 7 | `back`/`forward` bare `{"action":"back"}` | 130 | **PARTIALLY** — cross-document fixed; **same-document now hard-fails** |
| 8 | `perf` post-reload race silent zero | 130 | **FIXED** (`resources_pending`) |
| 9 | stale `daemon.*.spawn.lock` | 132 | **FIXED** (5/5 fixture cases) |
| 10 | routed commands don't self-identify | 128 | **PARTIALLY** — 2 wired, 24 surveyed lack `route` |
| f1 | `scroll --bottom` / `dom --stats` hints | 132 | **FIXED** |
| f2 | top-level `await` in `eval` | 132 | **FIXED** (inline/`--stdin`/`--file`) |

iter-133 viewport: `screenshot --window-size` **FIXED** (verified at pixel level, genuine
mobile render); below-floor `launch --window-size` **FIXED** (request echoed + actionable
warning); above-floor **PARTIALLY** — `innerWidth` exact but `--window-size` sizes the
*outer* window, so `innerHeight` is 715 for a requested 800.

## Findings

### What Works Well

- `screenshot` viewport and `--full-page` — faithful, ~0.3 s (fixed by iter-135).
- `a11y summary --format text` — landmarks + heading outline in ~25 ms; best output in the tool.
- `responsive` — honest about its own limits.
- `navigate` URL guards — `javascript:`/`file:` refused; DNS failure → `nav_dns_fail` + hint.
- `scroll` (`bottom`/`text`/`to`), `emulate`, `doctor` stale-profile detection.
- `--jq` works everywhere and rescued several broken text outputs.

### Issues Found

**Daemon-mode parity (the big one)**

1. **[MAJOR] Daemon proxy returns 0 frame targets.** `enumerate_frame_targets` sees
   nothing through the proxy — not even the top-level target. Breaks `consent accept`
   (`{"cmp":null}` vs `{"cmp":"sourcepoint","action":"accepted"}`), cross-origin `click`,
   and `--frame`. Confirmed independently on Guardian and BBC. Cause: target events are
   consumed by the daemon's reader before the temporary sink in
   `crates/ff-rdp-core/src/actors/watcher.rs:365-385` can observe them.
   **Every iter-129 live test passes `--no-daemon`** (`live_129_frames_and_consent.rs:38`),
   so the default path was never exercised; the iteration's own `dogfood_path` fails as written.
2. **[MAJOR] Daemon caps concurrency at 2**, with `{"error":"operation timed out after 0ms
   (phase: recv)"}` — not a real duration. 4 parallel `page-text`: 2 succeed via daemon,
   4/4 succeed with `--no-daemon`. Also makes `network --follow` + `navigate` impossible.
3. **[MAJOR] `network` returns different data per mode** — daemon 77 rows/`watcher`,
   no-daemon 137 rows/`performance-api`, same page same moment.

**Navigation truthfulness**

4. **[MAJOR] `navigate` never reports HTTP status.** 404 and 503 pages return a normal
   success envelope (`ready_state:"complete"`); `document.title` is `"BBC - 404: Not Found"`.
   `network` can't fill the gap (`status` null on performance-api rows). No way to learn the
   main document's HTTP status. Highest-value single fix.
5. **[MAJOR] Same-document (`pushState`) `back`/`forward` hard-fails.** Blocks the full
   `--timeout`, exits 124, reports a Timeout error — while the traversal actually succeeded.
   Reproduced 4/4. **Severity regression vs session-62 #7.** Compounding: the error
   recommends `--no-wait`, which does not exist on `back`/`forward`/`reload`.
6. **[MAJOR] Same-page fragment navigation** (`https://www.gov.uk/#frag`) burns the full
   timeout then falsely reports failure; `location.href` confirms it succeeded.
7. **[MAJOR] Timeout messages report the wrong budget.** `--timeout 8000` → 8.10 s wall,
   message says 2384 ms; `--timeout 20000` → 20.19 s wall, says 5907 ms. Agents will size
   retries ~3× too small.
8. **[MODERATE] `back`/`forward` report a third-party iframe URL as `committed_url`**
   (both modes). `navigate` gets this right.
9. **[MODERATE] `navigate --with-network` drops `committed_url` and `ready_state`** (both
   null). Truthful navigation *or* network data, not both.

**Measurement honesty**

10. **[MAJOR] `perf` fabricates "good" CLS and TBT.** Firefox's
    `PerformanceObserver.supportedEntryTypes` has no `layout-shift` and no `longtask` —
    structurally unmeasurable — yet vitals report `cls: 0.0, "good"` and `tbt_ms: 0.0,
    "good"` on an ad-heavy BBC News. Same false-good class already fixed for LCP in iter-125.
11. **[MODERATE] `perf audit` byte attribution is self-contradictory** — `navigation.transfer_size`
    64035 vs `resource_by_type.document` 300; fonts appear to be 78 % of the page only
    because they are same-origin while images are opaque; `third_party_summary.count`
    equals the total count (first-party counted as third-party).
12. **[MODERATE] `perf vitals` has no page identity** — no URL, no timestamp; returned stale
    18 s FCP/TTFB from a previous failed navigation.
13. **[MODERATE] `perf summary --format text` still has session-62 issue 2** — untruncated
    URLs, lines of 6709 and 7378 chars. Only `network` and `sources` got the fix.

**Element targeting**

14. **[MAJOR] `--ref` is broken three ways.** Refs round-trip as a JS expression fed to
    `querySelector` (`Document.querySelector: 'document.querySelectorAll('button')[0]' is not
    a valid selector`); the registry is **single-use** (second call → `ref e2 not found`);
    `click --ref` additionally burns 10 s. Advertised across many commands.
15. **[MODERATE] `click`/`type` take the first DOM match blindly.** `type` picks a hidden
    `[0]` and fails while `geometry` reports the visible match — the two commands disagree
    about what a selector means. Error conflates not-found/hidden/unstable, doesn't say how
    many matched, costs 10 s. No `--index`/`--nth`/`--visible` to recover.
16. **[MODERATE] Generated page-map hands agents unusable selectors** — `{"label":"Search",
    "selector":"button"}` matches every button on the page.
17. **[MODERATE] `click`'s frame diagnostic dumps every frame URL untruncated** — 65 KB error
    on a 97-frame page; iter-128's middle-ellipsis never applied here.
18. **[LOW] `--frame` error miscounts** — reports `targets.len()` not the filtered
    `candidates.len()`, claiming 97 frames tried when `--frame` narrowed to a handful.
19. **[LOW] `click`'s `results.frame_url` does not exist** — `--help` promises it is always
    present so `--jq '.results.frame_url'` never throws; it lives in `meta` only.

**Output hygiene**

20. **[MAJOR] `--format text` pads every row to the widest cell.** `console --level error`
    → 39 lines, **255 KB**, rows padded to 8725 columns because one message is that long.
    The token-saving format costs ~64 k tokens. `dom` truncates class names, so the logic
    exists but isn't applied to `message`/`attrs`.
21. **[MAJOR] `index` emits invalid JSON** — the internal `navigate` result and the index
    result are concatenated, so `| jq` breaks. Its robots.txt parser also ignores
    user-agent grouping (applied `deepcrawl`'s `Disallow: /` to `*`), indexing 1 page
    instead of 3; each URL is enqueued twice.
22. **[MODERATE] `snapshot` is either near-empty or unaffordable** — default depth finds
    1 interactive element out of 191; `--depth 30 --max-chars 500000` → 644 KB (~160 k
    tokens), 40 % styled-component hashes. `truncated: true` is buried at line 3248 and
    absent from `meta`. `--max-chars` help says "characters of text content" but bounds
    the serialized tree.
23. **[MODERATE] `--format text` prints bare `[]` and drops JSON metadata** — `a11y contrast
    --fail-only` loses `sampled: 218` and `capped: true`, so a truncated sample reads as a
    clean bill of health, then offers to screenshot issues that don't exist.
24. **[MODERATE] CSS syntax errors bypass the JSON envelope** — plain-text stderr instead of
    `{"error":…,"error_type":…}`.
25. **[MODERATE] `sources` always returns an empty `actor`**, breaking the documented chain
    to `inspect`; in text mode URLs elide to identical-looking strings.
26. **[LOW] `network` JSON returns 20 of N with no truncation flag**; text mode disagrees
    with `--detail` JSON on totals.

**Housekeeping**

27. **[MODERATE] `daemon stop` false-negative, 3/3 reproducible** — errors that the port is
    still listening; `lsof` shows it free 2–3 s later. Reports the daemon pid as the Firefox pid.
28. **[MODERATE] `--auto-consent` silently fails on non-Sourcepoint CMPs** — BBC's banner
    (`#bbccookies-continue-button`) untouched, nothing warned; leaves a permanent
    `Consent-O-Matic Options` tab in every `tabs` listing.
29. **[MODERATE] Full-page screenshot duplicates the sticky header mid-page** (BBC, y≈3290).
30. **[LOW] Disk accumulation** — 62 temp profiles / 2.7 GB; `daemon.*.throttle.json` for 5
    dead pids; legacy `daemon.spawn.lock` uncollectable (`parse_spawn_lock_port` requires a port).
31. **[LOW] `eval` async-IIFE wrapper leaks** on ASI-separated scripts →
    `missing ) in parenthetical` at a line number past the end of the input. Multi-statement
    completion-value semantics invert on `await` (silent `{"type":"undefined"}`).
32. **[LOW] `wait` has no plain sleep form** — `--timeout` requires a condition flag.
33. **[LOW] Console output is locale-dependent** — German messages; `launch` should pin
    `intl.accept_languages`.
34. **[LOW] `doctor` reports `binary_staleness: skipped — not in an ff-rdp checkout`** while
    in the checkout (keys off cwd).

### ff-rdp-core live test failures (triaged separately)

Triaged as four **test-only bugs, no product bug**. **That was wrong for the fourth** —
implementing [[iteration-136-core-live-test-repairs]] found `AccessibilityActor` genuinely
broken, masked by a silent JS-eval fallback. The three cookie tests were test-only as triaged:

- `live_cookies`, `live_cookies_empty` — cleanup sends the **oneway** `unwatchResources`
  via `send_raw()` (send + `.expect("recv")`); recv times out and panics after the real
  assertions passed. `watcher.rs:41` documents it as oneway.
- `live_cookies_httponly` — **hang**: `listener.incoming().take(10)` waits for a second HTTP
  request that never arrives while the main thread blocks in `server.join()`.
- `live_accessibility_tree` — **product bug, not a test bug.** FF153's `accessibleWalkerSpec`
  has neither `getRootNode` nor `getDocument`; the root comes from an argument-less `children`
  on the walker, a node's children from `children` on the accessible actor, and nothing answers
  until the platform a11y service is enabled (the request stalls to the socket timeout rather
  than erroring). The product's fallback chain was dead code and `ff-rdp a11y` had been silently
  degrading to its JS-eval tree. Fixed in [[iteration-136-core-live-test-repairs]]; whether
  `a11y` should enable the browser-global service is deferred to [[iteration-143-native-a11y-tree]].

### Feature Gaps

1. HTTP status on `navigate` (currently unobtainable anywhere).
2. `type --enter` / `--submit`.
3. `--index N` / `--visible` on `click`/`type`/`styles`.
4. `screenshot --selector <sel>` for element/region capture.
5. Compact/minified JSON mode (pretty-printing inflated snapshot 4.3×).
6. Attribute-noise control on `snapshot`/`dom` (`--no-class`, `--attrs`).
7. `network --follow` concurrent with `navigate`.
8. `a11y summary` flagging a missing `h1`.

## Summary

- 30+ commands exercised across 7 sites by 4 parallel agents; 34 issues found, 12 regression
  checks performed.
- 8 of session-62's 12 items fully fixed, 4 partially.
- Key takeaway: **the default daemon path is the weakest surface in the tool** — zero frame
  targets, a 2-connection cap, and mode-dependent network data — and iter-129 shipped green
  only because all its live tests bypassed it with `--no-daemon`.
