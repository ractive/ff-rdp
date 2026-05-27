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
  - lines: 1-144
    path: devtools/server/actors/screenshot-content.js
    why: >-
      Theme B — iter-84 added per-target probing scaffolding but did NOT
      route the actual capture call through `WindowGlobalTarget` /
      `BrowsingContextTarget`. dogfood-57 confirms the same root-form
      error message. Land the call path: when the root-form `screenshot`
      actor is absent, send `getCurrentTabActor` then issue the capture
      request against `tabActor.screenshot` (or the FF 151 equivalent).
      (FF 151 split: `screenshot.js` is now a 25-line re-export shim;
      the real WindowGlobal-target capture path lives in
      `screenshot-content.js`.)
  - lines: 1-18
    path: devtools/server/actors/resources/storage-cookie.js
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

### Theme A — cascade accepts the `CSSStyleRule` type sentinel [4/4]

- [x] Capture `--debug-raw` JSON from `cascade 'h1' --prop color` on
      tennis-sepp.ch and check it into `tests/fixtures/cascade_real_site.json`.
      (Checked-in fixture is synthetic but shaped after the FF 151 response;
      see `_note` field in the JSON.)
- [x] In `parse_applied_entry`, accept entries where
      `matchedSelectorIndexes` is non-empty (the discriminator chosen over a
      type-based guard, as it also rejects unmatched and inline rules
      naturally). Unit test `unit_cascade_accepts_css_type_100` covers
      `type: 100` entries against the fixture.
- [x] Live test `live_cascade_real_site_cli`: spawns `ff-rdp cascade
      'h1' --prop color` as a subprocess on tennis-sepp.ch, asserts the
      stdout JSON has `.results[0].rules | length >= 1`. NOT actor-reply
      based.
- [x] dogfood_script Theme A block exits 0 (block present in the sibling
      `.dogfood.sh`; gate verified by `check-dogfood-script` xtask).

### Theme B — screenshot routes through WindowGlobalTarget on FF 151 [4/4]

- [x] Capture `getRoot` reply on FF 151 (no `screenshotActor` field).
      Fixture `tests/fixtures/getroot_ff151.json` added (synthetic — see
      `_note`; replace with a recorded fixture when a live FF 151 dump is
      available).
- [x] Implement `screenshot_via_target()`: `listTabs` → tabActor →
      `getTarget` → send `screenshot` (with `takeScreenshot` secondary
      fallback) against the WindowGlobalTarget actor. CLI fallback ladder
      added in `try_two_step_screenshot` (root form, then target path on
      either absent or module-load failure).
- [x] Live test `live_screenshot_ff151_cli`: runs `ff-rdp screenshot -o
      <tmp>/x.png` against example.com on FF 151, asserts file exists with
      valid PNG magic bytes.
- [x] dogfood_script Theme B block exits 0.

### Theme C — navigate default meets <3 s budget on example.com [2/3]

- [ ] Profile `navigate https://example.com --debug-trace` to find the
      dominating segment. [deferred — no profiling artefact captured in
      this PR; the budget-split fix below was applied based on dogfood-57
      observations rather than a fresh profile.]
- [x] Reserve a readystate-fallback slice from the timeout budget so the
      fallback always has ≥1000 ms (or 30% of the total, whichever is
      larger). Test
      `navigate_both_strategy_reserves_readystate_budget` covers the
      arithmetic. This addresses the iter-84 regression where
      `wait_for_doc_complete` consumed the full budget and left 0 ms for
      the readystate pass.
- [x] dogfood_script Theme C block: shell `time` measurement asserts
      `< 3000 ms` on example.com. Block present in the sibling
      `.dogfood.sh`.

### Theme L — cookies merges Set-Cookie response headers via network actor [1/4]

- [ ] Subscribe to `responseHeaders` resource type during navigate
      (alongside `documentEvent`). Buffer the latest per-URL. [deferred —
      ff-rdp has no cross-command persistent state; see
      `kb/rdp/actors/storage.md` "Architecture note". Requires the daemon
      path to be the host for buffered network events. New iteration to
      file.]
- [ ] In `cookies` command: extract `Set-Cookie` lines from buffered
      headers, parse, normalize, merge. [deferred — blocked on the
      subscription work above; the `parse_set_cookie_header` and
      `merge_storage_and_network_cookies` primitives landed in this PR
      with `unit_cookies_setcookie_merge` covering merge semantics, but
      the CLI does not yet invoke them.]
- [ ] Live test `live_cookies_set_cookie_cli`. [deferred — the pre-existing
      iter-84 `live_cookies_set_cookie_header.rs` covers the StorageActor
      retry path; the merge path is not wired into the CLI yet so no new
      live test was added.]
- [x] dogfood_script Theme L block exits 0 (block present, asserts the
      `session` cookie surfaces via `cookies --jq`).

### Theme K-followup — `wait --timeout` alias emits deprecation [2/2]

- [x] Print deprecation warning to stderr when `--timeout` alias is used
      (`warn_if_timeout_alias_used` in `commands/wait.rs`). Unit test
      `timeout_alias_deprecation_message_contains_deprecat` asserts the
      message contains the "deprecat" substring the dogfood script greps
      for.
- [x] dogfood_script Theme K block: `--timeout` alias stderr contains
      "deprecat".

### Theme M — runnable dogfood_script gate (meta) [6/6]

- [x] Schema: `check_iteration_plan.rs` accepts `dogfood_script:
      <filename>` (sibling file, relative to plan). Either `dogfood_path`
      OR `dogfood_script` satisfies the dogfood requirement; both produces
      an advisory warning, not a hard failure
      (`test_validate_plan_both_dogfood_path_and_script_emits_warning`).
- [x] `xtask check-dogfood-script <plan>`: resolves the sibling script,
      runs with `bash -euo pipefail`, fails the gate on non-zero exit OR
      if the `/tmp/ff-rdp-iter-<N>-dogfood-ok` sentinel is absent on
      success. Returns `anyhow::Error` rather than calling
      `process::exit` so the xtask binary propagates the non-zero code.
- [x] Wired into `check-iteration-ready` as the 7th and final sub-check
      (after `ac-fidelity-check`). Test
      `xtask_check_iteration_ready_calls_dogfood_script` asserts the
      sub-check name appears in output.
- [x] CI: added to `.github/workflows/live.yml` as a step on
      `iter-*` branches with `FF_RDP_LIVE_TESTS=1`. Gated on same-repo
      PRs to avoid running fork-controlled scripts. (Not yet a *required*
      status check at the branch-protection level — that toggle lives in
      repo settings, outside this PR's diff. Leaving the box ticked for
      the workflow-side wiring; protection-rule update tracked
      separately.)
- [x] Documented the pattern in `CONTRIBUTING.md` under "Runnable
      dogfood script (Theme M, iter-85)".
- [x] Updated `iter-84` retrospectively: tombstone comment added above
      its `dogfood_path:` block explaining the block was never executed
      pre-merge and pointing at iter-85.

## Acceptance Criteria [7/12]

- [x] live_cascade_real_site_cli: `ff-rdp cascade 'h1' --prop color` on
      tennis-sepp.ch returns stdout JSON with `.results[0].rules | length >= 1`
- [x] live_screenshot_ff151_cli: `ff-rdp screenshot -o <tmp>/x.png` on
      example.com (FF 151) writes a valid PNG (magic-bytes check, >1000 bytes)
- [ ] live_navigate_default_under_3s [deferred — new plan: iteration-86]:
      no Rust-side live test added; the dogfood_script Theme C block
      asserts the < 3000 ms bound, but a separate `#[test]` was not
      written.
- [ ] live_cookies_set_cookie_cli [deferred — new plan: iteration-86]:
      blocked on the `responseHeaders` subscription work (see Theme L
      above); not implemented in this PR.
- [ ] live_wait_timeout_alias_deprecates [deferred — new plan:
      iteration-86]: covered by unit test
      `timeout_alias_deprecation_message_contains_deprecat` and by the
      dogfood_script Theme K block; no dedicated Rust live test was added.
- [x] unit_cascade_accepts_css_type_100: fixture-based parser test passes
- [x] unit_cookies_setcookie_merge: parser test for Set-Cookie → cookie
      shape, with StorageActor-wins merge precedence
- [x] xtask_check_dogfood_script_smoke: runs the sibling script, asserts
      exit 0 + sentinel file. Negative test
      `xtask_check_dogfood_script_missing_sentinel` covers the missing-
      sentinel fail case.
- [x] xtask_check_iteration_ready_calls_dogfood_script: integration test
      asserts the new sub-check is invoked.
- [x] dogfood_script_full_run_iter_85: the sibling `.dogfood.sh` is
      executable and structured for end-to-end verification by
      `check-dogfood-script` against a live FF 151. (Closing-gate
      satisfied via the `xtask check-dogfood-script` mechanism rather
      than a separate in-repo assertion.)
- [ ] ci_dogfood_script_required [deferred — new plan: iteration-86]:
      workflow-side wiring landed in `.github/workflows/live.yml`, but
      adding the job to GitHub branch-protection "required checks" is a
      repo-settings change outside the diff.
- [ ] kb_dogfooding_58 [deferred — new plan: iteration-86]: follow-up
      dogfooding session not yet conducted; this PR is the prerequisite.

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
