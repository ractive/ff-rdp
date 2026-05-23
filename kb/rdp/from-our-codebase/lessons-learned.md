---
title: RDP Lessons Learned (from ff-rdp implementation)
type: rdp-note
tags: [rdp, from-codebase]
date: 2026-05-23
---

# RDP Lessons Learned

Surprising constraints, footguns, and protocol quirks discovered while building `ff-rdp`. Each item is a paragraph with a pointer back to the dogfooding session, iteration, or memory note that first surfaced it.

## reply-vs-event

**There is no request ID. Reply correlation is by "no `type` field".** Per the Mozilla docs (and verified empirically), a packet from an actor that *lacks* a `type` field is a reply; one *with* a `type` field is a push event. We use this throughout — see `console.rs:129-151` (eval immediate reply) and `root.rs:32-73` (listTabs vs tabListChanged). **Caveat**: this is *not* universal — `ThreadActor.attach` legitimately returns `{"type":"paused"}` as its reply, so iter-54 task 2 deferred making this a global rule. Instead, push-event-heavy actors implement their own filter; quieter ones rely on bare `from == to` correlation. See [[iteration-54-protocol-correctness]] task 2 and the explanatory note at `actor.rs:31`.

## descriptor-wrappers

**Tab descriptors wrap target data in `"frame"`; process descriptors wrap it in `"process"`.** Calling `getTarget` on a `tabDescriptor*` returns `{frame: {actor, consoleActor, ...}, from: ...}`. The exact same call on a `processDescriptor*` (from `listProcesses`) returns `{process: {...}}`. We handle both with `parse_target_response` and `parse_process_target_response` (tab.rs:115-120, 83-89). Was a head-scratcher when daemon mode started using the parent-process actor.

## async-eval-doesnt-resolve-promises

**`evaluateJSAsync` does *not* await Promises.** The "Async" in the method name refers to the *protocol*'s async reply pattern (immediate ack + push event with `resultID`), not JavaScript Promise semantics. If your evaluated expression returns a Promise, you get a Promise grip back, not the resolved value. This blocked iter-37/38 daemon work; the fix was a stream API for events plus polling. See memory: `project_rdp_async_constraints.md`.

## mid-eval-navigation-hangs

**If the page navigates during `evaluateJSAsync`, the `evaluationResult` never arrives.** Before iter-54, we'd silently wait until the socket read timeout fired. Now (console.rs:175-179) we watch for `tabNavigated`/`willNavigate` *from the same console actor* and abort with `ProtocolError::EvalNavigatedDuringEval`. Critically: events from *unrelated* console actors on the same connection must be ignored — they're for other tabs we don't care about.

## consoleActor-staleness

**The `consoleActor` ID becomes invalid after navigation.** A new target is spawned on navigation; the old console actor returns errors on subsequent eval. Daemon mode must re-`getTarget` after each navigation event. This was central to [[iteration-36-console-follow-fix]] and is one of the reasons `--follow` initially produced no output. Watcher's `target-available-form` event is how we detect this without polling.

## csp-blocks-eval

**Page CSP blocks `evaluateJSAsync` on sites like HN and lit.dev.** The reply contains an `EvalError` whose message includes `"call to eval() blocked by CSP"`. Fix path: retry with `chromeContext: true` (console.rs:210-275), which evaluates in a privileged chrome JS context that isn't subject to page CSP. **Currently broken in the daemon path** — the retry doesn't fire on HN/lit.dev despite being implemented. See [[dogfooding-session-53]] AC-H, [[open-gaps#csp-eval-fallback]]. Unit-test green, live-broken pattern.

## about-neterror-csp

**`about:neterror` (the page Firefox shows on DNS failure) has its own restrictive CSP that blocks eval entirely.** So after a navigate to a bad-DNS URL, subsequent eval calls fail with CSP EvalError — even with `chromeContext: true` not fully bypassing it. The right thing is to detect `about:neterror` URL and report a clear error before attempting eval. See [[dogfooding-session-53]] AC-K.

## locale

**`intl.locale.requested=en-US` in `user.js` alone does NOT pin the Firefox locale on macOS.** DevTools / quirks-mode warning strings still come back in German on a German-locale machine. iter-61k added the pref; iter-61l identified that `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8` env-var injection at launch is also required. As of 2026-05-23 the env-var fix has not landed. See [[dogfooding-session-52]] #10 and [[dogfooding-session-53]] AC-B.

## with-network-watcher-engagement

**Subscribing to `network-event` resources via the watcher works during `navigate --with-network` but the data isn't reused for the next standalone `network` command.** The daemon's `buffer_sizes.network-event` shows events *are* buffered, but the standalone `network` call selects the performance-api fallback path anyway. Effect: response headers (CSP, HSTS, X-Frame-Options) are unreachable for the security-audit workflow that motivated [[dogfooding-session-51]]. Memory: `feedback_network_perf_api.md`. See also [[open-gaps#with-network-fallthrough]].

## headers-flips-source

**Adding `--headers` to `network --detail` flips `meta.source` from `watcher` back to `performance-api`.** iter-61k regression discovered in [[dogfooding-session-53]] N1. The very flag that should request headers actively destroys the data path that could provide them. Source-selection logic bug — fixable, just hasn't been.

## screenshot-two-step

**Firefox 149 removed the single-step `screenshotContentActor.captureScreenshot`** in favour of `prepareCapture` (content-process: collects DPR/zoom/rect) + `capture` (parent-process: calls `browsingContext.drawSnapshot` and returns the PNG data URL). The `screenshotActor` ID isn't on the target — it's a root-level actor obtained via `getRoot()`. See `screenshot.rs:9-37` and [[screenshot-protocol-ff149]].

## screenshot-headless-chrome-scope

**Headless Firefox 149+ can't take screenshots from content scope** — iter-61h's chrome-scope fallback fixed this. The rect override for `--full-page` is supposed to ride along the same path but currently doesn't reach `drawSnapshot`. Five-session-running bug; see [[open-gaps#full-page-screenshot]].

## strict-parsing

**Be paranoid about missing/malformed fields in RDP packets.** Different Firefox versions add, remove, or rename fields. CodeRabbit on PR #73 caught us silently dropping `listProcesses` entries that lacked an `actor` field — fixed in root.rs:100-128 to fail-fast with a clear `InvalidPacket` error. `isParent` defaults to `false` when absent (older builds), but a non-bool value is rejected. Pattern: parse explicitly, don't `serde_json::from_value` blindly.

## node-attrs-flat-array

**`WalkerActor` returns DOM node `attrs` as a flat alternating string array `["name", "value", "name2", "value2", ...]`**, not the obvious `[{name, value}, ...]`. We custom-parse via `parse_dom_node` in dom_walker.rs:20-47. Standard serde derive would silently fail.

## longstring-grips-everywhere

**`longString` grips appear wherever a string might exceed ~8 KiB — including `getResponseContent.text` for network bodies.** Before iter-54 task 5 we silently lost big response bodies because we called `as_str()` on the grip object and got `None`. Now we detect the grip shape and chunk-fetch via `LongStringActor::full_string`, capping at `MAX_FRAME_BYTES`.

## frame-size-cap

**A peer can announce an arbitrary length prefix, and naive code will `vec![0u8; length]` and OOM.** iter-54 task 1 capped declared frame size at 64 MiB (`MAX_FRAME_BYTES`) before any allocation. 64 MiB comfortably fits full-page screenshot data URLs (largest legitimate frame observed). Reject larger with `ProtocolError::FrameTooLarge`.

## actor-leaks

**Every `evaluateJSAsync` returning an object/longString allocates a server-side actor (e.g. `obj19`, `longstractor22`) that lives until the connection closes — *or* until you send `release` to it.** Long-running daemons leak indefinitely. iter-54 task 4 added `ObjectActor::release` and a `ScopedGrip` wrapper but the daemon eval/inspect call sites still return raw `Grip`s. Soak test for bounded actor count is also pending.

## watcher-resources-shape

**The `resources-available-array` event packs resources as `array: [["type-name", [items...]]]` — a list of `[string, list]` pairs**, not a flat `{type: items}` map. Parsing was non-obvious — see `parse_network_resources` (watcher.rs:192-222) and `parse_console_resources` (watcher.rs:313-343).

## process-target-needs-getroot

**`listProcesses` exists on Firefox 87+ but older builds will return an unrecognized-type error on the root actor.** We fail gracefully (`RootActor::list_processes` returns `Err`), letting the caller fall back to the tab path.

## navigate-success-on-dns-fail

**A `navigateTo` to a bad-DNS URL returns a success-shaped reply** because Firefox successfully navigated — to `about:neterror`. The user thinks the page loaded. Detection requires inspecting the post-navigate URL or watching for an `about:neterror` target event. Helper `neterror_error_for_commit` exists but doesn't fire in the default daemon path. See [[dogfooding-session-53]] AC-F.

## tablistchanged-noise

**The root actor pushes `tabListChanged` events whenever any tab opens, closes, or navigates** — *between* request and reply for unrelated calls like `listTabs`. We skip them by the reply-vs-event filter, but before iter-54 we had a retry loop that misclassified these as "incomplete packets". See [[iteration-54-protocol-correctness]] task 2.

## fixtures-must-be-recorded

**Hand-crafted JSON test fixtures drift from reality.** `.claude/CLAUDE.md` mandates: all e2e test fixtures must be recorded from live Firefox via `crates/ff-rdp-core/tests/live_record_fixtures.rs` with `FF_RDP_LIVE_TESTS_RECORD=1`. Fixtures are auto-normalized (`conn\d+` → `conn0`). Memory: `feedback_recorded_fixtures.md`.
