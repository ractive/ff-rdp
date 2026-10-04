---
title: Backlog
type: backlog
status: in-progress
date: 2026-10-03
tags: [backlog]
---

# Backlog

One line per item, with where it came from. Tick it in the same commit as the fix. This file
replaces GitHub issues and per-item iteration plans for follow-ups; a plan file is only for real
product work that needs design. Keep items to one line — if an item needs more, it is a plan.

## Product

- [x] `navigate --with-network --headers` (and `click --wait-for-network`): fetch response headers before the connection closes; `network --headers` after a finished navigate returns nothing (#287)
- [x] Skill playbooks A2, D2, E1, E3 (and their `evals/fixtures/*/bug.json`) still read headers with an after-the-fact `network --detail --headers`, which returns nothing on a new connection; rewrite them to `navigate <page> --with-network --headers --all` filtered by `--jq` (A1 done) (#287 follow-up)
- [x] Per-command emulation flags replacing the removed `emulate`: `screenshot --color-scheme dark|light`, `screenshot --media print`, `navigate --user-agent <ua>` — settings reset with the connection (#287)
- [ ] `navigate --user-agent` is refused with `--with-network` and `--auto-consent`: both run part of the command outside `run_core`'s connection (`navigate/network_capture.rs`, the consent click's second connection), so the override would not cover them (#297)
- [ ] `snapshot` / `a11y` stamp every interactive element with `data-ffrdp-ref` before the output cap applies; stamp only what is emitted (#287)
- [x] Auto-wait's rect-stability probe has a fixed sub-budget that fails under machine load ("auto-wait stopped during rect stability probe did not answer"); first nightly 2026-10-03 hit it in live_160/210/224/237 at `--jobs 4` on a 4-core runner. Make the probe share the command's `--timeout-ms` instead of a separate small budget (nightly run 37071459324)
- [ ] `consent accept` on theguardian.com (Sourcepoint frame detected, not accepted) and bbc.com (no CMP reported) failed on the ubuntu CI runner in the sites job on 2026-10-03 while passing locally the same day — geo/CMP variant difference; check from a non-CH IP before touching the selectors (run 37071459324)
- [ ] `navigate/readiness.rs` still releases a LongString header grip (`GripHandle::without_queue(..).release`); likely removable like eval's (#291)

## Tests and tooling

- [x] Remaining wall-clock bounds in live tests measure the laptop, not the product: live_113, 129, 145, 158, 174, 237, 272, `live_bulk_cap`, `live_navigate_default_fast` — delete the bounds, keep the functional assertions (#290)
- [ ] Live-sweep post-phase leak check was deleted with the real-root scan; leaked test Firefoxes are now only caught on the watchdog path — run `managed_firefox_pids()` after every phase (#291)
- [ ] `clippy::assert_is_empty` is allowed in the lint tables rather than fixed at ~30 test sites (#286)
- [ ] `kb/iterations/dogfood-lib.sh` and the historical `*.dogfood.sh` scripts remain only because `tools/axi-bench` sources the lib; move the lib under `tools/axi-bench/` and drop the scripts (#286)
- [ ] `count_tabs` in `launch` does not reuse `connect_tab`'s list helper because that helper takes `&Cli` (#291)
- [x] Per-test `FF_RDP_HOME`/profile dirs leaked under `$TMPDIR` (1,157 in two days); now removed on drop, including after panics (#302)
- [ ] Firefox leaves `remote-settings-startup-bundle-<n>` in the system temp dir when killed during its startup download; tests confine `TMPDIR`, which Gecko ignores on macOS — needs a launch pref or policy that skips the startup bundle (#302)
- [ ] `kb/dogfooding/*` session logs still mention `--wait-timeout`; historical, leave or annotate (#291)
- [ ] `live_record_fixtures::live_page_style_get_layout` fails on FF157 (walker `querySelector h1` returns no node after the reconnect), so `page_style_get_layout_response.json` still holds the old unit-less shape; Firefox now sends `"margin-top": "7.03906px"` — re-record once the recorder works (fix/easy-batch-2)

## Global skills (edit by hand in `~/.claude/skills/`, shared with other repos)

- [x] `create-pr`: drop `check-dogfood-script`; run the xtask `check-*` enumeration and the fmt/clippy/test gates only when `git rev-parse HEAD^{tree}` differs from the last green run (see [[reset-2026-10-execution]] "Global skill follow-ups")
- [x] `review-pr`: one local review pass per PR; no re-review for style-only fixes
- [x] `merge-pr`: no separate `status: done` commit — the flip goes in the same commit as the code

## From dogfooding session 64 (2026-10-04)

- [x] **lie** `navigate https://expired.badssl.com/` reports success (`ready_state:"complete"`, rc 0) while the tab is on about:certerror; return a `nav_cert_error` ([[dogfooding-session-64]] #1) (fix/navigate-truthfulness)
- [x] **lie** `navigate --throttle slow-3g` reports `ready_state:"complete"` at ~480 ms while the document is still interactive, and strands in-flight requests after exit (2/2 repros; two agents) ([[dogfooding-session-64]] #2) (fix/navigate-truthfulness: cause was an iframe's `dom-complete` taken for the page's; the "stranded" page was the throttled load still finishing at the throttled pace after the early exit)
- [x] **lie** `eval 'Promise.reject(new Error("boom"))'` returns `{"results":null}` with rc 0 instead of the error envelope a sync throw gets ([[dogfooding-session-64]] #3) — fixed: error envelope with `stack`, `promise_rejected:true`, rc 1
- [x] **lie** `snapshot` at default depth returns 0 refs on HN and react.dev with `meta.truncated:false` and no depth hint ([[dogfooding-session-64]] #4) — fixed: folded nodes list `refs_below`; `truncated`/`depth_truncated` + hint
- [x] **lie** refs restart at `e1` on every document, so a stale `click --ref e4` silently clicks a different element on the next page; add a per-document epoch ([[dogfooding-session-64]] #5) — fixed: refs numbered from a random per-document base; stale ref → `stale_ref`
- [x] **lie** one-shot `network` on comparis hangs past `--timeout 3000` (30 s+, 3/3) because beacon traffic never goes quiet ([[dogfooding-session-64]] #6)
- [x] **wrong** `styles --layout` reports margin/border/padding all 0 where `getComputedStyle` gives 14px/1px/2.8px ([[dogfooding-session-64]] #7) (fix/easy-batch-2: `getLayout` sends `"7.03906px"`; the parser rejected the unit)
- [ ] **wrong** `cascade` puts the winning declared value (`var(...)`, `22em`) in `computed`, has `stylesheet:null`/`line:1` on every rule, and lists no UA rules ([[dogfooding-session-64]] #8)
- [ ] **wrong** `a11y contrast` default samples only nav items, `--selector` does not include descendants, and transparent backgrounds give fg = bg ratio 1 ([[dogfooding-session-64]] #9)
- [ ] **wrong** `responsive h1 --widths 320,768,1024` on comparis reports width 820 at every width while claiming the geometry is accurate ([[dogfooding-session-64]] #10)
- [x] **wrong** `perf vitals` says Firefox lacks LCP although FF157 supports `largest-contentful-paint` (buffered observer got 71 ms), and pairs `lcp_ms:null` with `lcp_approximate:true` ([[dogfooding-session-64]] #11) (fix/easy-batch-1) — cause: the buffered observer's callback runs on a later task, after the one-shot eval returned; entries are now read with `takeRecords()`, `lcp_source` added, DOM approximation only where `supportedEntryTypes` lacks LCP
- [x] **wrong** `run`: a `wait`/`assert_url` step after a navigating `click` polls the pre-navigation document and times out ([[dogfooding-session-64]] #12) (fix/navigate-truthfulness)
- [x] **wrong** throttled `navigate --with-network` returns after 3.2 s with 44 of ~67 requests and `timeout_reached:false`, not flagged incomplete ([[dogfooding-session-64]] #13) (fix/easy-batch-2: capture continues until readyState complete within `--network-timeout`; otherwise `partial:true` + `timeout_reached:true`)
- [x] **wrong** `console` labels Firefox warnings (entryTypes, cookie, Referrer-Policy, preload) as `level:"error"` ([[dogfooding-session-64]] #14) (fix/easy-batch-1) — pageError `warning`/`info` flags now map to `warn`/`info` (Referrer-Policy "ignoring less restricted" is flagged `info` by Firefox)
- [ ] **wrong** `scroll text 'Further reading'` scrolls to the first TreeWalker hit (sidebar TOC) instead of the heading, with no match count ([[dogfooding-session-64]] #15)
- [ ] **wrong** `a11y summary` lists 43 `banner` landmarks on comparis where ARIA scoping allows 2 ([[dogfooding-session-64]] #16)
- [ ] **wrong** `network --follow` response events lack `status`/`duration_ms`/`transfer_size` for 54 of 168 responses, including the funnel's key POST ([[dogfooding-session-64]] #17)
- [ ] **wrong** `sources` `actor` ids are per-connection (`conn29` → `conn30`), so no later command can use them ([[dogfooding-session-64]] #18)
- [x] **wrong** `reload --wait-idle` reports `status_reason:"not_observed"` alongside `requests_observed:293` ([[dogfooding-session-64]] #19) (fix/easy-batch-2: drain feeds the document-status tracker; updates no longer double-count)
- [ ] **wrong** `--with-page` collects before SPA work settles and, on non-readerable pages, fills its cap with nav links while dropping all form controls ([[dogfooding-session-64]] #20)
- [x] **wrong** `snapshot` `max_chars` counts compact bytes while stdout is pretty-printed (50 KB budget → 352 KB output) ([[dogfooding-session-64]] #21) — fixed: budget measured on the printed JSON
- [ ] **wrong** `click` on a disabled control's label returns `clicked:true` with no warning ([[dogfooding-session-64]] #22)
- [x] **gap** `navigate --auto-consent` / `consent accept` do not handle consentmanager.net (`#cmpbox`, `a.cmpboxbtnyes`) used by comparis ([[dogfooding-session-64]] #23) (fix/easy-batch-1) — any-host native CMP entry `consentmanager`
- [x] **gap** `storage localStorage` has no per-value cap and printed 1.05 MB on Wikipedia ([[dogfooding-session-64]] #24) (fix/easy-batch-2: `--max-value-chars`, default 2048, `truncated_values`)
- [ ] **gap** `navigate --block` leaves no `blocked` marker or count on blocked requests, and `--block ''` is silently dropped ([[dogfooding-session-64]] #25)
- [x] **gap** `cookies` ignores `--fields`, `--limit` and `--sort` and accepts unknown fields with exit 0 ([[dogfooding-session-64]] #26) (fix/easy-batch-2)
- [x] **gap** unknown or stale `--ref` fails as a ~3 s generic selector timeout (exit 124) leaking `[data-ffrdp-ref=…]` instead of an immediate ref-specific error ([[dogfooding-session-64]] #27) — fixed: `stale_ref` before the auto-wait, < 100 ms
- [x] **gap** a plain `click` on a link returns before navigation commits with no signal that one is pending ([[dogfooding-session-64]] #28) (fix/navigate-truthfulness)
- [ ] **wrong** `click --with-page` settles on the transient cross-process `about:blank` (new innerWindowId) and collects the page view from it; `page_view::NavigationOrigin::confirms` lacks the about:blank rule `click`'s own commit check has — share one origin type (#306 review)
- [ ] **wrong** navigate's stale-lifecycle filter never judges a replayed outgoing `dom-loading` and compares Firefox's clock with the local one (50 ms slack), so a remote `--host` with clock skew or a replay that includes `dom-loading` defeats it (#306 review)
- [ ] **gap** `click --ref` on a visually hidden custom radio times out instead of falling back to its `<label>` ([[dogfooding-session-64]] #29)
- [ ] **gap** `inspect` shows getter/setter descriptors but no values for accessor properties (`window.location.href`) ([[dogfooding-session-64]] #30)
- [x] **gap** `snapshot --query` searches only the depth-limited tree and gives no hint when depth caused the miss ([[dogfooding-session-64]] #31) — fixed: `--query` walks the whole document
- [x] **docs** `screenshot --viewport-height` is in `README.md:480` and `--help` but refused at runtime; remove or implement ([[dogfooding-session-64]] #32)
- [x] **docs** shipped `ff-rdp-debug` playbooks K0/C2 (and `evals/fixtures/K0/bug.json`) use nonexistent `network --status`, `snapshot --interactive-only` and `snapshot --filter` ([[dogfooding-session-64]] #33)
- [x] **docs** `kb/reference/script-format.md` still mentions the daemon and documents the removed `--env-file` ([[dogfooding-session-64]] #34)
- [ ] **docs** `--help` contradicts behaviour for `styles`/`a11y summary`/`dom --count` output shapes, `navigate` wait and `--network-timeout`, ref lifetime, `network`, `a11y contrast` and `snapshot --query` ([[dogfooding-session-64]] #35)
- [x] **docs** `launch` occupied-port error suggests `--replace` even when the port owner is foreign, which `--replace` refuses ([[dogfooding-session-64]] #36) (fix/easy-batch-1)
- [ ] **polish** console messages and page text come out German; pin the locale prefs in the launch profile ([[dogfooding-session-64]] #37) — investigated in fix/easy-batch-1: the prefs ARE pinned (`intl.locale.requested=en-US`, `intl.accept_languages=en-US, en` in user.js and prefs.js); the cause is that this machine's Firefox is a German build (`de.lproj`, no en-US resources), so en-US falls back to de. `launch --english-language-pack <en-US.xpi> --restart-after-language-pack-install` with archive.mozilla.org's `157.0/mac/xpi/en-US.xpi` gives English console text ("Ignoring unsupported entryTypes: longtask."). Default fix needs an automatic pack download — a design decision (network fetch at launch), not an easy fix
- [x] **polish** `perf vitals`/`audit` log two unsupported-entryTypes console errors per call, polluting `console --level error` ([[dogfooding-session-64]] #38) (fix/easy-batch-1) — types missing from `supportedEntryTypes` are no longer observed
- [x] **polish** writing to a closed pipe (`snapshot … | head -1`) panics with "failed printing to stdout: Broken pipe" ([[dogfooding-session-64]] #39) (fix/easy-batch-1) — panic hook exits 141 quietly on a closed stdout
- [ ] **polish** invalid selectors in `click` classify as `Timeout`/124 while `dom` says `User`, timeouts exit 124 or 5 by command, and `error_type` casing is mixed ([[dogfooding-session-64]] #40)
- [x] **polish** `snapshot --format text` omits `e<N>` refs ([[dogfooding-session-64]] #41) — fixed: `ref=e…` and `refs_below` lines in text mode
- [ ] **polish** `scroll until` brute-forces an element already in the DOM (79 scrolls, 16 s) and says "not found" on timeout ([[dogfooding-session-64]] #42)
- [ ] **polish** `page-text --query` joins match windows with no gap marker ([[dogfooding-session-64]] #43)
- [x] **polish** `navigate --with-network` duplicates `entries` in `slowest`, emits `total_transfer_bytes` as a float, and reports `status:200` for `data:` URLs ([[dogfooding-session-64]] #44) (fix/easy-batch-2: `slowest` top 5, integer bytes, `data:` → `no_document_request`)
- [x] **polish** home view says "version unknown" while `doctor` reports FF157, and doctor's tested range 120–150 is stale ([[dogfooding-session-64]] #45) (fix/easy-batch-1) — home uses the device-actor fallback; range now 120–157 (the nightly live sweep's Firefox)
- [x] **polish** `profiles prune --all --dry-run` fills `removed_live` instead of a `would_remove` field ([[dogfooding-session-64]] #46) (fix/easy-batch-2: `would_remove_live`)
- [x] **polish** `--version` still prints 0.3.0; bump before tagging ([[dogfooding-session-64]] #47)
- [ ] **polish** `click`'s obscured error always suggests `consent accept`, even for fixed FABs or the target's own label svg ([[dogfooding-session-64]] #48)
- [x] **polish** `type --ref eN V` rejects positional text without pointing to `--text` ([[dogfooding-session-64]] #49) (fix/easy-batch-2)
- [ ] **bug** `run` playbook `click` flaked 1/15 with `committed:false, outcome:"timeout"` after 10 s although the page had navigated (session 64 verification N3)
- [ ] **polish** `run --jq` is ignored when a script executes; it applies only with `--dry-run` (session 64 verification N5)
- [ ] **polish** `run` has no `page_text` verb ("unknown variant") (session 64 verification N5)
- [x] **polish** a never-settling Promise (`eval --timeout 4000 'new Promise(()=>{})'`) exits rc 5, which `eval --help` does not list (session 64 verification N5) (fix/easy-batch-2: documented in `eval --help`; no README exit-code table exists)
- [ ] **polish** `--with-page` on Firefox's error page (`click body --with-page` on `about:certerror`) spends the whole `--timeout` collecting, then reports `page_ready:false` (found in #308)

## Done

- [x] Global skills `create-pr`/`review-pr`/`merge-pr` edited 2026-10-03: gates run once per tree hash, one review pass per PR, no status-flip commit (`ralph-loop` skills left untouched — retired)
- [x] CI skips docs-only PR pushes (#292, 2026-10-03)
- [x] #288 / #289 moved here from GitHub issues (2026-10-03)
