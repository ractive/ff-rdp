---
title: "Iteration 47: Dogfooding 38 Bug Fixes"
type: iteration
date: 2026-04-17
status: completed
branch: iter-47/dogfood-bugfixes
tags:
  - iteration
  - bugfix
  - scroll
  - format
  - launch
  - reload
  - responsive
---

# Iteration 47: Dogfooding 38 Bug Fixes

Fix bugs found in [[dogfooding/dogfooding-session-38]] on MDN Web Docs. Prioritized by severity.

## Source of Truth

All bug findings from dogfooding session 38, plus one carryover from session 37.

## Tasks

### 1. Fix `ff-rdp launch` to write remote debugging prefs [3/3]

`ff-rdp launch --headless --port 6000` fails to connect on a fresh profile because Firefox's remote debugging prefs aren't set. Users must manually create a `user.js` with `devtools.debugger.remote-enabled`, `devtools.debugger.prompt-connection`, and `devtools.chrome.enabled`. The `launch` command should handle this automatically.

- [x] When creating/using a profile, write the required `user.js` prefs (`devtools.debugger.remote-enabled=true`, `devtools.debugger.prompt-connection=false`, `devtools.chrome.enabled=true`) before launching Firefox
- [x] Only write prefs if the profile doesn't already have them (avoid clobbering user customizations)
- [x] Add e2e test that verifies `launch` creates a connectable Firefox instance with a fresh temp profile

### 2. Fix `scroll` commands reporting stale viewport position [3/3]

`scroll bottom` reports `viewport.y: 0` when actual position is 24381. `scroll top` reports `viewport.y: 24381` when actual is 0. The viewport position is read before the scroll takes effect.

- [x] Add a small delay or re-read position after the `scrollTo` call returns in the scroll eval JS
- [x] Verify that `scroll top`, `scroll bottom`, and `scroll by --dy N` all report the correct post-scroll position
- [x] Add e2e test asserting viewport.y changes after scroll

### 3. Add `--format text` for `geometry`, `network` summary, and `dom tree` [3/3]

These three commands silently output JSON when `--format text` is used. All other commands with text formatters work correctly.

- [x] Add text formatter for `geometry` (tabular: selector, x, y, width, height, in_viewport, overlaps)
- [x] Add text formatter for `network` summary mode (tabular sections matching the JSON structure: totals, by-type, by-domain, slowest)
- [x] Add text formatter for `dom tree` (indented tree like `snapshot --format text`)

### 4. Fix `reload --wait-idle` reporting 0 requests [2/2]

Carryover from session 37. `reload --wait-idle` waits the full timeout then reports `requests_observed: 0` and `idle_at_ms` equal to the timeout. The network watcher attaches after the reload fires, missing all traffic.

- [x] Attach the network watcher *before* sending the reload command, so it captures requests from the start
- [x] Add e2e test that verifies `reload --wait-idle` on a real page reports `requests_observed > 0`

### 5. Fix `responsive` reporting implausible negative `rect.y` values [2/2]

At width=320, elements show `rect.y: -117096.5`. The geometry capture happens before layout stabilizes after the viewport resize.

- [x] Add a short delay or wait for layout-stable signal (e.g., `requestAnimationFrame` + `setTimeout`) after each viewport resize before capturing geometry
- [x] Add e2e test that verifies responsive rect values are non-negative for visible elements

### 6. Hide irrelevant global flags from static-output commands [1/1]

`recipes --help` and `llm-help --help` show `--host`, `--port`, `--timeout`, `--format`, etc. which are meaningless for these static-text commands.

- [x] Use clap's `hide` attribute on inherited global flags for the `recipes` and `llm-help` subcommands, or define them without the global args group

## Acceptance Criteria

- [x] `ff-rdp launch --headless --port 6000` works with a fresh temp profile without manual user.js setup
- [x] `scroll bottom` reports a non-zero `viewport.y` on a page with scroll content
- [x] `geometry --format text`, `network --format text` (summary), and `dom tree --format text` produce tabular/tree output instead of JSON
- [x] `reload --wait-idle` reports `requests_observed > 0` on a real page
- [x] `responsive` reports non-negative `rect.y` values for visible elements
- [x] `recipes --help` does not show `--host`, `--port`, `--timeout` flags
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test`
