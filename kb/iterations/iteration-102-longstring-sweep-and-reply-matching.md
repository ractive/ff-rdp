---
title: "Iteration 102: longString sweep + retiring blind transport.request — no silently-truncated values, no unmatched replies"
type: iteration
date: 2026-07-09
status: planned
branch: iter-102/longstring-sweep
depends_on: []
firefox_refs:
  - lines: 15-78
    path: devtools/shared/specs/node.js
    why: >-
      nodeValue and attribute-value slots are declared longstring — Firefox
      sends a longString grip instead of an inline string past the
      DebuggerServer longStringLength threshold.
  - lines: 55-270
    path: devtools/shared/specs/storage.js
    why: >-
      cookie/localStorage/sessionStorage value slots declared longstring —
      large stored values arrive as grips.
  - lines: 40-50
    path: devtools/shared/specs/style/style-types.js
    why: >-
      computed-style value slot declared longstring — long CSS custom
      property values arrive as grips.
  - lines: 64-85
    path: devtools/shared/specs/string.js
    why: >-
      the reference read() handles inline-string-or-grip transparently — the
      behavior our typed LongString decoder mirrors and these call sites skip.
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
tags: [iteration, longstring, protocol, correctness, review-2026-07]
---

# Iteration 102: longString sweep + retiring blind `transport.request`

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

### A. longString sweep [0/4]
- [ ] Audit `crates/ff-rdp-core/src/specs/` + Firefox
      `devtools/shared/specs/*.js` for every slot declared `longstring` that
      ff-rdp consumes; produce the definitive list (expected: dom_walker
      nodeValue + attrs, storage values, page_style computed values — plus
      anything the audit surfaces).
- [ ] Route each site through the existing `LongString` decoder
      (`specs/types.rs:37-99`) with `substring` fetch and actor release,
      matching the console/network implementations.
- [ ] Record fixtures from real Firefox (per the recording workflow in
      `.claude/CLAUDE.md`): pages with a >20 KB text node, a >20 KB cookie
      value, and a >20 KB CSS custom property, via
      `live_record_fixtures.rs` + `save_core_fixture()`.
- [ ] Update [[lessons-learned]]#longstring-grips-everywhere: list the swept
      sites and the rule "any new spec consumer with a `longstring` slot must
      use the typed decoder".

### B. Retire blind `request()` [0/2]
- [ ] Route `TargetFront::reload(force=true)` (`fronts/target.rs:53`)
      through `actor_request`/the typed call path so the reply is matched
      by `recv_reply_from`; drop the `let _ =` swallow.
- [ ] Demote `RdpTransport::request` (`transport.rs:433-436`) to
      `#[cfg(test)]` (or `pub(crate)` with a doc warning if tests outside
      the crate need it) so no new production caller can appear.
      Note (iter-101): this line range shifted from the original `450-453`
      after iter-101 deleted the dead `DemuxReader`/`split_demux`/`Packet`
      pub API (425 lines) from `transport.rs`; re-verify against
      `origin/main` before starting in case of further drift.

### C. `.expect()` cleanup [0/1]
- [ ] Remove the three production `.expect()` sites: build the packet `Map`
      directly instead of re-asserting shape (`transport.rs:463`, was `480`
      before iter-101's `DemuxReader` deletion shifted line numbers); return
      `Result` from `ScreenshotArgsExt::to_args_value`
      (`actors/screenshot.rs:96-98`); restructure the `CAPTURE_METHODS`
      fallback to avoid `Option::expect`
      (`actors/screenshot_content.rs:53`).

## Acceptance Criteria [0/6]

- [ ] live_dom_text_longstring_roundtrip: a text node injected with 20 000
      chars is returned by `dom`/`page-text` at full length (== 20000), not
      empty.
- [ ] live_cookie_longstring_value: a 20 000-char cookie value is returned in
      full by `cookies`.
- [ ] live_computed_longstring_value: a 20 000-char CSS custom property value
      is returned in full by `computed`.
- [ ] live_reload_force_with_watched_resources: `reload --force` while
      console resources are being watched → the reload reply is correctly
      matched and an immediately-following request on the same actor returns
      its own reply (no stream desync).
- [ ] unit_no_production_expect_in_core: the existing source-scan test is
      extended to fail on `.expect(` in non-test ff-rdp-core code (and
      passes because the three sites are gone).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
