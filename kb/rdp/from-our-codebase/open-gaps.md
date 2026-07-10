---
title: Open Protocol-Level Gaps in ff-rdp
type: rdp-note
tags: [rdp, from-codebase, gaps]
date: 2026-05-23
closed-in:
  - iter-61n
  - iter-61q
  - iter-61r
  - iter-61v
  - iter-74
  - iter-61w
updated-in:
  - iter-117
---

# Open Protocol-Level Gaps

Catalog of known RDP-layer gaps as of 2026-05-23, drawn from dogfooding sessions 48–53 and iterations 61g–61l. Each item: symptom, where the gap lives in the protocol, suggested investigation. Excludes UX-only issues — see the dogfooding session notes for the full list.

## with-network-fallthrough

**Symptom**: `navigate --with-network` engages the WatcherActor and inline-returns proper `{source: "watcher", status: 200, method: "GET", transfer_size: ...}`. The *next* standalone `network` call falls back to `source: performance-api` with `status: null, method: null` — even though `daemon status` shows `buffer_sizes: {network-event: 209}` (data IS captured).

**Protocol layer**: The watcher subscription appears to be torn down (or its data made unreachable) between the navigate response and the next CLI invocation. The buffer exists but the `network` command's source-selection logic picks performance-api.

**Sessions**: [[dogfooding-session-51]] #5, [[dogfooding-session-52]] AC-C, [[dogfooding-session-53]] AC-C.

**Effect**: response headers (CSP, HSTS, X-Frame-Options, Set-Cookie attributes) are completely unreachable in the security-audit workflow that motivated session 51. This is the single biggest protocol-level gap for the security-audit use case.

## shadow-dom-piercing

**Symptom**: `dom 'selector'` now correctly flags `hasShadowRoot: true` / `shadowMode: "open"` on host nodes (iter-61k) but does not traverse *into* the shadow root. SPAs that use shadow DOM heavily (Lit, web components) are opaque past the host.

**Protocol layer**: WalkerActor has shadow-DOM traversal support; we just don't call it. Need `--include-shadow` flag plumbed through.

**Sessions**: [[dogfooding-session-52]] gap #6, [[dogfooding-session-53]] feature gaps.

## actor-leak-in-daemon

**Status (iter-76b)**: CLOSED. `extract_grips` now walks watcher resource payloads and populates `ResourceGripGuard` for every `consoleAPICall` / `evaluationResult` containing object or longString grips. The drainer thread (`grip_release_drainer_loop`) owns `ReleaseQueueRx` and sends release packets over the shared `FramedWriter`. Type-safe dispatch via `AnyGripHandle` ensures `LongString` actors are not misidentified as `Object` actors. Live test: `live_grip_release_actually_releases` (`FF_RDP_LIVE_TESTS=1`).

Remaining known gap: grip-consuming call sites in inspector and network-response-body paths still do not call `add_grip`; those are low-priority because daemon eval is the primary source of unbounded actor growth. File a new iteration if they become a problem.

**Symptom**: Each `evaluateJSAsync` returning an object/longString allocates server-side actor IDs that are never released in long-running daemons. iter-54 task 4 landed `ObjectActor::release` + `ScopedGrip` wrapper as building blocks but didn't wire them into daemon-mode call sites or add a soak test.

**Protocol layer**: Fixed in iter-76b — `release` packets are now sent for grip actors extracted from daemon watcher events.

**Sessions**: surfaced in [[iteration-54-protocol-correctness]] task 4 (deferred sub-tasks 2 & 3); closed in [[iteration-76b-daemon-scalability-bulk-drain-and-live-grip-release]].

## legacy-startlisteners-coexistence

**Status (iter-71)**: experimental test added; removal deferred pending live verification.
`crates/ff-rdp-cli/tests/live_console_no_double_delivery.rs` implements the
parallel-listen experiment (`live_console_no_double_delivery`, gated on
`FF_RDP_LIVE_TESTS=1`, `#[ignore]`).  Run manually to verify no double-delivery
before removing the legacy `WebConsoleActor::start_listeners` call sites in
`commands/console.rs`.

**Symptom**: Console flow uses both `WebConsoleActor.startListeners(["PageError", "ConsoleAPI"])` *and* `WatcherActor.watchResources(["console-message", "error-message"])`. Running both risks double-delivery; iter-54 task 6 wanted to drop the legacy path.

**Protocol layer**: The watcher-only path was found to drop pushes for some actor states during earlier iterations, so the legacy listener was left wired. Needs a parallel-listen experiment + dedup before the legacy can be removed safely.

**Sessions**: noted in [[iteration-54-protocol-correctness]] task 6 (deferred). No live dogfooding session has caught a duplicate.

## viewport-sizing

**Symptom**: No way to programmatically change the viewport via RDP. `ResponsiveActor` does not expose `setViewportSize` — it was never part of the protocol. Memory note `project_viewport_protocol.md`.

**Protocol layer**: DevTools RDM sizes the viewport via `synchronouslyUpdateRemoteBrowserDimensions` on the browser chrome layer, which is unreachable from RDP (chrome process, not content/parent-process RDP scope). Our workaround is CSS-width simulation. A proper solution would require either a new actor in Firefox or driving the chrome via `chromeContext` eval.

**Sessions**: surfaced during responsive-design iteration; no dogfooding hit it as a blocker yet.

## sources-actor-fallback

**Symptom**: `sources` command falls back to JS-eval enumeration of `document.scripts` rather than using the Source actor / sources walker. iter-61g added the fallback after the Source-actor path was found unreliable in some Firefox versions.

**Protocol layer**: ThreadActor's `sources` method + per-source SourceActor exists but we don't wire it through. Fallback works fine but bypasses the canonical path.

**Sessions**: [[dogfooding-session-48]] #3 (resolved non-issue), tracked in iter-61g.

## spec-drift-bugs-awaiting-filing

**Status (iter-117)**: three `allow-spec-drift` annotations in
`crates/ff-rdp-core/src/actors/screenshot.rs` carried a `bug TBD` marker that
CLAUDE.md requires be replaced with a real Mozilla Bugzilla number before the
next release cut. Bugzilla searches (iter-117) found **no existing bug** for
any of the three gaps — they are novel, discovered by ff-rdp's own testing
against FF149–152. Filing needs a Bugzilla account (James's action), so the
annotations keep the convention `bug TBD` marker — the grep-detectable
awaiting-filing form the rdp-spec-reviewer flags by design — each carrying
its SD reference, with the Bugzilla-ready descriptions recorded below. **These block publishing v0.3.0**: James files the three bugs,
then a follow-up commit replaces each `bug TBD` with the real number before
the draft release is published. The v0.3.0 release is created as a **draft**
for exactly this reason.

### SD-1 — screenshot.args spec dict omits browsingContextID/snapshotScale/rect

- **Site**: `screenshot.rs:34` (`ScreenshotArgsExt`).
- **Component**: DevTools :: Framework / Server.
- **Summary**: The published `screenshot` actor spec dict at
  `devtools/shared/specs/screenshot.js:13-35` declares only
  `fullpage`/`file`/`clipboard`/`selector`/`dpr`/`delay`, but the server-side
  `devtools/server/actors/screenshot.js` reads three additional fields
  (`browsingContextID`, `snapshotScale`, `rect`) that a client must send for
  the two-step FF149+ capture protocol to work. The spec dict should declare
  these fields so out-of-tree clients can send them without spec-drift.

### SD-2 — screenshotActor.capture fails to load capture-screenshot.js (FF151, still repros on FF152)

- **Site**: `screenshot.rs:255` (`screenshot_via_process_drawsnapshot`).
- **Component**: DevTools :: Framework / Server.
- **Summary**: On Firefox 151 the root/target `screenshotActor.capture` path
  fails to load `capture-screenshot.js` in the DevTools distinct global
  (`moz-src:` scheme not supported there), so a full-page or even a plain
  capture throws a module-load failure. **iter-117 reassessment: the
  regression STILL reproduces on Firefox 152.0.5** — a live probe
  (`RUST_LOG=ff_rdp_cli::screenshot=debug ff-rdp screenshot`) logs
  `screenshotActor module load failure; retrying via
  screenshot_via_process_drawsnapshot`. The workaround (parent-process
  `BrowsingContext.drawSnapshot` eval) is therefore still required; a
  version-gate removing it on FF152 would break screenshots. The workaround
  must stay until Mozilla fixes the module-load path, at which point it is
  gated behind a version check or removed.

### SD-3 — WindowGlobalTarget.screenshot implemented server-side but undeclared in spec

- **Site**: `screenshot.rs:401` (`screenshot_via_target`).
- **Component**: DevTools :: Framework / Server.
- **Summary**: The `screenshot` method observed on the
  `WindowGlobalTarget` actor (FF151+ moved the capability onto the target
  actor) is read by the server but is not declared in
  `devtools/shared/specs/targets/window-global.js`. The spec dict should
  declare the `screenshot` method so out-of-tree clients can call it without
  spec-drift.

## summary

| Gap | Severity | Sessions broken | Pure-protocol? |
|---|---|---|---|
| with-network-fallthrough | major | 51, 52, 53 | yes (source selection + state) |
| shadow-dom-piercing | moderate | 52, 53 | no (walker API not called) |
| actor-leak-in-daemon | moderate | — | yes |
| legacy-startlisteners | latent | — | yes |
| viewport-sizing | known limitation | — | yes (RDP scope) |
| sources-actor-fallback | minor | — | yes |

## Closed gaps

The following gaps were closed by the iter-61m..61v stability roadmap and the
iter-61w refresh.  Kept here as a historical record; cross-link from
[[lessons-learned]] where each was originally surfaced.

### full-page-screenshot

closed-in: iter-61v

iter-61r reworked `screenshot --full-page` to call the root-scoped `screenshot`
actor with `fullpage:true, rect, snapshotScale, browsingContextID` (the
4th-positional `fullpage` to `drawSnapshot` is the actual switch).  iter-61v
added the live regression `live_screenshot_full_page_dpr2` asserting
PNG height ≥ `scrollHeight × DPR` on a ≥5000 px synthetic page.

### csp-eval-fallback

closed-in: iter-61r

`evaluateJSAsync` now sends `mapped: { await: true }` on every call.  The
SpiderMonkey `Debugger` API used for awaited evaluation is privileged and
bypasses page CSP entirely, so the dedicated `chromeContext: true` retry is
no longer needed for the CSP case.  See [[lessons-learned#async-eval-doesnt-resolve-promises]]
and [[evaluate-js]].

### headers-source-regression

closed-in: iter-61q

The full WatcherActor engagement work in iter-61q removed the source
downgrade.  `meta.source` now stays `"watcher"` regardless of which optional
fields the caller requests, and `getResponseHeaders` is issued per-entry
against the captured `networkEventActor` IDs.

### navigate-success-on-bad-dns

closed-in: iter-61v

`navigate` is now orchestrated as a multi-actor Command (iter-61r) and gated
on `document-event` resources (iter-61v).  The default daemon path invokes
`neterror_error_for_commit`, inspecting the post-navigate URL and the next
`target-available-form` event; a bad-DNS navigate returns a structured
`error_type: "neterror"` instead of false-success.

### navigate-race-timeout

closed-in: iter-61v

iter-61v's document-event gating replaces the previous `wait_for_commit`
timeout heuristic with a deterministic wait on `dom-loading` /
`dom-interactive` / `dom-complete` resources delivered through the
ResourceCommand bus.  Throttle on the bus was set to zero so a fast
cross-origin navigate cannot race the wait setup.

### oneway-method-hangs

closed-in: iter-74

Methods declared `oneway: true` in Firefox specs never send a reply.
Before iter-74, ff-rdp called `actor_request` on them, which blocked
until the socket read timeout (≥10s). Fixed by routing all oneway
calls through `actor_send`. Full list: `watcher.unwatchTargets`,
`watcher.unwatchResources`, `watcher.clearResources`, `root.unwatchResources`,
`root.clearResources`, `reflow.start`, `reflow.stop`, `walker.clearPicker`.
The xtask `check-oneway-conformance` CI gate prevents regression.
See [[rdp/protocol/message-format]] §"Oneway methods".

### sibling-packet-loss

closed-in: iter-74

`recv_reply_from` and `recv_event_from` silently dropped packets from
actors other than the one being awaited. This caused cross-actor events
(WatcherActor resource batches, intermediate `consoleAPICall` events during
`evaluateJSAsync`) to be lost. Fixed by forwarding all non-matching packets
to the event sink. Transport invariant is now: no packet read off the wire
is ever discarded. See [[rdp/protocol/message-format]] §"Transport invariant".

### registry-lifecycle-on-target-destroyed

closed-in: iter-74

`target-destroyed-form` events from the WatcherActor were not cascaded to
dependent fronts (inspector, walker, console) in the registry. Stale actor
IDs remained alive, causing `ActorGone` errors on subsequent operations.
Fixed by `dispatch_watcher_event` → `Registry::invalidate_target`, which
BFS-cascades invalidation from the target root to all dependent fronts.

### locale-pin

closed-in: iter-61w
needs verification

`intl.locale.requested=en-US` plus `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8`
env-var injection at Firefox launch was identified as the required fix
combination in iter-61l.  We believe the env-var half landed in one of the
iter-61m..61v iterations as part of the broader stability work, but no
specific iteration plan explicitly claims it — needs verification by a live
re-run on a German-locale machine.
