---
title: "Iteration 85: dogfood-57 carry-overs (cascade type sentinel, screenshot WindowGlobalTarget, cookies network-merge, navigate budget) + runnable dogfood_script gate"
type: iteration
date: 2026-05-27
status: planned
branch: iter-85/dogfood-57-carryovers-and-runnable-dogfood-path
depends_on:
  - iteration-84-dogfood-56-real-real-fixes
firefox_refs:
  - lines: 260-520
    path: devtools/server/actors/page-style.js
    why: >-
      Theme A — dogfood-57 found that iter-84's cascade parser accepts
      `rule.type` absent or `== 1` (LegacyRule sentinel), but Firefox 151
      sends `type: 100` with `className: "CSSStyleRule"` for ordinary
      author rules. Re-confirm the type enum and accept the modern
      `CSSStyleRule` constant (or drop the filter and rely on
      `matchedSelectorIndexes` non-empty as the discriminator).
  - lines: 1-160
    path: devtools/server/actors/screenshot.js
    why: >-
      Theme B — iter-84 added per-target probing scaffolding but did NOT
      route the actual capture call through `WindowGlobalTarget` /
      `BrowsingContextTarget`. dogfood-57 confirms the same root-form
      error message. Land the call path: when the root-form `screenshot`
      actor is absent, send `getCurrentTabActor` then issue the capture
      request against `tabActor.screenshot` (or the FF 151 equivalent).
  - lines: 1-200
    path: devtools/server/actors/storage.js
    why: >-
      Theme L — iter-84 shipped a 250 ms StorageActor retry but never
      reached for the network actor. dogfood-57 confirms cookies set via
      `Set-Cookie` response header against httpbin.org/cookies/set still
      return `[]`. Land the network-actor side: subscribe to
      `responseHeaders` resources during navigate, extract `Set-Cookie`
      values, parse name/value/domain/path/expires, and merge with the
      StorageActor reply (StorageActor wins on conflict).
  - lines: 1-125
    path: devtools/server/actors/resources/document-event.js
    why: >-
      Theme C — iter-84 stopped the 10 s timeout but example.com still
      takes ~7.2 s end-to-end (AC said < 3000 ms). Profile which
      event/fallback is dominating the budget — likely the readystate
      fallback fires unconditionally even when `dom-interactive` already
      arrived. Make the fallback conditional on the event-path actually
      timing out, not on event-path absence.
kb_refs:
  - kb/rdp/actors/page-style.md
  - kb/rdp/actors/screenshot.md
  - kb/rdp/actors/storage.md
  - kb/rdp/actors/watcher.md
  - kb/dogfooding/dogfooding-session-57.md
  - kb/iterations/iteration-84-dogfood-56-real-real-fixes.md
first_call_sites:
  - primitive: "cascade parser accepts CSSStyleRule type sentinel (Theme A)"
    site: "crates/ff-rdp-core/src/actors/page_style.rs"
  - primitive: "screenshot capture routed through WindowGlobalTarget (Theme B)"
    site: "crates/ff-rdp-core/src/actors/screenshot.rs"
  - primitive: "cookies merge from network-actor Set-Cookie headers (Theme L)"
    site: "crates/ff-rdp-core/src/actors/storage.rs"
  - primitive: "navigate readystate fallback gated on event-path timeout (Theme C)"
    site: "crates/ff-rdp-cli/src/commands/navigate.rs"
  - primitive: "wait --timeout deprecation warning (Theme K-followup)"
    site: "crates/ff-rdp-cli/src/commands/wait.rs"
  - primitive: "xtask check-dogfood-script: extract+exec sibling .dogfood.sh"
    site: "crates/xtask/src/check_dogfood_script.rs"
  - primitive: "iteration plan schema accepts `dogfood_script:` pointer"
    site: "crates/xtask/src/iteration_plan.rs"
dogfood_script: iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.dogfood.sh
tags:
  - iteration
  - bugfix
  - dogfood
  - testing-discipline
  - cascade
  - screenshot
  - cookies
  - navigate
  - meta-gate
---

# Iteration 85 — fix the carry-overs, make the dogfood-path actually run

dogfood-57 verified iter-84 against the real sites and the verdict was
4 fixed / 2 partial / 3 still-broken. The bigger finding was meta:
iter-84's `dogfood_path` (the prose block in YAML frontmatter that's
supposed to prove the author actually used the fix) was wrong in
three places — wrong jq paths, a `--debug-events` flag that doesn't
exist on `navigate`. Direct evidence the dogfood_path was never
executed before ACs were ticked. The mechanical gates
(`check-dead-primitives`, `ac-fidelity-check`, `claims-vs-code`) all
passed because they could only catch *symbol-level* lies, not
*semantic* ones.

This iteration does two things:

1. **Theme A/B/C/L carry-overs:** the three still-broken themes (plus
   the partial navigate-budget regression) get landed for real, with
   live tests that actually exercise the user-visible command path —
   not the actor reply.
2. **Theme M (meta-gate):** convert `dogfood_path` from a YAML prose
   blob into a sibling `.dogfood.sh` script that
   `check-iteration-ready` actually executes. After iter-85 merges,
   no future iteration can claim a user-visible bug fix without the
   gate having reproduced the user-visible reproduction.

## Hard rule (this iteration only): the dogfood_script gate is the closing test

Do not tick an AC checkbox until the entire
`iteration-85-….dogfood.sh` exits 0 on the author's machine against a
live headless Firefox. The script's last line writes
`/tmp/ff-rdp-iter-85-dogfood-ok` — `check-iteration-ready` greps for
that sentinel and refuses to pass without it.

## Why iter-84's gate didn't catch the bugs

- `dogfood_path` was a YAML literal block. Authors edited it like prose,
  not like code. Three jq paths and one CLI flag drifted from reality
  without anyone noticing because nothing executed them.
- The AC live tests (`live_cascade_real_site` etc.) asserted on the
  actor reply being non-empty, not on `ff-rdp cascade` CLI output
  having non-empty `rules[]`. A successful actor handshake with a
  wrong parser still gives an empty `rules[]` — and the test still
  passes.

iter-85 fixes both ends: AC live tests assert on **CLI stdout JSON**,
and the dogfood_script is run by the harness, not by hand.

## Tasks

### Theme A — cascade accepts the `CSSStyleRule` type sentinel [0/4]

- [ ] Capture `--debug-raw` JSON from `cascade 'h1' --prop color` on
      tennis-sepp.ch and check it into `tests/fixtures/cascade_real_site.json`.
- [ ] In `parse_applied_entry`, accept entries where
      `rule.type == 100` (`CSSStyleRule`) OR `rule.className == "CSSStyleRule"`
      OR `matchedSelectorIndexes` is non-empty. Add unit test with the
      checked-in fixture.
- [ ] Live test `live_cascade_real_site_cli`: spawns `ff-rdp cascade
      'h1' --prop color` as a subprocess on tennis-sepp.ch, asserts the
      stdout JSON has `.results[0].rules | length >= 1`. NOT actor-reply
      based.
- [ ] dogfood_script Theme A block exits 0.

### Theme B — screenshot routes through WindowGlobalTarget on FF 151 [0/4]

- [ ] Capture `getRoot` reply on FF 151 (no `screenshotActor` field).
      Add fixture `tests/fixtures/getroot_ff151.json`.
- [ ] Implement `screenshot_via_target()`: `getTab` → tabActor →
      send `takeScreenshot` request against the target actor. Fall back
      to root-form only if target path also fails.
- [ ] Live test `live_screenshot_ff151_cli`: runs `ff-rdp screenshot -o
      /tmp/x.png` against example.com on FF 151, asserts file exists and
      `file /tmp/x.png` reports a valid PNG of non-zero height.
- [ ] dogfood_script Theme B block exits 0.

### Theme C — navigate default meets <3 s budget on example.com [0/3]

- [ ] Profile `navigate https://example.com --debug-trace` to find the
      dominating segment. Likely: readystate fallback fires
      unconditionally even after `dom-interactive` arrived.
- [ ] Gate the readystate fallback on event-path *timeout*, not
      event-path *absence*. Drop unconditional ≥4 s sleep that
      dogfood-57 observed.
- [ ] dogfood_script Theme C block: `time` reports < 3000 ms on
      example.com. (AC test asserts the time bound, not just exit code.)

### Theme L — cookies merges Set-Cookie response headers via network actor [0/4]

- [ ] Subscribe to `responseHeaders` resource type during navigate
      (alongside `documentEvent`). Buffer the latest per-URL.
- [ ] In `cookies` command: extract `Set-Cookie` lines from buffered
      headers, parse with `cookie` crate, normalize to the same shape
      as StorageActor cookies, merge (StorageActor wins on key match).
- [ ] Live test `live_cookies_set_cookie_cli`: navigates to
      `https://httpbin.org/cookies/set?session=abc123`, runs `ff-rdp
      cookies`, asserts stdout JSON `.results[] | select(.name=="session")`
      is present.
- [ ] dogfood_script Theme L block exits 0.

### Theme K-followup — `wait --timeout` alias emits deprecation [0/2]

- [ ] Print deprecation warning to stderr (not stdout) when `--timeout`
      alias is used. Tag with `(deprecated, use --timeout-ms)`.
- [ ] dogfood_script Theme K block: `--timeout` alias stderr contains
      "deprecat".

### Theme M — runnable dogfood_script gate (meta) [0/6]

- [ ] Schema: `iteration_plan.rs` accepts `dogfood_script: <filename>`
      (sibling file, relative to plan). Either `dogfood_path` OR
      `dogfood_script` allowed; warn if both present.
- [ ] `xtask check-dogfood-script <plan>`: resolves the sibling
      script, refuses to run if not executable, runs with
      `bash -euo pipefail`, fails the gate on non-zero exit OR if the
      `/tmp/ff-rdp-iter-<N>-dogfood-ok` sentinel is absent on success.
- [ ] Wire into `check-iteration-ready` as the final sub-check
      (after ac-fidelity).
- [ ] CI: add the gate as a required check, gated on
      `FF_RDP_LIVE_TESTS=1` (so PR CI without a live Firefox skips
      cleanly with a warning, but iteration branches run it).
- [ ] Document the pattern in `CONTRIBUTING.md` under "Iteration
      discipline". Mention: scripts get the same shellcheck CI as any
      other `.sh` in the repo.
- [ ] Update `iter-84` retrospectively: leave its `dogfood_path`
      block as a tombstone with a top-line comment "this was never
      executed pre-merge — see iter-85 for the fix".

## Acceptance Criteria [0/12]

- [ ] live_cascade_real_site_cli: `ff-rdp cascade 'h1' --prop color` on
      tennis-sepp.ch returns stdout JSON with `.results[0].rules | length >= 1`
- [ ] live_screenshot_ff151_cli: `ff-rdp screenshot -o /tmp/x.png` on
      example.com (FF 151) writes a valid PNG of height > 0
- [ ] live_navigate_default_under_3s: `time ff-rdp navigate
      https://example.com` reports real time < 3000 ms
- [ ] live_cookies_set_cookie_cli: `ff-rdp cookies` after navigate to
      `httpbin.org/cookies/set?session=abc123` contains the `session` cookie
- [ ] live_wait_timeout_alias_deprecates: `ff-rdp wait --selector x
      --timeout 1000` stderr contains "deprecat"
- [ ] unit_cascade_accepts_csss_type_100: fixture-based parser test passes
- [ ] unit_cookies_setcookie_merge: parser test for Set-Cookie → cookie
      shape, with StorageActor-wins merge precedence
- [ ] xtask_check_dogfood_script_smoke: runs the sibling script,
      asserts exit 0 + sentinel file. Negative test: missing sentinel
      → check fails.
- [ ] xtask_check_iteration_ready_calls_dogfood_script: integration
      test asserts the new sub-check is invoked
- [ ] dogfood_script_full_run_iter_85: the sibling `.dogfood.sh` exits
      0 against a live FF 151 with the merged code (this is the
      closing gate — see Hard rule)
- [ ] ci_dogfood_script_required: GitHub Actions job
      `check-dogfood-script` is a required status check on iter-* branches
- [ ] kb_dogfooding_58: a follow-up dogfooding session (#58) verifies
      iter-85's claims with a fresh manual pass; report linked from
      iter-85 status=done commit

## Out of scope

- The remaining iter-84 partial (Theme K deprecation channel) beyond
  the warning itself — full migration to a new flag-deprecation
  framework can come later.
- Retroactively running the new gate against historical iterations.
- Replacing `dogfood_path` everywhere; iter-85 just makes the new
  pattern available and required for new iterations.

## References

- [[dogfooding-session-57]] — the verification that found these bugs
- [[iteration-84-dogfood-56-real-real-fixes]] — what was supposed to fix this
- [[iteration-83-dogfood-55-real-fixes]] — the original attempt
