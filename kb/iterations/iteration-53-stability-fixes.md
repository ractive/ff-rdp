---
title: "Iteration 53: Stability Fixes"
type: iteration
date: 2026-05-06
status: planned
branch: iter-53/stability-fixes
tags:
  - iteration
  - bugfix
  - navigate
  - daemon
  - screenshot
  - actor
---

# Iteration 53: Stability Fixes

Third of three iterations addressing [[../dogfooding/dogfooding-session-40]]. Depends on [[iteration-51-onboarding-fixes]] landing first (uses the `meta.connection` and Firefox-version probing introduced there). Companion: [[iteration-52-input-eval-ergonomics]].

Three reliability bugs that are individually small but together make the CLI feel flaky on first contact:

1. `navigate --wait-text` reproducibly fails with `noSuchActor` on the *first* call after a fresh launch — the console actor is resolved before navigation, then invalidated when navigation tears down the docshell.
2. The daemon prints `warning: registry not found ...` on the happy path, even though the direct-connection fallback works. New users read `daemon.log` for nothing.
3. `screenshot` fails with `Unable to load actor module` on some Firefox versions, with no fallback or clear version message.

## Tasks

### 1. Re-resolve console actor in `navigate --wait-text` [2/2]

`navigate URL --wait-text "..."` reproducibly fails on the *first* navigate after a fresh launch with `noSuchActor (unknownActor)` on `consoleActor3`. The wait-text path resolves the console actor *before* navigation, then the actor is invalidated when navigation tears down the docshell.

- [ ] In the `navigate --wait-text` flow, defer console-actor resolution until *after* the navigation `frameUpdate` / `tabNavigated` event fires. Re-resolve on each navigation rather than reusing the pre-navigate handle.
- [ ] E2e test (live-recorded fixture) for fresh-launch → `navigate --wait-text` on the very first call.

### 2. Suppress benign daemon "registry not found" warning [2/2]

`warning: daemon started but registry not found: timed out after 5s waiting for daemon to write registry, connecting directly` appears even when the direct fallback succeeds. Visual noise that pushes new users to read `daemon.log` for nothing.

- [ ] Downgrade to debug-level (only printed under `RUST_LOG=debug` or `--verbose`) when the direct connection succeeds. If the direct connection *also* fails, keep the warning visible alongside the actual failure.
- [ ] E2e test asserting the warning is silent on the happy path and visible on the genuinely-broken path.

### 3. Graceful `screenshot` fallback / version warning [3/3]

`screenshotActor.capture` fails with `Unable to load actor module 'devtools/server/actors/screenshot'` on some Firefox versions (the user's hit `ChromeUtils.importESModule: global option is required in DevTools distinct global`). Detect and either fall back or report cleanly.

- [ ] On `screenshot` invocation, if the screenshot actor fails to load, fall back to a DOM-based capture path via `eval` (`canvas.drawWindow` or `html2canvas`-style, whichever is feasible without external deps). Only attempt the fallback when the actor failure is the known module-load error, not for other failure modes.
- [ ] If no fallback works (or fallback is also broken on this version), surface a clean one-liner: `error: screenshot actor unavailable on Firefox <version>; minimum supported version: <Y>. hint: upgrade Firefox or run \`ff-rdp doctor\` for the full compatibility report.` (Uses `meta.connection.firefox_version` from iter-51.)
- [ ] E2e test: mock the actor-load failure, assert clean error message and non-zero exit. Separate test for the successful-fallback path if implemented.

## Acceptance Criteria

- [ ] `navigate --wait-text` succeeds on the first call after a fresh launch.
- [ ] The daemon "registry not found" warning is silent when the direct fallback succeeds.
- [ ] `screenshot` either falls back to a working capture path or fails with a clean version-mismatch message that names `doctor`.
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -q`.
