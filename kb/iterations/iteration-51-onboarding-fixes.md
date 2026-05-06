---
title: "Iteration 51: Onboarding & First-Contact Fixes"
type: iteration
date: 2026-05-06
status: planned
branch: iter-51/onboarding-fixes
tags:
  - iteration
  - dx
  - ai
  - onboarding
  - bugfix
  - launch
  - doctor
  - eval
  - type
---

# Iteration 51: Onboarding & First-Contact Fixes

Fix the issues uncovered in [[../dogfooding/dogfooding-session-40]]. The dogfooder spent ~10 minutes flailing before the first successful navigate because `launch` silently no-op'd when port 6000 was already held by a stale Firefox process, and every error message pointed back at "run launch" — exactly what they had already done.

The CLI is good *once you're connected*. This iteration is about closing the gap between "user runs first command" and "first successful page interaction" — for both AI agents and humans.

## Motivation

The single biggest pain point: **port collision is silent**. `launch --temp-profile` returned a healthy-looking JSON envelope with a fresh PID and `port: 6000`, but the new Firefox was a ghost — its `--start-debugger-server 6000` collided with an existing listener and quietly went nowhere. Meanwhile `tabs` happily talked to the *other* Firefox (a 13-day-old daily-driver instance) which had no debuggable tabs because session-restore had unloaded them. Result: empty `[]` from `tabs`, indistinguishable from "your launch failed."

Three more papercuts compounded the bad onboarding:
- `type` flag-vs-positional confusion with an unhelpful clap "tip"
- `eval` leaking `const`/`let` into a shared global scope across invocations
- `screenshot` actor broken on the user's Firefox version with no fallback

## Tasks

### 1. Detect port collisions in `launch` [3/3]

Root cause of the 10-minute hole. After spawning Firefox, probe the debugger port to detect a pre-existing listener and surface it with the offending PID + hint.

- [ ] Before spawning Firefox, check whether `localhost:<port>` already accepts a TCP connection. If yes, identify the listener (`lsof -nP -iTCP:<port> -sTCP:LISTEN` on Unix; `netstat -ano -p tcp` on Windows) and abort with: `error: port <N> is already in use by <process> (PID <pid>). hint: pass --port <N+10> to use a different port, or stop the existing listener.`
- [ ] When the pre-launch check is inconclusive (no `lsof`), still poll the port for ~3s after spawn — if the port was occupied before spawn but our spawned Firefox didn't bring up its own listener, fail with the same error.
- [ ] E2e test: simulate occupied port (start a dummy TCP listener on a free port, attempt `launch --port <that>`, assert the structured error and exit code).

### 2. Add `ff-rdp doctor` subcommand [4/4]

One command to run when stuck. Probes the connection top-to-bottom and reports the failure mode in plain English.

- [ ] New `doctor` subcommand. Probes (in order):
  1. Daemon status: is the registry present? is the socket reachable? when was it started?
  2. Port owner: who is listening on `--port`? PID, process name, uptime.
  3. RDP handshake: can we connect and complete `getRoot`?
  4. Tab count: how many tabs are exposed by the connected target?
  5. Firefox version: parse from RDP `getRoot` / `applicationType`. Compare against actor minimums (screenshot actor needs ≥ X).
- [ ] Output a structured report (JSON envelope + text-mode "checklist" rendering with ✓/✗/⚠ and a one-line hint per failure).
- [ ] Exit code 0 if everything passes, 1 if any check fails.
- [ ] E2e tests: doctor on a working daemon (all green), doctor with no Firefox running (port owner = none, suggests `launch`), doctor with stale connection (no tabs, suggests checking PID/uptime).

### 3. Include connected-target metadata in every response [3/3]

A 13-day-old daily-driver Firefox would have been an instant red flag if `tabs` had reported `connected_pid` and `uptime_s`. Embed connection metadata in the JSON envelope `meta` field for all commands that talk to the browser.

- [ ] Resolve and cache (per-process / per-daemon-session) the connected target's `pid`, `port`, `profile_path` (from launch arguments where known), and Firefox `version`. Read once at handshake; refresh on connection re-establishment.
- [ ] Include `meta.connection = { host, port, connected_pid, uptime_s, firefox_version }` in every command's JSON envelope.
- [ ] Update at least one e2e test to assert the new `meta.connection` fields are populated.

### 4. Improve the canonical "no tabs" error [2/2]

The current message — `error: no tabs available — is a page open in Firefox? Use \`ff-rdp launch --headless --temp-profile\` to start one` — is actively misleading after the user has *already* launched. Branch on root cause.

- [ ] Distinguish three states and tailor the hint accordingly:
  - **No Firefox connected** (handshake fails / port not listening) → suggest `launch`.
  - **Firefox connected but zero tabs** (handshake succeeds, listTabs empty) → report connected PID + uptime; suggest opening a tab manually or relaunching with `--temp-profile`.
  - **Firefox connected, tabs exist but none debuggable** (e.g. all unloaded by session-restore) → suggest `--tab N` and report which tab indices were seen but unavailable.
- [ ] E2e test for each branch.

### 5. Make `type` work on React/Vue/Svelte inputs [3/3]

`type` currently sets `input.value = ...` directly. Modern frameworks track values via React's value-tracker / Vue's v-model, so the change is silently discarded. Fix once for every framework.

- [ ] In the `type` JS payload, use the native prototype setter (`Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set`, plus the equivalent for `HTMLTextAreaElement` and `HTMLSelectElement`) to invalidate React's value tracker.
- [ ] After the value mutation, dispatch `input` and `change` events with `{ bubbles: true }`.
- [ ] E2e test against a fixture page with a React-style controlled input (or a vanilla input with a tracker that throws on direct value assignment) — assert the bound state actually updates.

### 6. Improve `type` flag-vs-positional ergonomics [2/2]

`ff-rdp type --selector ... --text ... --clear` failed with a generic clap "tip" telling the user how to escape `--selector` as a value, not how to use the command. Other commands (`dom`, `wait`) accept `--selector`, so reaching for it was natural.

- [ ] Accept `--selector` and `--text` as named flags on `type` (in addition to the positional form). When both positional and named are provided, error clearly.
- [ ] If the user still hits clap's default "unexpected argument" error, override the help with a tailored hint: `hint: \`type\` accepts selector and text positionally — try \`ff-rdp type 'input[type=search]' 'Krankenkasse'\`.`
- [ ] E2e tests for both invocation forms.

### 7. Wrap `eval` user code in an IIFE by default [2/2]

`const x = ...` in two consecutive `eval` calls fails with "redeclaration of const x" because Firefox's console actor shares a global scope across invocations. Surprising default — fix it once.

- [ ] Wrap the user-supplied JS in `(function(){ "use strict"; <user code> })()` by default. Preserve the existing return-value semantics (last expression / explicit `return`).
- [ ] Add `--no-isolate` flag to opt out (when the user *wants* to share state across calls — e.g. building up a helper across an interactive debugging session).
- [ ] E2e test: two consecutive `eval 'const x = 1; x'` calls succeed by default; with `--no-isolate` the second one errors.

### 8. Re-resolve console actor in `navigate --wait-text` [2/2]

`navigate URL --wait-text "..."` reproducibly fails on the *first* navigate after a fresh launch with `noSuchActor (unknownActor)` on `consoleActor3`. The wait-text path resolves the console actor *before* navigation, then the actor is invalidated when navigation tears down the docshell.

- [ ] In the `navigate --wait-text` flow, defer console-actor resolution until *after* the navigation `frameUpdate` / `tabNavigated` event fires. Re-resolve on each navigation.
- [ ] E2e test (live-recorded fixture) for fresh-launch → `navigate --wait-text` on the very first call.

### 9. Suppress benign daemon "registry not found" warning [2/2]

`warning: daemon started but registry not found: timed out after 5s waiting for daemon to write registry, connecting directly` appears even when the direct fallback succeeds. Visual noise that pushes new users to read `daemon.log` for nothing.

- [ ] Downgrade to debug-level (only printed under `RUST_LOG=debug` or `--verbose`) when the direct connection succeeds. If the direct connection *also* fails, keep the warning visible alongside the actual failure.
- [ ] E2e test asserting the warning is silent on the happy path.

### 10. Graceful `screenshot` fallback / version warning [2/2]

`screenshotActor.capture` fails with `Unable to load actor module 'devtools/server/actors/screenshot'` on some Firefox versions (the user's hit `ChromeUtils.importESModule: global option is required in DevTools distinct global`). Detect and either fall back or report cleanly.

- [ ] On `screenshot` invocation, if the screenshot actor fails to load, fall back to a DOM-based capture path (`canvas.drawWindow` via `eval`, or `Page.captureScreenshot` if CDP is available) where feasible.
- [ ] If no fallback works, surface a clean one-liner: `error: screenshot actor unavailable on Firefox <version>; minimum supported version: <Y>. hint: upgrade Firefox or run \`ff-rdp doctor\` for full compatibility report.`
- [ ] E2e test: mock the actor-load failure, assert clean error message and non-zero exit.

### 11. Documentation & help updates [3/3]

- [ ] Update `--help` "Troubleshooting" section (or add one) covering: port collision, stale Firefox connection, `doctor` subcommand.
- [ ] Add `eval --help` note about default IIFE wrapping and `--no-isolate`.
- [ ] Add a short "First contact" section to the README aimed at AI agents: `launch` → `tabs` → `navigate`, and what to do when any step returns empty.

## Acceptance Criteria

- [ ] `launch` fails loudly with PID + hint when the target port is already in use.
- [ ] `ff-rdp doctor` reports green on a healthy daemon and pinpoints the failing layer otherwise.
- [ ] Every browser-touching command's JSON envelope includes `meta.connection` with `connected_pid`, `uptime_s`, `firefox_version`.
- [ ] "no tabs" error branches by root cause and includes connected-PID/uptime when relevant.
- [ ] `type` works against React-style controlled inputs without manual `eval` workarounds.
- [ ] `type --selector ... --text ...` works as a synonym for the positional form.
- [ ] Consecutive `eval 'const x = 1; x'` calls succeed by default; `--no-isolate` preserves old shared-scope behavior.
- [ ] `navigate --wait-text` succeeds on the first call after a fresh launch.
- [ ] The daemon "registry not found" warning is silent when the direct fallback succeeds.
- [ ] `screenshot` either falls back to a working capture path or fails with a clean version-mismatch message.
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -q`.
