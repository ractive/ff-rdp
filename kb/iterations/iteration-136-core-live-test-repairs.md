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
first_call_sites: []
status: planned
---

# Iteration 136: repair four stale ff-rdp-core live tests

Found by the post-batch sweep and triaged during [[dogfooding-session-63]]. **All four are
test-only bugs — there is no product regression.** The commit message must say so; do not
claim "fixes an FF153 cookie regression".

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

### Theme C — stale pre-FF125 method name

`live_accessibility_tree` sends a raw `getRootNode` straight to the walker actor
(`live_record_fixtures.rs:2124-2126`). Firefox 153 rejects it:

```
"error": "unrecognizedPacketType",
"message": "...accessiblewalker18 does not recognize the packet type 'getRootNode'"
```

Product code already handles this — `crates/ff-rdp-core/src/actors/accessibility.rs:95-119`
(`AccessibilityActor::get_root`) tries `getDocument` first (the FF125+ rename) and only falls
back to `getRootNode` on `unrecognizedPacketType`. The test bypasses its own helper.

Fix: call `AccessibilityActor::get_root` instead of hand-rolling the raw send.

### Theme D — guard against the class of bug

Both A and B are "test helper does something the protocol doesn't support, and only fails at
runtime". Add whatever cheap guard fits: e.g. make `send_raw` reject known-oneway packet types,
or give the recording helpers an explicit oneway variant so the choice is visible at the call site.
Keep this proportionate — a small guard, not a framework.

## Acceptance criteria

- [ ] live_cookies: passes, no `recv: Timeout` panic
- [ ] live_cookies_empty: passes, no `recv: Timeout` panic
- [ ] live_cookies_httponly: passes **and the process exits** — no hang; verify with a
      wall-clock bound, not just a green tick
- [ ] live_accessibility_tree: passes against Firefox 153 via `AccessibilityActor::get_root`
- [ ] unit_send_raw_rejects_oneway (or equivalent Theme D guard): a test proving the
      misuse in Theme A is now caught at the helper boundary
- [ ] full core suite: `cargo test -p ff-rdp-core -- --include-ignored --test-threads=1`
      completes with 0 failures and terminates

## Notes

- Requires a headless Firefox on port 6000 for the core live tests.
- Triage detail and process-sample evidence in [[dogfooding-session-63]].
