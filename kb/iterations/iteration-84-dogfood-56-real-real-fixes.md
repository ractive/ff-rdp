---
title: "Iteration 84: dogfood-56 real-real fixes — finish iter-83's still-broken themes (cascade, screenshot, navigate default, styles dedupe) + bundled chaos findings (dom-stats parity, stale-tab race, a11y contrast, flag unification, cookies Set-Cookie)"
type: iteration
date: 2026-05-26
status: planned
branch: iter-84/dogfood-56-real-real-fixes
depends_on:
  - iteration-83-dogfood-55-real-fixes
firefox_refs:
  - lines: 1-260
    path: devtools/server/actors/page-style.js
    why: >-
      Theme A — iter-83 claimed the cascade parser was fixed, but
      dogfood-56 shows `cascade 'h1' --prop color` against tennis-sepp.ch
      STILL returns `rules: []` even though `--debug-raw` proves Firefox
      sent `matchedSelectorIndexes: [0]` plus full `authoredText` /
      `declarations`. The post-processing layer (cascade aggregator) is
      dropping rules. Re-verify the `getApplied` wire shape and align
      the parser's filter to keep entries whose `matchedSelectorIndexes`
      is non-empty.
  - lines: 1-25
    path: devtools/server/actors/screenshot.js
    why: >-
      Theme B — iter-83's screenshot shim still errors with "screenshot
      actor not found in Firefox 151 root form". Need an alternative
      discovery path: per-target actor on the WindowGlobalTarget form,
      or a `WindowGlobalTarget.takeScreenshot` equivalent. Capture the
      raw `getRoot` reply on FF 151 to enumerate what IS advertised, and
      route the screenshot call to whatever path actually exists.
  - lines: 1-125
    path: devtools/server/actors/resources/document-event.js
    why: >-
      Theme C — `navigate` (default flags) now waits (improvement) but
      times out at 10s even on example.com because the document-event
      listener never fires AND the readystate fallback exhausts its
      budget. Investigate watcher subscription ordering on cross-origin
      targets and ensure the readystate fallback has a guaranteed slice
      of the budget regardless of how long events waited.
  - lines: 260-520
    path: devtools/server/actors/page-style.js
    why: >-
      Theme E — `styles 'h1' --applied` still emits duplicate rule
      entries on real sites (two `::after, ::before` rows, two `h1`
      rows). Dedupe must key on rule actor ID, not on `(selector,
      property)` pairs that legitimately repeat across stylesheets.
  - lines: 1-200
    path: devtools/server/actors/storage.js
    why: >-
      Theme L — `cookies` against httpbin.org/cookies/set returns empty
      `results: []` even when Firefox processed the `Set-Cookie`
      response header. StorageActor doesn't always surface server-set
      cookies that haven't been persisted to disk; need to also read
      the `Set-Cookie` response headers via the network actor and
      merge.
kb_refs:
  - kb/rdp/actors/page-style.md
  - kb/rdp/actors/screenshot.md
  - kb/rdp/actors/watcher.md
  - kb/rdp/actors/storage.md
  - kb/rdp/actors/console.md
  - kb/dogfooding/dogfooding-session-56.md
first_call_sites:
  - primitive: "cascade aggregator reads matchedSelectorIndexes (Theme A)"
    site: "crates/ff-rdp-core/src/actors/page_style.rs"
  - primitive: "CLI cascade rendering (Theme A)"
    site: "crates/ff-rdp-cli/src/commands/cascade.rs"
  - primitive: "screenshot per-target probe (Theme B)"
    site: "crates/ff-rdp-core/src/actors/screenshot.rs"
  - primitive: "CLI screenshot fallback wiring (Theme B)"
    site: "crates/ff-rdp-cli/src/commands/screenshot.rs"
  - primitive: "navigate default budget split (Theme C)"
    site: "crates/ff-rdp-cli/src/commands/navigate.rs"
  - primitive: "styles --applied dedupe by rule actor id (Theme E)"
    site: "crates/ff-rdp-cli/src/commands/styles.rs"
  - primitive: "shared element-count source for dom stats + perf audit (Theme H)"
    site: "crates/ff-rdp-cli/src/commands/dom.rs"
  - primitive: "tab handle invalidation on navigate (Theme I)"
    site: "crates/ff-rdp-core/src/client.rs"
  - primitive: "a11y contrast scanner pair logic (Theme J)"
    site: "crates/ff-rdp-cli/src/commands/a11y_contrast.rs"
  - primitive: "wait --timeout-ms canonical with --timeout alias (Theme K)"
    site: "crates/ff-rdp-cli/src/commands/wait.rs"
  - primitive: "cookies merges Set-Cookie response headers (Theme L)"
    site: "crates/ff-rdp-cli/src/commands/cookies.rs"
dogfood_path: |
  # === MANDATORY: rebuild and locally test every theme before ticking ACs ===
  # This is the THIRD attempt at the iter-82/iter-83 carry-over themes.
  # Iter-83 ticked 7/7 ACs but dogfood-56 showed 4 of 7 were lies. This
  # iteration MUST verify each theme by building the binary and running
  # these exact commands against a live headless Firefox.
  #
  # Test sites used below:
  #   - https://tennis-sepp.ch                  (real external CSS, multiple stylesheets)
  #   - https://example.com                     (trivial cross-origin baseline)
  #   - https://news.ycombinator.com            (cross-origin SPA-ish)
  #   - https://www.w3.org/WAI/demos/bad/before/home.html  (intentional a11y violations)
  #   - https://httpbin.org/cookies/set?foo=bar (Set-Cookie via response header)
  cargo install --path crates/ff-rdp-cli --offline
  ff-rdp -V                                                       # must show git-sha
  ff-rdp launch --headless --auto-consent

  # --- Theme A: cascade returns real rules on a REAL site (iter-83 regression) ---
  ff-rdp navigate https://tennis-sepp.ch
  ff-rdp cascade 'h1' --prop color --jq '.results.rules | length'  # MUST be ≥ 1 (was 0 post iter-83)
  ff-rdp cascade 'h1' --prop color --jq '.results.rules[0].matched_selectors'  # MUST contain "h1"
  ff-rdp cascade 'h1' --prop color --debug-raw | head -40         # raw reply still non-empty
  # Cross-site verification (dogfood-56 confirmed cascade broken EVERYWHERE):
  ff-rdp navigate https://dequeuniversity.com/demo/mars/
  ff-rdp cascade 'h1' --prop color --jq '.results.rules | length'  # MUST be ≥ 1

  # --- Theme B: screenshot actually writes a PNG on FF 151 ---
  ff-rdp navigate https://example.com
  ff-rdp screenshot -o /tmp/iter-84.png
  test -s /tmp/iter-84.png
  file /tmp/iter-84.png | grep -i 'PNG image'                     # must be a real PNG
  ff-rdp screenshot --full-page -o /tmp/iter-84-full.png
  test -s /tmp/iter-84-full.png

  # --- Theme C: default `navigate` completes within budget on a trivial page ---
  # Pre-iter-84: `navigate https://example.com` (no flags) times out at 10032ms.
  # Post-iter-84: default returns within 3000ms on example.com.
  time ff-rdp navigate https://example.com                        # MUST complete <3s
  time ff-rdp navigate https://news.ycombinator.com               # MUST complete <10s
  time ff-rdp navigate https://tennis-sepp.ch                     # MUST complete <10s
  # The readystate fallback MUST run when events stall:
  ff-rdp navigate https://example.com --debug-events 2>&1 | grep -i 'readystate'  # fallback triggered

  # --- Theme E: styles --applied no duplicate rule entries ---
  ff-rdp navigate https://tennis-sepp.ch
  ff-rdp styles 'h1' --applied --jq '[.results[].rule_actor_id] | length as $n | unique | length as $u | $n == $u'
  # MUST be `true` (no duplicate rule_actor_id values)
  ff-rdp styles 'body' --applied --jq '.results | length'         # MUST be ≥ 1

  # --- Theme H: dom-stats vs perf-audit element-count agreement ---
  ff-rdp navigate https://www.w3.org/WAI/demos/bad/before/home.html
  DS=$(ff-rdp dom stats --jq '.results.images_without_lazy')
  PA=$(ff-rdp perf audit --jq '.results.dom_stats.images_without_lazy')
  test "$DS" = "$PA"                                              # MUST be equal

  # --- Theme I: stale-tab race — first call after navigate hits the new tab ---
  ff-rdp navigate https://news.ycombinator.com
  ff-rdp navigate https://example.com
  # First call after navigate MUST report example.com, not HN:
  ff-rdp snapshot --jq '.results.url' | grep -i 'example.com'
  ff-rdp dom stats --jq '.results.url' | grep -i 'example.com'

  # --- Theme J: a11y contrast on intentionally-bad WAI demo ---
  ff-rdp navigate https://www.w3.org/WAI/demos/bad/before/home.html
  ff-rdp a11y contrast --fail-only --jq '.results.aa_fail'        # MUST be ≥ 1

  # --- Theme K: flag unification — wait accepts --timeout-ms ---
  ff-rdp navigate https://the-internet.herokuapp.com/dynamic_loading/1
  ff-rdp click '#start button'
  ff-rdp wait --selector '#finish h4' --timeout-ms 8000           # MUST work (canonical name)
  ff-rdp wait --selector '#finish h4' --timeout 8000 2>&1 | grep -i 'deprecat'  # alias warns

  # --- Theme L: cookies surfaces Set-Cookie response headers ---
  ff-rdp navigate 'https://httpbin.org/cookies/set?session=abc123'
  ff-rdp cookies --jq '[.results[].name] | contains(["session"])'  # MUST be true

  # --- AC verification gate ---
  # All AC live tests must pass:
  FF_RDP_LIVE_TESTS=1 cargo test --test live_cascade_real_site -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_screenshot_ff151 -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_navigate_default_fast -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_styles_applied_dedupe -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_dom_stats_perf_audit_parity -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_stale_tab_race -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_a11y_contrast_wai_bad -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_wait_timeout_ms_canonical -- --include-ignored
  FF_RDP_LIVE_TESTS=1 cargo test --test live_cookies_set_cookie_header -- --include-ignored
  pkill -f 'firefox.*ff-rdp-profile'
tags:
  - iteration
  - bugfix
  - dogfood
  - testing-discipline
  - cascade
  - screenshot
  - navigate
  - styles
  - a11y
  - cookies
---

# Iteration 84 — finish iter-83's still-broken themes + bundled chaos findings

iter-83 ticked 7/7 ACs as "backed by diff evidence", but dogfood-56
showed that only 3 of 7 themes (D, F, G) actually work on real sites.
Themes A (cascade), B (screenshot), C (navigate default) and E (styles
dedupe) are still broken — and the dogfooding session surfaced 5 more
quality issues worth bundling into this pass. See
[[dogfooding-session-56]] for the full evidence list.

This is the THIRD attempt at the carry-over themes from iter-82 →
iter-83 → iter-84. The pattern is: AC tests are written against
oversimplified fixtures or proxy signals ("raw reply is non-empty",
"slug appears in diff") that don't exercise the user-visible command
path. This iteration tightens the verification gate to require
**command-output evidence**, not actor-reply evidence.

## Hard rule (this iteration only): do not tick an AC checkbox until

Because this is the THIRD attempt at the same set of bug fixes, the
verification gate is even tighter than iter-83's:

1. The binary has been rebuilt locally from this branch (`cargo install
   --path crates/ff-rdp-cli --offline`) AND
2. The relevant command in the `dogfood_path` above has been executed
   manually against a live headless Firefox and produced the documented
   post-condition **as seen on stdout** (the user-visible output, NOT
   the raw actor reply), AND
3. The corresponding live AC test has been run with `FF_RDP_LIVE_TESTS=1`
   and returns `test result: ok. … 0 failed`, AND
4. The AC's named test must assert against the **CLI command output**
   (parsed JSON results object), not against the underlying actor's raw
   reply. "`getApplied` returned `matchedSelectorIndexes`" is NOT
   evidence that `ff-rdp cascade` parses it.

If a step fails, FIX THE CODE, rebuild, and retry — do not move on.
"Ticked because the test slug appears in the diff" is not verification
for this iteration. "Ticked because the raw reply has the data" is also
not verification for this iteration.

If iter-84 also ships fake ACs, the next iteration will require a
**video recording of each command run** attached to the PR description.

This rule is **scoped to iter-84** — it's not a project-wide discipline
change. Future feature iterations don't have a known-failing baseline
to verify against, so they keep the existing AC fidelity rule.

## Why the iter-83 ACs didn't catch the bugs

iter-83 tightened the gate (build binary + run live test + check
stdout), but still passed because its evidence was the wrong layer:

| Theme | iter-83 AC test | Why dogfood-56 still found it broken |
|-------|-----------------|---------------------------------------|
| A — cascade | `live_cascade_returns_matched_rules_external_css` | Used a fixture page where the parser happened to work; live test asserts against the raw actor reply (`matchedSelectorIndexes` present), not the CLI output (`rules: []`). The CLI's aggregator drops what the parser keeps. |
| B — screenshot | `live_screenshot_no_args_on_firefox_151` | Likely silently skipped: `FF_RDP_LIVE_TESTS=1` gate plus a pre-navigate failure path means the assertion never runs. CI passes; the user sees "screenshot actor not found in Firefox 151 root form". |
| C — navigate default | `live_navigate_default_completes_within_timeout` | Test fixture uses an in-process HTTP server with sub-100ms response; real cross-origin DNS+TLS+events delays don't surface. example.com still times out at 10s for real users. |
| E — styles dedupe | `live_styles_applied_returns_real_rules` | Asserts ≥ 2 rules survive (correct on the fixture), but doesn't assert **uniqueness** of rule actor IDs. Real sites with multiple stylesheets produce duplicates that the test never sees. |

Common pattern, sharper than iter-83's: **AC evidence must be the
user-visible command output, not the underlying actor reply or a
proxy signal.** "The raw reply has the data" ≠ "ff-rdp parses the data".
"At least 2 rules" ≠ "no duplicates". "The slug is in the diff" ≠
"the test ran green".

## Tasks

### Theme A — cascade aggregator parses `matchedSelectorIndexes` from real replies
- [ ] Run `ff-rdp cascade 'h1' --prop color --debug-raw` against
      tennis-sepp.ch AND dequeuniversity.com/demo/mars and capture both
      raw replies. Diff against the aggregator's field expectations in
      `crates/ff-rdp-core/src/actors/page_style.rs` (or wherever the
      CLI command in `crates/ff-rdp-cli/src/commands/cascade.rs`
      consumes the parser output).
- [ ] Identify the exact filter step that drops entries even when
      `matchedSelectorIndexes` is non-empty. Likely culprits: a guard
      that requires `entries[].selector` to be populated, or a dedupe
      step that treats every entry's empty `properties` as redundant.
- [ ] Fix the aggregator to keep entries whose `matchedSelectorIndexes`
      is non-empty AND whose `authoredText`/`declarations` carry the
      property data, regardless of whether `selector` is pre-resolved.
- [ ] **AC test must assert on `.results.rules[].matched_selectors`** as
      seen in `ff-rdp cascade` stdout, not on the raw reply.

### Theme B — screenshot actor probe works on FF 151
- [ ] Reproduce on FF 151: `ff-rdp screenshot -o /tmp/x.png` errors
      with `screenshot actor not found in Firefox 151 root form`.
      Dump the raw `getRoot` reply (and the per-target form) to see
      what IS advertised.
- [ ] If the screenshot actor moved to the per-target form on FF 151+,
      route the request to the WindowGlobalTarget; if it's gone
      entirely, fall back to `WindowGlobalTarget.takeScreenshot` or the
      equivalent target-level capture method.
- [ ] **AC test must assert the PNG file exists, is non-empty, and
      starts with the PNG magic bytes** — not just that the actor probe
      succeeded.

### Theme C — default `navigate` completes within budget on a trivial page
- [ ] Reproduce: `time ff-rdp navigate https://example.com` (no flags)
      hits 10032ms today. Determine why the document-event listener
      doesn't fire on a trivial cross-origin page — watcher
      subscription timing, target-attach ordering, or resource-replay
      window.
- [ ] Fix the budget split: the readystate fallback must have a
      guaranteed slice of the timeout (e.g. reserve 30% / start
      concurrently and return on first signal) so iter-83's "no
      remaining budget for readystate fallback" error becomes
      impossible.
- [ ] **AC test must use `time` measurements on `ff-rdp navigate
      https://example.com` (no flags) against a real cross-origin
      target**, not an in-process HTTP server. Asserts wall clock
      < 3000ms.

### Theme E — styles `--applied` dedupe by rule actor id
- [ ] Reproduce on tennis-sepp.ch: `ff-rdp styles 'h1' --applied`
      shows two `::after, ::before` rows (same column 26798) and two
      `h1` rows (column 28837 + 5245). Capture the raw reply and
      identify the duplicate keys.
- [ ] Change the dedupe key from `(selector, property)` to **rule
      actor ID** (the rule's stable RDP `actor` string). Drop later
      occurrences of the same rule actor.
- [ ] **AC test must compute `[rule_actor_id] | unique | length` vs
      `length` on the live output and assert equality** — i.e. zero
      duplicates by rule actor.

### Theme H — `dom stats` and `perf audit`'s embedded `dom_stats` agree
- [ ] Run both commands on the WAI bad demo and capture the disagreement
      (`images_without_lazy: 9` vs `42`).
- [ ] Pick one definition (`document.images` is the user-facing one)
      and make `perf audit` call into the same shared counter that
      `dom stats` uses, in `crates/ff-rdp-cli/src/commands/dom.rs`.
- [ ] **AC test asserts both commands return the same number for the
      same page in the same load.**

### Theme I — tab handle invalidation on navigate (stale-tab race)
- [ ] Reproduce: `navigate A; navigate B; snapshot` returns A's DOM
      on the first call after the second navigate.
- [ ] On every `navigate` complete, invalidate cached tab/document
      actor handles in `crates/ff-rdp-core/src/client.rs` so the next
      command refetches them. (Or: every command that takes a tab
      handle must verify the URL matches the most recent navigate
      target before returning data.)
- [ ] Also flush the `consoleActor` cache on navigate (covers the
      `noSuchActor` on first click reported by dogfood-56).
- [ ] **AC test does two back-to-back navigates and asserts the first
      `snapshot` after the second navigate reports the second URL.**

### Theme J — a11y contrast scanner reports violations on WAI bad demo
- [ ] Reproduce: `ff-rdp a11y contrast --fail-only` on
      `https://www.w3.org/WAI/demos/bad/before/home.html` returns
      `total: 0`. Page is famously full of contrast issues.
- [ ] Audit the fg/bg pairing logic in
      `crates/ff-rdp-cli/src/commands/a11y_contrast.rs`: likely it
      only samples computed-style on a narrow subset (e.g. text nodes
      with explicit `color`), missing inherited colors / nested
      elements.
- [ ] Walk all visible text nodes, resolve effective `color` and
      ancestor `background-color` (or computed background via
      `getComputedStyle`), and compute WCAG contrast for each pair.
- [ ] **AC test asserts `aa_fail >= 1` on the WAI bad demo.**

### Theme K — flag unification: `wait --timeout-ms` canonical, `--timeout` deprecated alias
- [ ] Rename `wait`'s `--timeout` flag to `--timeout-ms` to match
      `navigate` and other commands.
- [ ] Keep `--timeout` as a hidden deprecated alias that emits a
      stderr warning on use.
- [ ] Run a quick audit across all commands and capture any other
      `--timeout` vs `--timeout-ms` inconsistency in a follow-up note
      (no fix this iter unless trivial).
- [ ] **AC test runs `wait --selector X --timeout-ms 8000` (canonical)
      and `wait --selector X --timeout 8000` (alias, expects stderr
      deprecation warning).**

### Theme L — cookies command surfaces `Set-Cookie` response headers
- [ ] Reproduce: `navigate 'https://httpbin.org/cookies/set?session=abc123'`
      followed by `ff-rdp cookies` returns `results: []` even though
      the response Set-Cookie should have stuck.
- [ ] In `crates/ff-rdp-cli/src/commands/cookies.rs`, after querying
      StorageActor and `document.cookie`, also pull `Set-Cookie`
      headers from the network actor's recorded response headers for
      the current document load and merge them in (deduped by
      `(name, domain, path)`).
- [ ] **AC test asserts `[.results[].name] | contains(["session"])`
      after navigating to a `Set-Cookie`-bearing URL.**

## Acceptance Criteria [0/9]

Each AC below MUST be verified by running its named test with
`FF_RDP_LIVE_TESTS=1 cargo test --test <name> -- --include-ignored`
AND by executing the corresponding `dogfood_path` command on a
locally-built binary AND by asserting the post-condition on the
**CLI command's user-visible stdout**, before being ticked.

- [ ] `live_cascade_real_site`
      (NEW, crates/ff-rdp-cli/tests/live_cascade_real_site.rs):
      navigates to a fixture page that uses an external
      `<link rel="stylesheet">` with `h1 { color: red }`, runs
      `ff-rdp cascade 'h1' --prop color`, asserts the parsed CLI
      output has `results.rules | length >= 1` AND
      `results.rules[0].matched_selectors` contains `"h1"`. Gated
      `FF_RDP_LIVE_TESTS=1`.
- [ ] `live_screenshot_ff151`
      (UPDATED, crates/ff-rdp-cli/tests/live_screenshot_shim.rs or
      new crates/ff-rdp-cli/tests/live_screenshot_ff151.rs):
      pre-stabilises navigate with default flags, calls
      `ff-rdp screenshot -o $tmp.png`, asserts the file exists, is
      non-empty, and the first 8 bytes match the PNG magic
      `\x89PNG\r\n\x1a\n`. Must run green without panicking in setup.
- [ ] `live_navigate_default_fast`
      (NEW, crates/ff-rdp-cli/tests/live_navigate_default_fast.rs):
      runs `ff-rdp navigate https://example.com` with NO flags
      against a real cross-origin target (or a local server with
      realistic event-replay timing), measures wall clock, asserts
      duration < 3000ms AND exit code 0.
- [ ] `live_styles_applied_dedupe`
      (NEW, crates/ff-rdp-cli/tests/live_styles_applied_dedupe.rs):
      on a fixture page with multiple stylesheets that each define
      `h1` rules, runs `ff-rdp styles 'h1' --applied`, asserts
      `[.results[].rule_actor_id] | unique | length` equals
      `.results | length` (zero duplicates by rule actor id) AND
      `.results | length >= 2`.
- [ ] `live_dom_stats_perf_audit_parity`
      (NEW, crates/ff-rdp-cli/tests/live_dom_stats_perf_audit_parity.rs):
      on a fixture page with a known image count, runs
      `ff-rdp dom stats` and `ff-rdp perf audit`, asserts both report
      the same `images_without_lazy` value.
- [ ] `live_stale_tab_race`
      (NEW, crates/ff-rdp-cli/tests/live_stale_tab_race.rs):
      navigates to page A, then page B, then immediately runs
      `ff-rdp snapshot`, asserts `results.url` matches page B
      (no retry, first call must be correct).
- [ ] `live_a11y_contrast_wai_bad`
      (NEW, crates/ff-rdp-cli/tests/live_a11y_contrast_wai_bad.rs):
      on a local fixture mirroring the WAI bad-contrast demo
      (low-contrast text on similar background), runs
      `ff-rdp a11y contrast --fail-only`, asserts
      `results.aa_fail >= 1`.
- [ ] `live_wait_timeout_ms_canonical`
      (NEW, crates/ff-rdp-cli/tests/live_wait_timeout_ms_canonical.rs):
      runs `ff-rdp wait --selector body --timeout-ms 5000` (asserts
      exit 0, no deprecation warning on stderr) AND
      `ff-rdp wait --selector body --timeout 5000` (asserts exit 0
      AND stderr contains the word "deprecat").
- [ ] `live_cookies_set_cookie_header`
      (NEW, crates/ff-rdp-cli/tests/live_cookies_set_cookie_header.rs):
      navigates to a fixture URL whose response includes
      `Set-Cookie: session=abc123`, runs `ff-rdp cookies`, asserts
      `[.results[].name] | contains(["session"])`.

## Out of scope

- Fix #3 (consoleActor cache survives navigation) is addressed as a
  side-effect of Theme I's invalidation pass; if it persists, file
  iter-84b.
- Fix #5 (opaque "0ms timeout" error after `data:` URL block) and
  #7 (network timing all-zeros after daemon hiccup) are deferred —
  both appear to be symptoms of a wedged daemon-state path that needs
  its own investigation. Track as iter-84c.
- Full root-cause fix for the `events`-strategy missed `dom-complete`
  on cross-origin navigates: Theme C ships a working default via a
  fixed budget split + concurrent readystate, but the underlying
  watcher-subscription race may still warrant its own iteration.
- Project-wide `--timeout` vs `--timeout-ms` audit beyond `wait`: only
  the `wait` command is renamed this iter; broader flag taxonomy is
  out of scope (file a `cargo xtask check-flag-naming` lint as a
  separate plan if useful).

## References

- [[dogfooding-session-56]] — bug list and the "iter-83 ACs still
  fake" finding
- [[iteration-83-dogfood-55-real-fixes]] — the iteration this one
  finishes (second attempt)
- [[iteration-82-dogfood-54-fixes]] — the iteration iter-83 was
  supposed to finish (first attempt)
- [[dogfooding-session-55]] — original "AC ticked without run"
  finding that motivated iter-83's hard rule
