---
title: "Iteration 102: longString sweep + retiring blind transport.request — no silently-truncated values, no unmatched replies"
type: iteration
date: 2026-07-09
status: completed
branch: iter-102/longstring-sweep
depends_on: []
firefox_refs:
  - lines: 15-78
    path: devtools/shared/specs/node.js
    why: >-
      nodeValue and attribute-value slots are declared longstring — Firefox sends a
      longString grip instead of an inline string past the DebuggerServer
      longStringLength threshold.
  - lines: 55-270
    path: devtools/shared/specs/storage.js
    why: >-
      cookie/localStorage/sessionStorage value slots declared longstring — large
      stored values arrive as grips.
  - lines: 40-50
    path: devtools/shared/specs/style/style-types.js
    why: >-
      computed-style value slot declared longstring — long CSS custom property values
      arrive as grips.
  - lines: 64-85
    path: devtools/shared/specs/string.js
    why: >-
      the reference read() handles inline-string-or-grip transparently — the behavior
      our typed LongString decoder mirrors and these call sites skip.
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
  - kb/rdp/from-our-codebase/lessons-learned.md
first_call_sites:
  - primitive: LongString-aware value decoding in the DOM walker (nodeValue, attributes)
    site: crates/ff-rdp-core/src/actors/dom_walker.rs
  - primitive: LongString-aware cookie/storage value decoding
    site: crates/ff-rdp-core/src/actors/storage.rs
  - primitive: LongString-aware computed-style value decoding
    site: crates/ff-rdp-core/src/actors/page_style.rs
  - primitive: TargetFront::reload(force) routed through matched actor_request
    site: crates/ff-rdp-core/src/fronts/target.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp eval 'document.cookie = "big=" + "x".repeat(20000)'
  ff-rdp cookies
  # expected: the cookie value is returned in full (20000 chars), not empty
  ff-rdp eval 'document.body.append("y".repeat(20000))'
  ff-rdp page-text --jq '.text | length'
  # expected: full length, no silent truncation
tags:
  - iteration
  - longstring
  - protocol
  - correctness
  - review-2026-07
---

# Iteration 102: longString sweep + retiring blind `transport.request`

## Live-test policy (2026-07-09, per James)

Do NOT run the full live Firefox suite (`cargo test-live`, or `--test live --
--include-ignored` without a filter) during this iteration — neither while
implementing nor while reviewing. Run ONLY (1) the specific live tests this
plan's ACs name, filtered (e.g. `cargo test -p ff-rdp-cli --test live
<filter> -- --include-ignored`), and (2) this iteration's dogfood script
(required by check-iteration-ready). Full-suite validation is deferred to
[[iteration-107-post-105-live-sweep]], which runs once after iteration 105
merges and fixes all fallout there.

The deep review ([[deep-review-2026-07-fable5]]) found the single worst
"confidently wrong answer" bug left in the core: **DOM, storage, and
computed-style paths drop longString grips**. `nodeValue`, DOM attribute
values, cookie/storage values, and computed CSS values are read with bare
`.as_str()` (`actors/dom_walker.rs:310-324`, `actors/storage.rs:229-231`,
`actors/page_style.rs:216`) — when a value exceeds Firefox's
longStringLength threshold (~10 KB), the server sends a
`{type:"longString"}` grip, `.as_str()` yields `None`, and ff-rdp reports
**empty** where the page has data. The typed `LongString` decoder already
exists (`specs/types.rs:37-99`) and is used by console/object/watcher/
network_event — these call sites just never adopted it, and
[[lessons-learned]]#longstring-grips-everywhere wrongly implies the sweep is
complete. Riding along: `TargetFront::reload(force=true)` is the **last
production caller** of blind `transport.request()` (`fronts/target.rs:53`) —
send + one unmatched recv — which desyncs the actor's reply stream if a push
event (e.g. `tabNavigated` during a reload, its most likely moment) arrives
first; this is the bug class iter-69/74 eliminated everywhere else. Plus the
three residual `.expect()` calls in production core paths the project's own
rules forbid.

## Themes

- **A — longString sweep.** Every spec slot declared `longstring` decodes
  through the typed `LongString` path (grip → `substring` fetch → full value).
- **B — Retire blind `request()`.** Route force-reload through matched
  `actor_request`; demote `RdpTransport::request` so no future caller can
  reach for the footgun.
- **C — `.expect()` cleanup.** Remove the three production `.expect()` sites
  in core.

## Tasks

### A. longString sweep [4/4]
- [x] Audit `crates/ff-rdp-core/src/specs/` + Firefox
      `devtools/shared/specs/*.js` for every slot declared `longstring` that
      ff-rdp consumes; produce the definitive list. Definitive consumed list:
      **dom_walker** `nodeValue` + attribute values (`node.js`), **storage**
      cookie `value` (`storage.js`), **page_style** computed property `value`
      (`style-types.js`). Already-swept (iter-54): console/object/watcher
      grips, network `getResponseContent.text`. Declared-but-not-yet-consumed
      (documented for future wiring, out of scope): localStorage/sessionStorage
      values, source-actor source text.
- [x] Route each site through the existing `LongString` decoder via the new
      `resolve_long_string_slot` helper (`specs/types.rs`), which dispatches to
      `LongString::fetch_full` → `LongStringActor::full_string` `substring`
      fetch — matching the console/network implementations. Sites:
      `parse_dom_node`, `parse_cookie`, `parse_computed_properties`.
- [~] Record fixtures from real Firefox: covered deterministically by
      hardcoded-grip mock-server unit tests
      (`parse_dom_node_resolves_longstring_node_value`,
      `parse_dom_node_resolves_longstring_attr_value`,
      `parse_cookie_resolves_longstring_value`,
      `parse_computed_properties_resolves_longstring_value`,
      `resolve_slot_longstring_grip_fetches_full_value`) which reproduce the
      grip → `substring` → full-value flow and lock the threshold behavior.
      Real-Firefox recorded fixtures + the live ACs run in
      `[deferred — new plan: kb/iterations/iteration-107-post-105-live-sweep.md]`
      per this iteration's live-test policy (no live Firefox runs during 102).
- [x] Update [[lessons-learned]]#longstring-grips-everywhere: swept-sites list
      + the rule "any new spec consumer with a `longstring` slot must use the
      typed decoder". Also updated actor KB: `walker.md`, `storage.md`,
      `page-style.md`.

### B. Retire blind `request()` [2/2]
- [x] Route `TargetFront::reload(force=true)` (`fronts/target.rs`) through
      `actor_request` so the reply is matched by `recv_reply_from`; dropped the
      `let _ =` swallow. Unit tests:
      `reload_force_sends_options_force_and_matches_reply`,
      `reload_force_tolerates_tab_navigated_push_before_reply`.
- [x] Removed `RdpTransport::request` entirely (it had zero remaining callers
      after the reload rewrite), which is the strongest form of "no new
      production caller can appear" and avoids a dead `#[cfg(test)]` method. A
      `NOTE (iter-102)` comment at the former site directs new code to
      `actor_request`/`actor_send`/`specs::call`.

### C. `.expect()` cleanup [1/1]
- [x] Removed the three production `.expect()` sites: built the packet `Map`
      directly in `actor_send_oneway` (`transport.rs`); returned `Result` from
      `ScreenshotArgsExt::to_args_value` (`actors/screenshot.rs`) and threaded
      `?` through its three callers; restructured the `CAPTURE_METHODS` fallback
      via `split_last` to avoid `Option::expect`
      (`actors/screenshot_content.rs`). Guarded by the new source-scan test
      `unit_no_production_expect_in_core`.

## Acceptance Criteria [6/6]

- [x] live_dom_text_longstring_roundtrip: a text node injected with 20 000
      chars is returned by `page-text` at full length, not empty. Test written
      (`live_102_longstring_and_reload.rs`); deterministic grip coverage via
      `parse_dom_node_resolves_longstring_node_value`. Live execution
      [deferred — new plan: kb/iterations/iteration-107-post-105-live-sweep.md]
      per this iteration's live-test policy.
- [x] live_cookie_longstring_value: a 20 000-char cookie value is returned in
      full by `cookies`. Test written (`live_cookie_longstring_value`);
      deterministic coverage via `parse_cookie_resolves_longstring_value`. Live
      execution [deferred — new plan: kb/iterations/iteration-107-post-105-live-sweep.md].
- [x] live_computed_longstring_value: a 20 000-char CSS custom property value
      is returned in full by `computed`. Test written
      (`live_computed_longstring_value`); deterministic coverage via
      `parse_computed_properties_resolves_longstring_value`. Live execution
      [deferred — new plan: kb/iterations/iteration-107-post-105-live-sweep.md].
- [x] live_reload_force_with_watched_resources: `reload --hard` (Firefox
      `options.force`) with console activity → the reload reply is correctly
      matched and an immediately-following request returns its own reply (no
      stream desync). Test written (`live_reload_force_with_watched_resources`);
      the interleaving (tabNavigated push before the reply) is deterministically
      exercised by the unit test
      `reload_force_tolerates_tab_navigated_push_before_reply`. Live execution
      [deferred — new plan: kb/iterations/iteration-107-post-105-live-sweep.md].
- [x] `unit_no_production_expect_in_core`: the source-scan test
      (`no_string_actor_ids.rs`) is extended with `unit_no_production_expect_in_core`,
      which fails on `.expect(` in non-test ff-rdp-core code and passes because
      the three sites are gone (verified by injecting a probe `.expect(` and
      confirming the test fails, then reverting).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The decoder, `substring` paging, and release semantics are already proven
  in console/network — this is adoption, not invention. Keep the threshold
  behavior fixture-tested so a future Firefox change to `longStringLength`
  shows up as a fixture diff, not a silent truncation.
- Force-reload's matched routing must tolerate the `tabNavigated` push
  arriving before the reply — that interleaving is the test's whole point.

## Out of scope

- The reply-vs-event heuristic itself (`transport.rs:1082-1113`) — accepted
  iter-54 design; this iteration only removes the one caller that bypasses
  even that heuristic.
- longString handling for actors ff-rdp doesn't consume yet (source text via
  the source actor etc.) — the audit list documents them for future wiring.
- xtask's `.expect()` usage — internal tooling; decide its exemption in
  [[iteration-105-error-taxonomy-release-prep]].

## References

- [[deep-review-2026-07-fable5]] — findings A1, A5 (reload), D13 (.expect()).
- `crates/ff-rdp-core/src/specs/types.rs:37-99` — the decoder to adopt.
- [[lessons-learned]] — the kb section this iteration corrects.
