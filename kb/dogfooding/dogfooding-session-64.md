---
title: Dogfooding Session 64
type: dogfooding
date: 2026-10-04
status: completed
site: www.comparis.ch/hypotheken, news.ycombinator.com, en.wikipedia.org, developer.mozilla.org, react.dev, expired.badssl.com, httpbin.org
commands_tested: [launch, home, navigate, tabs, page-text, snapshot, dom, a11y, click, type, wait, back, forward, reload, scroll, network, perf, screenshot, responsive, computed, styles, cascade, geometry, console, cookies, storage, sources, inspect, eval, run, consent, doctor, index, profiles]
tags: [dogfooding]
---

# Dogfooding Session 64

Pre-release check of the post-reset build (`target/release/ff-rdp` 0.3.0, e38f82f1, no daemon)
on headless Firefox 157. Three parallel agents ran on separate ports with isolated `FF_RDP_HOME`s:
a comparis.ch mortgage funnel (SPA, consentmanager.net CMP, masked form inputs, heavy third-party
traffic), a baseline plus CSS/DOM pass on Hacker News and Wikipedia, and a regression pass on MDN,
react.dev and httpbin. Each agent killed only its own Firefox by pid. About 34 commands were exercised
and 26 of the session-63 items re-tested (20 fixed, 2 still broken, 4 N/A). **49 issues were found,
6 of them lies.** The headline: the reset surface is clean, and every new flag does roughly what it
claims, but truthfulness bugs are back. They come from new flags (`--throttle`) and from old surfaces
(TLS error pages, Promise rejections, refs).

Previous: [[dogfooding-session-63]]

## What's new since session 63

- **Daemon removed.** Every command opens its own connection. That makes session-63 issues 1–3 and 27 moot ([[step-back-2026-10-02]]).
- **Refs live in the page.** `snapshot`/`a11y`/`--with-page` stamp `data-ffrdp-ref="eN"` on elements, and `click/type --ref eN` resolve the attribute.
- **New `navigate` flags.** `--throttle`, `--block`, `--user-agent`, and `--with-network --headers/--security` (the headers are fetched before the connection closes).
- **New `screenshot` flags.** `--color-scheme dark|light` and `--media print` replace the removed `emulate`.
- **Auto-wait budget.** The rect-stability probe now shares the command's `--timeout-ms` instead of using a fixed sub-budget.
- **Process cut.** Follow-ups go in `kb/backlog.md` instead of GitHub issues or per-item plans, CI has one required `ci` check, and docs-only PRs skip the Rust matrix.

## Regression checks

| Session 63 | Now |
|---|---|
| 1–3, 27 daemon: 0 frame targets, 2-conn cap, mode-dependent network, `daemon stop` false negative | **N/A**: the daemon is gone. `network --follow` alongside `reload` works (74 req / 63 resp) |
| 4 `navigate` never reports HTTP status | **FIXED**: 404 → `"status":404`. A redirect reports the committed doc's status |
| 5 pushState `back`/`forward` hard-fails | **FIXED** (0.63 s, `no_document_request`) |
| 6 same-page `#frag` burns the timeout | **FIXED** (307 ms). react.dev `#frag2` reported `304`, unconfirmed |
| 7 timeout message wrong budget | **FIXED** |
| 8 `back` reports iframe URL | **FIXED** on MDN |
| 9 `--with-network` drops `committed_url`/`ready_state` | **FIXED** |
| 10 perf fabricates good CLS/TBT | **FIXED** (`null` + `"unavailable"`) |
| 11 perf audit third-party = total | **FIXED** (8 of 123) |
| 12 `perf vitals` no page identity | **FIXED** (`page_url`, `measured_at_ms`) |
| 13 `perf summary --format text` 7k-char lines | **FIXED** (max 132) |
| 14 `--ref` broken three ways | **FIXED**: round-trips and can be reused. New ref issues are #5 and #27 |
| 15 no `--index/--visible` | **FIXED** on click and type, and `type --submit` exists |
| 19 `results.frame_url` missing | **FIXED** |
| 20 text rows padded to widest cell | **FIXED** (498 B on react.dev console) |
| 21 `index` emits invalid JSON | **FIXED** (`jq -e .` passes). robots grouping not re-tested |
| 22 `snapshot` near-empty at default depth | **STILL BROKEN** (#4) |
| 23 text mode drops `sampled`/`capped` | **FIXED** (footer line) |
| 24 CSS syntax errors bypass envelope | **FIXED** (`error_type:"User"`) |
| 25 `sources` empty `actor` | **CHANGED**: filled in, but per-connection, so unusable (#18) |
| 26 `network` 20 of N, no flag | **FIXED** (`shown/total/truncated`) |
| 28 Consent-O-Matic tab | **N/A** (no extension launch this session) |
| 30 temp profile accumulation | **IMPROVED** (`--replace` and next launch sweep dead profiles) |
| 31 eval ASI / `await` completion | **FIXED** |
| 32 `wait` has no sleep form | **FIXED** (`--sleep-ms`) |
| 33 console locale German | **STILL BROKEN** (#37) |
| 34 doctor `binary_staleness` skipped | **FIXED** |
| 16–18, 29 | not re-tested |

## Smoke results

| command | status | note |
|---|---|---|
| `launch --headless` / `--replace` / occupied port | ✅ | own FF → `already_running`; foreign owner named. The hint is wrong (#36) |
| `ff-rdp` (home view) | ✅ | first suggested action is the HN logo (`link ""`) |
| `navigate` (HN, Wikipedia, MDN, react.dev) | ✅ | `committed_url`, `status`, `ready_state` |
| `navigate` DNS fail / refused / bad scheme | ✅ | JSON errors, exit 7/9/1 |
| `navigate` expired TLS cert | ❌ | reports success (#1) |
| `navigate --auto-consent` / `consent accept` (comparis) | ❌ | consentmanager.net unsupported (#23) |
| `navigate --throttle slow-3g` | ❌ | throttles, but lies about `ready_state` (#2, #13) |
| `navigate --block` | ⚠️ | blocks, but no evidence in output (#25) |
| `navigate --user-agent` | ✅ | httpbin echoes it; the override ends with the command |
| `navigate --with-network --headers --security` | ✅ | headers and TLS on all entries; duplication (#44) |
| `back` / `forward` / `reload` | ✅ | `reload --wait-idle` status oddity (#19) |
| `page-text` (`--query`, `--max-chars`) | ⚠️ | windows joined with no separator (#43) |
| `snapshot` default / `--depth` / `--query` / `--format text` | ❌ | 0 refs at default depth (#4); budget (#21); query (#31); text refs (#41) |
| `dom` / `dom stats` / `--count` / `dom tree` | ✅ | `dom tree` has nodeName only |
| `a11y summary` / `--all` / `--selector` | ⚠️ | 43 banners (#16); `--all` is the only surface that lists form controls |
| `a11y contrast --fail-only` | ❌ | wrong sample (#9) |
| `click --ref` (link, `--with-page`, `--index/--visible`) | ✅ | plain click returns before navigation (#28) |
| `click --ref` (custom radio / disabled / stale) | ❌ | #29, #22, #5 |
| `click --wait-for-network` | ✅ | matched request, `duration_ms:0` |
| `type --ref --text` / `--submit` | ✅ | reports the masked value (`850'000`) |
| `wait --text/--selector/--eval/--sleep-ms` | ✅ | clear timeout messages, exit 124 |
| `scroll by/to/top/bottom` | ✅ | |
| `scroll text` / `until` | ⚠️ | wrong match (#15); brute force (#42) |
| `network` one-shot (watcher) | ❌ | hangs on comparis (#6); fresh connection gives a good hint |
| `network --source performance-api` | ✅ | |
| `network --follow` | ⚠️ | key POST responses lack status (#17) |
| `perf vitals` / `summary` / `audit` | ⚠️ | stable; LCP claim false (#11); probes pollute the console (#38) |
| `screenshot` / `--full-page` | ✅ | comparis 1366×7804, no repeated sticky header |
| `screenshot --color-scheme dark` / `--media print` | ✅ | dark MDN and print layout confirmed; dark+print refused |
| `screenshot --viewport-height` | ❌ | advertised, refused (#32) |
| `responsive --widths` | ❌ | comparis h1 820 px at every width (#10); honest warnings on Wikipedia |
| `computed` / `geometry` | ✅ | `geometry` overlap found the FAB covering submit |
| `styles` / `--applied` / `--layout` | ❌ | `--layout` zeros (#7); no stylesheet URL |
| `cascade` | ❌ | wrong `computed`, null stylesheet (#8) |
| `console` | ⚠️ | warnings labelled `error` (#14); German (#37) |
| `cookies` | ⚠️ | ignores `--fields/--limit/--sort` (#26) |
| `storage` | ⚠️ | 1.05 MB on Wikipedia (#24) |
| `sources` | ✅ | 109 real actors (#285 holds); actor id is per-connection (#18) |
| `inspect` | ⚠️ | accessor values missing (#30) |
| `eval` (Promise, `await`, fetch, `--file/--stdin`) | ✅ | resolves. The memory note "won't resolve Promises" is obsolete |
| `eval` rejected Promise | ❌ | `null`, rc 0 (#3) |
| `run` (YAML playbook) | ❌ | `wait` after navigating click (#12) |
| `doctor` / `index` / `profiles prune` | ✅ | minor (#45, #46) |
| README verbatim (15 examples) | ⚠️ | 14 pass; `--viewport-height` fails |
| removed-flag grep (README, skills, every `--help`) | ✅ | clean; only `kb/reference/script-format.md` (#34) |

## Issues found

### lie (output claims something false)

1. **`navigate` reports success on a TLS error page** (baseline). `navigate https://expired.badssl.com/` → `{"status":null,"status_reason":"no_status_reported","committed_url":"https://expired.badssl.com/","ready_state":"complete"}`, rc 0. `tabs` shows `about:certerror?e=nssBadCert…` and `page-text` is `""`. Expected: a `nav_cert_error` error, as DNS and refused already get.
2. **`navigate --throttle slow-3g` reports `ready_state:"complete"` while the document is interactive** (regress #1, comparis #3). `navigate about:blank; navigate 'https://react.dev/learn?df=b2' --throttle slow-3g --timeout 60000` → `{"status":200,"ready_state":"complete","elapsed_ms":474}`. The next `eval document.readyState` → `"interactive"`, `loadEventEnd` 0 (2/2 repros). On comparis it returned `interactive` after 4.4 s, and the page was still wedged 48 s after exit. Expected: `complete` only when complete, and in-flight requests not stranded.
3. **`eval` turns a rejected Promise into `null`** (baseline 3b). `eval 'Promise.reject(new Error("boom"))'` → `{"results":null,"total":1}`, rc 0. Expected: the error envelope that a sync `throw` already gets.
4. **`snapshot` default depth yields 0 refs with `truncated:false`** (all three). On HN, depth 6 stops at `<td truncated: "6 children not shown">`. On react.dev it gives 0 refs among 140 links and buttons. On comparis it folds `<main>`. Expected: `truncated:true` and a depth hint, or a better default.
5. **A stale ref silently clicks a different element** (regress #3). Refs restart at `e1` on every document. On MDN `e4` is the logo. After `navigate https://react.dev`, `click --ref e4` clicks react.dev's "Learn" link with no warning. Help says refs "die". Expected: per-document epoch in ref ids.
6. **One-shot `network` hangs and ignores `--timeout`** (comparis #2). `network --timeout 3000` on comparis prints nothing for 30 s (killed, exit 142); with the default timeout it runs past 2 min (3/3). Help says it "collects for a short window". Likely cause: beacon traffic never goes quiet.

### wrong (incorrect data)

7. **`styles --layout` box model is all zeros.** `styles 'table.infobox' --layout` → margin, border and padding all `0.0`. `getComputedStyle` gives marginLeft 14.08px, borderTop 1px and paddingTop 2.82px.
8. **`cascade` `computed` field is the declared value.** `cascade 'table.wikitable > * > tr > th' --prop background-color` → `"computed":"var(--background-color-neutral,#eaecf0)"`. `width` gives `"22em"` (actual 309.7px). `stylesheet:null` and `line:1` appear on every rule, there are no UA rules despite the help, and `winner_verified` is unexplained.
9. **`a11y contrast` samples the wrong elements** (comparis #4, regress #13). With no selector, comparis checks 14 nav items and never the 57 `<p>` in `main`. `--selector main` checks 0, so the selector does not include descendants. react.dev reports fg = bg `#99a1b3`, ratio 1 (a transparent bg is not resolved).
10. **`responsive` geometry is identical across widths.** `responsive h1 --widths 320,768,1024` on comparis → `width:820` at all three, while the warning says "geometry is accurate for the requested width".
11. **`perf vitals` says LCP is unsupported, which is false on Firefox 157** (comparis #8, regress #12). `supportedEntryTypes` includes `largest-contentful-paint`, and a buffered observer via `eval` got 71 ms. The tool returns `lcp_ms:null` with `lcp_approximate:true` ("estimated via DOM approximation"), which contradicts itself. `fcp_rating:null` should be `"unavailable"`.
12. **`run`: `wait` after a navigating `click` polls the dead document.** Playbook navigate HN → click `item?id=` → `wait {selector: ".comment-tree"}` → not found after 5000 ms. A standalone `dom '.comment-tree' --count` returns 1. `assert_url` sees the old URL.
13. **Throttled `--with-network` returns a partial capture unflagged** (regress #2). `navigate … --throttle slow-3g --with-network --timeout 60000` → `elapsed_ms:3222`, `ready_state:"interactive"`, 44 of ~67 requests, `timeout_reached:false`. Expected: collect up to `--network-timeout`, or mark the capture `incomplete`.
14. **`console` labels Firefox warnings as `level:"error"`** (baseline #13, regress #8). Unsupported-entryTypes, cookie-partitioned, Referrer-Policy and preload-unused messages all come back as `error`. Likely cause: pageError `warning:true` is unmapped (inferred).
15. **`scroll text` picks the first match.** `scroll text 'Further reading'` on Wikipedia lands on the sidebar TOC `span` (y 26961), not the `<h2>` (y≈43010). Expected: prefer content, or report `matches` and accept `--index`.
16. **`a11y summary` landmarks ignore ARIA scoping.** comparis lists 43 `banner <header>`; only 2 sit outside sectioning elements.
17. **`network --follow` drops response fields.** comparis `GetCalculatedProduct` POST has a `response` event with no `status`, `duration_ms` or `transfer_size`. 54 of 168 responses have `status:null`, 20 of them XHRs.
18. **`sources` `actor` is per-connection** (`conn29` → `conn30` on the next call), so the documented chain to other commands cannot use it.
19. **`reload --wait-idle` reports `status_reason:"not_observed"` alongside `requests_observed:293`.** Help defines `not_observed` as never subscribed. 293 is also about twice MDN's 148-request load.
20. **`--with-page` misreports SPA pages.** It collects before SPA work settles, so it showed the consent link after the click, an empty view before the autocomplete rendered, and the "wird berechnet" interstitial. On non-readerable pages the 50-entry cap fills with nav links and drops all 12 radios and 5 inputs.
21. **`snapshot` size budget counts compact bytes.** `--depth 20` on HN: `max_chars:50000`, compact 50,442 B, but stdout is pretty-printed at 351,696 B.
22. **A click on a disabled control returns `clicked:true`.** comparis step-4 "Ja" label while greyed out: `{"text":"Ja"}`, nothing checked. That the input was disabled is inferred from its styling.

### gap (missing)

23. **No consentmanager.net CMP.** comparis (`#cmpbox`, `a.cmpboxbtnyes`, `__tcfapi`): `navigate --auto-consent` → `{"cmp":null,"status":"no_cmp_detected"}` with the banner visible. `consent accept` → `consent_no_cmp`, exit 1. `CMP_TABLE` has only Sourcepoint.
24. **`storage` has no per-value cap.** `storage localStorage` on Wikipedia prints 1,055,610 B (`MediaWikiModuleStore:enwiki` is 1 MB). `--redact-threshold` and `--limit` have no effect.
25. **`--block` leaves no `blocked` marker** (regress #5, comparis). Blocked entries look in-flight (`status:null`), the summary has no `blocked` count, and `--block ''` is silently dropped.
26. **`cookies` ignores `--fields`, `--limit` and `--sort`** (both comparis and baseline). `cookies --fields name,bogus` exits 0 with all 10 keys, whereas `console` rejects `bogus`. `expires` mixes epoch-ms and `"Session"`.
27. **Unknown or stale refs fail as generic selector timeouts** (all three). `click --ref e9999` → `selector '[data-ffrdp-ref="e9999"]' not ready — 0 elements matched` after ~2.9 s, exit 124. Expected: an immediate "ref not on this page; re-run snapshot". React re-mounts also kill refs with no navigation, and `--with-page` stamped 1478 elements to return 50.
28. **A plain `click` on a link gives no navigation signal.** `click --ref e23` → `clicked:true`, and `tabs` and `page-text` straight afterwards still read the old page.
29. **Custom-styled radios cannot be clicked by ref.** For `opacity:0; z-index:-1` inside a `<label>`, `click --ref e167` waits 10 s ("readiness was not established"), and `--no-wait` blames the radio's own svg. Expected: fall back to the `<label>`.
30. **`inspect` shows no values for accessors.** `inspect 'window.location'` → `href: {"get":{…},"set":{…}}`. `document.body` → `ownProperties:{}`.
31. **`snapshot --query` searches only the depth-limited tree.** It finds 0 "Hypothek" at default depth on the Hypotheken page and 2 at `--depth 30`, with no hint.

### docs

32. **`screenshot --viewport-height` is advertised and refused.** `README.md:480` and `screenshot --help` list it, but the run gives `{"error":"screenshot: --viewport-height is not supported; use --full-page…"}`.
33. **The shipped `ff-rdp-debug` skill uses nonexistent flags.** `playbooks/K0.md:42` has `network --status`, `K0.md:49` and `evals/fixtures/K0/bug.json` have `snapshot --interactive-only`, and `C2.md:37,98` has `snapshot --filter`.
34. **`kb/reference/script-format.md` is stale.** Lines 67 and 111 still mention the daemon, and line 152 documents `--env-file`, which is now rejected.
35. **`--help` contradicts behaviour.** `styles`, `a11y summary` and `dom --count` output shapes differ from what is documented. `navigate` says it waits for "interactive or complete". `--network-timeout` says "runs for this duration". `click --ref` says refs "die". `network` says "short window". `a11y contrast` says "default: all text elements". `snapshot --query` omits the depth limit. Value name `<WAIT_TIMEOUT>`. Iteration history and irrelevant global flags clutter every subcommand.
36. **`launch` occupied-port hint suggests `--replace` for a foreign owner,** which `--replace` then refuses.

### polish

37. **Console and page text are in German** (all three). Pin `intl.locale.requested`/`intl.accept_languages` in the launch profile.
38. **`perf vitals`/`audit` log 2 console errors per call** ("Folgende nicht unterstützte entryTypes …: longtask/layout-shift"). After 4 calls, 8 of 18 `console --level error` entries were the tool's own.
39. **Broken-pipe panic.** `snapshot --depth 30 | head -1` → `panicked … failed printing to stdout: Broken pipe`. Same for `page-text --full | head -c 10`.
40. **Exit codes and error classes are inconsistent.** `click 'div:::x'` → `Timeout`, rc 124, while `dom 'tr[['` → `User`. Timeouts give rc 124 for click/scroll but rc 5 for eval. `error_type` casing is mixed. clap errors are plain text.
41. **`snapshot --format text` prints no `e<N>` refs,** so the cheap format cannot drive `click --ref`.
42. **`scroll until` brute-forces existing elements.** `scroll until '#External_links'` times out at 10 s saying "not found". With `--timeout 30000` it took 79 scrolls and 16.3 s. Expected: `scrollIntoView` when the element already matches.
43. **`page-text --query` windows are joined with no gap marker.** HN story titles appear to be missing, and headings carry an "edit" suffix.
44. **`navigate --with-network`:** `slowest` repeats `entries` (~11 KB), `total_transfer_bytes` is a float, and `data:` URLs report an invented `status:200`.
45. **Home and doctor disagree.** Home says "version unknown" while `doctor` says FF 157. The doctor tested range "120–150" is stale.
46. **`profiles prune --all --dry-run` fills `removed_live`.** It should be `would_remove_live`.
47. **`--version` prints 0.3.0.** Bump it before tagging.
48. **`click`'s obscured error always suggests `consent accept`,** even for a `position:fixed` FAB or the target's own label svg. `scroll to --block center` is the right hint there.
49. **`type --ref eN V` rejects positional text** without mentioning `--text`.

## What works well

- The reset surface is clean. No removed flag survives in README, the skill or any `--help`, and every new flag does what it claims.
- The comparis mortgage funnel can be completed with ff-rdp alone. `type` fires React handlers on masked inputs, and `click --ref --with-page` follows client-side routes.
- `navigate`/`back`/`forward`/`reload` truthfulness, apart from TLS and throttle. HTTP status is reported, bfcache is labelled `no_document_request`, and URL guards give JSON errors with distinct exit codes.
- `navigate --with-page --query` on Wikipedia: the Readability excerpt and content refs first in 8 KB make it the best orientation path.
- `screenshot` (full page, dark, print), `computed --prop`, `dom`, `geometry` overlap detection and `wait` are concise and correct. Most commands finish in 40–100 ms.
- `eval` resolves Promises, `await` and fetch. Error envelopes are JSON with hints, and the success path writes nothing to stderr.

## Feature gaps

- Click by text or label (`click --text "Wohnung"`). The `label:has([data-ffrdp-ref=…])` workaround is undiscoverable.
- `page-text --reader`. Readability is reachable only through `--with-page`.
- `run` step verbs for `page_text`/`snapshot`, plus a documented step schema (fields were found from serde errors).
- Radio-group/legend context and `role=option` entries in `a11y summary`. `a11y --selector` loses th→td pairing on infoboxes.
- Document-absolute `geometry`. Inherited and UA rules in `cascade`.
- LCP through a buffered observer armed in one call and read in the next.

## Summary

- 3 parallel agents, 7 sites, ~34 commands. 26 session-63 items re-tested: 20 fixed, 2 still broken (#22 snapshot depth, #33 locale), 4 N/A.
- 49 issues: **6 lie**, 16 wrong, 9 gap, 5 docs, 13 polish.
- Takeaway: **removing the daemon fixed the parity class from session 63, but six new lies (cert errors, throttled `complete`, swallowed rejections, `truncated:false` with 0 refs, rebinding refs, a `network` hang) block calling the reset build release-ready.**
