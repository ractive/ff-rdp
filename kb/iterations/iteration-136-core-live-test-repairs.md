---
branch: iter-136/core-live-test-repairs
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-135-screenshot-ff153-capture-drift.md
dogfood_path: |
  # The repaired tests ARE the dogfood — they must pass, and not hang.
  ff-rdp launch --headless --port 6000
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-core --no-fail-fast -- --include-ignored --test-threads=1 \
    live_cookies live_cookies_empty live_cookies_httponly live_accessibility_tree
  # Post-condition: 4 passed, 0 failed, and the run TERMINATES (no hang).
first_call_sites:
  - primitive: AccessibilityActor::is_service_enabled
    site: crates/ff-rdp-cli/src/commands/a11y.rs (fast JS fallback when the a11y service is off)
status: planned
---

# Iteration 136: repair four stale ff-rdp-core live tests

Found by the post-batch sweep and triaged during [[dogfooding-session-63]]. The triage said
**all four are test-only bugs with no product regression**; that held for the three cookie
tests (Themes A, B) but **not** for `live_accessibility_tree` — implementing Theme C
uncovered a genuine product bug in `AccessibilityActor` (see below). Do not claim
"fixes an FF153 cookie regression": nothing about cookies changed in the product.

One of them **hangs indefinitely**, which is worse than failing: it stalls the suite and
would stall CI. That is the priority.

## Themes

### Theme A — oneway `unwatchResources` misused in test cleanup

`live_cookies` and `live_cookies_empty` fail identically. Their cleanup sends
`unwatchResources` through `send_raw()` (`crates/ff-rdp-core/tests/support/recording.rs:210-213`),
which does `send` then `.expect("recv")`. But `unwatchResources` is **oneway** — our own
`crates/ff-rdp-core/src/actors/watcher.rs:41` documents `oneway: true`, so Firefox never
replies. `RdpTransport::recv()` blocks its ~500 ms socket timeout and the `.expect` panics:

```
thread 'live_cookies' panicked at tests/support/recording.rs:212:22: recv: Timeout
```

The panic fires **after** the real assertions already passed. Call sites:
`crates/ff-rdp-core/tests/live_record_fixtures.rs:929-936` and `:1157-1164`.

Fix: fire-and-forget `transport.send(...)` with no `recv`, matching the pattern already used
correctly in `live_cookies_httponly` (`live_record_fixtures.rs:1067-1073`).

### Theme B — the hang: TCP accept loop that never terminates

`live_cookies_httponly` deadlocks. `live_record_fixtures.rs:968` runs
`for stream in listener.incoming().take(10)` inside a thread spawned at :967 and joined at
:1076. `.take(10)` caps at *up to* 10 items but never closes the listener; Firefox's single
`navigateTo` produces exactly one HTTP request, so after serving it the thread blocks forever
in `accept()` waiting for connection #2, and `server.join()` deadlocks with it. Confirmed by
sampling the hung process: main thread in `Thread::join`, server thread in `TcpListener::accept`.

The `isHttpOnly` assertion itself passes before the hang.

Fix: a single `listener.accept()` (only one request is ever expected), or an accept deadline.
Whichever is chosen, the test must terminate on its own even if Firefox sends nothing.

### Theme C — the walker's root accessor, corrected during implementation

`live_accessibility_tree` sends a raw `getRootNode` straight to the walker actor
(`live_record_fixtures.rs:2124-2126`). Firefox 153 rejects it:

```
"error": "unrecognizedPacketType",
"message": "...accessiblewalker18 does not recognize the packet type 'getRootNode'"
```

**The plan's original premise was wrong.** It assumed `AccessibilityActor::get_root`
already handled this because it tries `getDocument` first. It does not help: checking
`devtools/shared/specs/accessibility.js` in the local Firefox checkout shows
`accessibleWalkerSpec` has **neither** `getRootNode` **nor** `getDocument` — `getDocument`
exists only as an internal walker helper and was never a protocol method. Firefox 153
answers both with `unrecognizedPacketType`, so the product's fallback chain was dead code
and `ff-rdp a11y` had been silently degrading to its JS-eval tree.

The real protocol (documented in [[rdp/actors/accessibility]]):

- root document → `children` on the **walker**, no arguments, returns a 1-element array;
- a node's children → `children` on the **accessible actor itself**, no arguments. The
  walker's same-named method ignores arguments and always returns the root, so walking
  through the walker would repeat the root at every depth;
- and none of it answers at all until the platform accessibility service is enabled —
  the walker awaits a `document-ready` promise that never settles, so the request stalls
  to the socket read timeout instead of erroring.

Fix (product, not just test): `get_root` now uses walker-`children` with the legacy names
kept as a fallback, `children` addresses the accessible actor, and the new
`AccessibilityActor::is_service_enabled` (`bootstrap`) lets `ff-rdp a11y` take its JS
fallback immediately instead of hanging for the socket timeout when the service is off.
Enabling the service browser-wide is deliberately **not** done on the user's behalf — see
the follow-up plan [[iteration-143-native-a11y-tree]].

### Theme D — guard against the class of bug

Both A and B are "test helper does something the protocol doesn't support, and only fails at
runtime". Add whatever cheap guard fits: e.g. make `send_raw` reject known-oneway packet types,
or give the recording helpers an explicit oneway variant so the choice is visible at the call site.
Keep this proportionate — a small guard, not a framework.

## Acceptance criteria

- [x] live_cookies: passes, no `recv: Timeout` panic — cleanup now uses `send_raw_oneway`
- [x] live_cookies_empty: passes, no `recv: Timeout` panic — cleanup now uses `send_raw_oneway`
- [x] live_cookies_httponly: passes **and the process exits** — `serve_one_http_response`
      polls a non-blocking listener under `HTTP_SERVER_ACCEPT_DEADLINE` (30 s) and the test
      asserts both `served == true` and `started.elapsed() < HTTP_SERVER_ACCEPT_DEADLINE`
- [x] live_accessibility_tree: passes against Firefox 153 via `AccessibilityActor::get_root`
      (walker `children`) and `AccessibilityActor::children` (accessible `children`), with
      `AccessibilityActor::is_service_enabled` asserted true first
- [x] unit_send_raw_rejects_oneway: `reject_oneway_request` panics for `unwatchResources`,
      `clearResources`, `unwatchTargets`, `clearPicker` and passes request/reply types
- [x] a11y_falls_back_to_get_document_when_walker_children_unrecognized: e2e proof the
      legacy `getDocument` path still works when the walker rejects `children`
- [x] full core suite: `cargo test -p ff-rdp-core -- --include-ignored --test-threads=1`
      completes with 0 failures and terminates

## Notes

- Requires a headless Firefox on port 6000 for the core live tests.
- Carry-over filed before merge: [[iteration-143-native-a11y-tree]] — decide whether
  `ff-rdp a11y` should enable the browser-global accessibility service to serve the
  native tree instead of the JS-derived one.
- The CLI mock server now rewrites a reply's `from` to the addressed actor
  (`crates/ff-rdp-cli/tests/e2e/support/mock_server.rs`), so recorded fixtures replay
  regardless of the actor IDs of the session they were captured in.
- Triage detail and process-sample evidence in [[dogfooding-session-63]].
