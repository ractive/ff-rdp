---
title: Future Features Backlog
type: reference
date: 2026-04-06
tags: [backlog, future]
status: active
---

# Future Features Backlog

Features not yet implemented. Items completed in past iterations are marked done.

## Connection & Transport

- [ ] WebSocket transport mode (`ws:` prefix) for browser-based clients
- [x] Connection pooling / persistent daemon mode for reduced latency on repeated commands *(done: iteration 13)*
- [ ] `--follow` / streaming mode for real-time console and network event tailing
- [ ] Unix socket support for local-only connections

## Debugging

- [ ] Breakpoint management: `ff-rdp breakpoint set <url:line>`, `ff-rdp breakpoint list/remove`
- [ ] Step debugging: `ff-rdp step <into|over|out>`, `ff-rdp resume`, `ff-rdp pause`
- [ ] Stack frame inspection: `ff-rdp frames` when paused at breakpoint
- [x] Source listing: `ff-rdp sources` to enumerate loaded JS files *(done: iteration 10)*
- [ ] Source reading: `ff-rdp source <url> [--line-range]` to fetch source code
- [ ] Blackboxing: `ff-rdp blackbox <url>` to skip library code during debugging

## DOM & CSS (Native Actors)

- [ ] Native Inspector/Walker actor implementation for structured DOM trees
- [ ] Computed styles: `ff-rdp styles <selector>` via PageStyleActor
- [ ] DOM mutation watching: notify on DOM changes
- [ ] Accessibility tree inspection via AccessibilityActor

## Network

- [ ] Request/response body capture: `ff-rdp network <id> --body`
- [ ] Network blocking: `ff-rdp network block <url-pattern>`
- [ ] Network throttling: simulate slow connections
- [ ] HAR export: `ff-rdp network --har` for HTTP Archive format

## Browser Management

- [ ] Profile management: `ff-rdp profile create/list/delete` for isolated sessions
- [ ] Extension debugging: `ff-rdp extensions list` via WebExtensionDescriptorActor
- [ ] Multi-process debugging via ParentProcessDescriptorActor
- [ ] Worker debugging: `ff-rdp workers list` for web/service/shared workers

## Output & Integration

- [ ] `--format text` output mode with human-readable tables
- [ ] `ff-rdp perf audit` — single-command page performance report: TTFB, CWV, resource breakdown by type/domain, top-N slowest resources, third-party weight, DOM stats (node count, inline script size, render-blocking resources). Replaces multi-step jq workflows with one structured JSON output.
- [ ] Cookbook / recipes in `ff-rdp --help`: curated `--jq` one-liners for common tasks (top-N slowest resources, network summary by type, DOM size audit, third-party breakdown, etc.)
- [ ] Shell completions: `ff-rdp completions <bash|zsh|fish>`
- [ ] Configuration file: `.ff-rdp.toml` for default host/port/timeout settings

## Performance

- [x] Connection caching across invocations (socket reuse via background daemon) *(done: iteration 13)*
- [ ] Parallel tab operations: query multiple tabs in one invocation
- [ ] Lazy grip resolution: only fetch full object data when requested

## Distribution

- [ ] Homebrew tap: `brew install ractive/tap/ff-rdp`
- [ ] Scoop bucket for Windows
- [ ] Winget package
- [ ] AUR package for Arch Linux
- [ ] crates.io publication
- [ ] Nix flake

## Carryover from iter-54 / iter-55 (added 2026-05-10 after [[#ultrareview]])

Items that didn't land in iter-54/55 and weren't yet scoped into a follow-up iteration. Keep here until promoted into a planned iteration.

### Protocol-layer carryover (from iter-54 building blocks)

- [ ] Wire `ScopedGrip` into daemon-mode `eval`/`inspect` call sites so server-side actors are released after each command. Leak-soak test: 1000 evals returning objects, assert bounded actor count.
- [ ] Live-recorded e2e fixture for `evaluate_js_async` mid-eval navigation (script that triggers `location.href = ...`); assert `EvalNavigatedDuringEval` plus elapsed time below socket timeout.
- [ ] Live-recorded e2e fixture for `getResponseContent` against a > 8 KiB response body; assert full text captured and `truncated == false` below the cap.
- [ ] Drop legacy `WebConsoleActor::start_listeners(["PageError","ConsoleAPI"])` once a parallel-listen experiment confirms `WatcherActor.watchResources` delivers all messages. E2e test asserting no duplicate console messages on follow.
- [ ] Re-evaluate `actor_request` adopting the canonical *reply has no `type`* filter, once the `ThreadActor` `attach` reply path (which currently uses `{"type":"paused"}`) is decoupled.

### Deferred LOW items from the 2026-05-10 ultrareview

- [ ] Feature-flag the Firefox-version checks rather than the FF 120–150 clamp (FF 84 descriptors, FF 116 resources-array, FF 149 two-step screenshot).
- [ ] Document in `eval --help` that exception messages echo verbatim to stderr (user `throw new Error(document.cookie)` lands in shell history / CI logs).
- [ ] Tighten protocol struct field visibility in `ff-rdp-core` (`TabInfo`, `TargetInfo`, `NetworkResource`, `EvalResult`, `ConsoleMessage`) — currently all-`pub`; consider `pub(crate)` + accessors.
- [ ] Document `wait --eval` truthiness semantics and the timeout error shape in its `--help`.
- [ ] Mention `--jq` / `--format text` mutual exclusion in each subcommand's `--help` (currently only at root).
- [ ] Document ANSI color behavior on TTY vs pipe; expose `--no-color` if needed.
- [ ] Reconcile `cli.timeout` and `cli.daemon_timeout` semantics; the daemon's 30 s server-side read cap is currently independent of both.

### Deferred from iteration 56 (dogfood-41-fixes)

- [ ] **Screenshot fallback on Firefox 150** (was iter-56 A3): iter-53 promised a DOM `canvas.drawWindow` fallback when `screenshotActor.capture` fails; on Firefox 150 the trigger condition doesn't match the actual error. Needs live recording on FF 150 to capture exact error shape, then either widen the trigger or wire a real fallback. Defer until a Firefox 150 instance is available for live e2e.
- [ ] **`navigate --wait-text` first-call `noSuchActor`** (was iter-56 C1): iter-53 deferred resolution of console-actor; still reproduces intermittently on a fresh launch (one hit during dogfooding session 41 against `news.ycombinator.com`, retry succeeded). Needs a fresh-launch live recording loop to capture the failing transcript and confirm whether the stale actor is the console actor or the target actor.
