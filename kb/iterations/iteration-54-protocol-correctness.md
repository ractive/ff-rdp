---
title: "Iteration 54: Protocol Correctness & Robustness"
type: iteration
date: 2026-05-10
status: in-progress
branch: iter-54/protocol-correctness
tags:
  - iteration
  - protocol
  - rdp
  - core
  - correctness
  - robustness
---

# Iteration 54: Protocol Correctness & Robustness

Pure protocol-layer hardening in `ff-rdp-core`. No CLI surface changes. Driven by the [[#ultrareview]] of 2026-05-10, specifically the RDP-protocol findings.

The current correlation logic and a couple of recv-side gaps work in practice but rest on hacks (retry loops, event-sniffing) and have latent failure modes (mid-eval navigation hang, server-side actor leaks in long-lived daemons, dropped longString bodies, unbounded recv allocation). This iteration replaces the hacks with the canonical Mozilla-documented behaviors and adds the missing bounds.

## Tasks

### 1. Cap RDP length-prefix at 64 MiB [0/2]

`transport.rs:194-214` allocates `vec![0u8; length]` with the only bound being a 20-digit ASCII cap. A malicious or buggy peer announcing e.g. `99999999999999999999:` triggers immediate OOM/abort.

- [ ] Add `const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;` and reject larger declarations with `ProtocolError::FrameTooLarge { declared, max }`. 64 MiB chosen to comfortably fit Firefox screenshot data URLs (largest legitimate frame observed).
- [ ] Unit test: feed a length prefix `100000000:` (100 MB) and assert clean error, no allocation.

### 2. Correlate replies by absence of `type` field [0/3]

`actor_request` in `actor.rs:31` correlates by `from == to` only — unsafe for actors that emit pushes (console, watcher, network). The canonical Mozilla rule (per searchfox `devtools/shared/protocol.js`) is: *replies have no `type` field; events do*. Fixing this also removes two downstream hacks.

- [ ] Update `actor_request` to skip frames that have a `type` field (push events) and only consume the first `from == to` frame *without* `type` as the reply.
- [ ] Remove the `listTabs` retry hack in `root.rs:34` — the previous "incomplete packet" failures were misclassified `tabListChanged` events.
- [ ] Remove the eval-loop event-sniff workaround in `console.rs` that filters `consoleAPICall`/`pageError` by inspecting `type` — `actor_request` now handles it generically.

### 3. Abort `evaluateJSAsync` on mid-eval navigation [0/2]

`console.rs:165-186` waits for an `evaluationResult` matching `resultID` and silently discards everything else, including `tabNavigated`/`willNavigate`. If the page navigates during eval, the result never arrives and the loop hangs until the socket read timeout fires.

- [ ] In the eval-result wait loop, watch for `tabNavigated`/`willNavigate` from the matching target actor; on receipt, return `EvalError::NavigatedDuringEval` immediately.
- [ ] E2e test: live-recorded fixture for `eval` against a script that triggers `location.href = ...` — assert the typed error and reasonable elapsed time (< socket timeout).

### 4. Implement `releaseActor` for object/longString grips [0/3]

`evaluateJSAsync` returning `Object`, `LongString`, or exception objects allocates a server-side actor (`obj19`, `longstractor22`). Nothing currently calls `releaseActor`. Long-running daemons leak actor IDs into Firefox's connection pool indefinitely.

- [ ] Add `Grip::release(&conn) -> Result<()>` that sends `release` to the parent actor (or `releaseActor` to root, depending on grip type — see [object.js](https://searchfox.org/mozilla-central/source/devtools/server/actors/object.js)).
- [ ] In daemon mode, drop grips through a wrapper that calls `release` on `Drop`. In `--no-daemon` mode, the connection tears down anyway, so this is a no-op (the grips die with the connection).
- [ ] Loop-test: 1000 evals returning objects in daemon mode; assert server-side actor count stays bounded (use `getActors` on root or count via a heuristic).

### 5. Unwrap `longString` grips in network response bodies [0/2]

`network.rs:65-88` calls `as_str()` on `text` and silently produces `None` when Firefox returns a `longString` grip (`{type:"longString", actor, initial, length}`). Large response bodies are lost.

- [ ] Detect the grip shape; when present, fetch the full body via `longStringActor.substring(0, length)` and concatenate. Cap retrieval at `MAX_FRAME_BYTES` and report truncation in `meta`.
- [ ] E2e test against a fixture page returning > 8 KiB response body; assert full text is captured.

### 6. Remove legacy `startListeners(["PageError","ConsoleAPI"])` path [0/2]

`console.rs:52` keeps the old per-tab listener registration alongside the modern `WatcherActor.watchResources(["console-message","error-message"])`. Running both risks double-delivery.

- [ ] Drop the `startListeners` call. Verify all console-event paths (snapshot via `getCachedMessages`, follow via Watcher) still work.
- [ ] E2e test asserting no duplicate console messages on follow.

## Acceptance Criteria

- [ ] All existing tests + fixtures pass unchanged.
- [ ] New fixtures recorded against live Firefox (per [[CLAUDE]] test-fixtures policy).
- [ ] `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -q` clean.
- [ ] No CLI-visible behavior changes — pure core refactor + bug fixes.

## Design Notes

The reply-correlation fix is the highest-leverage change: it's ~5 lines in `actor.rs` and removes ~30 lines of workaround code elsewhere. Land it first; the other tasks become simpler with cleaner correlation.

Grip release in daemon mode is the only task introducing new lifetime/Drop discipline. Consider a `ScopedGrip<'a>` wrapper rather than a `Drop` impl on `Grip` itself — `Drop` can't return errors and we don't want release failures to be silent. A `ScopedGrip::release(self) -> Result<()>` consumed at end-of-scope is more honest.

## References

- [Firefox RDP docs](https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html)
- [searchfox protocol.js](https://searchfox.org/mozilla-central/source/devtools/shared/protocol.js)
- [searchfox webconsole.js](https://searchfox.org/mozilla-central/source/devtools/server/actors/webconsole.js)
- [searchfox object.js](https://searchfox.org/mozilla-central/source/devtools/server/actors/object.js)
- [geckordp](https://github.com/jpramosi/geckordp) — third-party RDP client for cross-reference
- [[iterations/iteration-29-code-review-simplification]]
