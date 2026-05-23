---
title: RDP Glossary (working definitions used by ff-rdp)
type: rdp-note
tags: [rdp, from-codebase, glossary]
date: 2026-05-23
---

# RDP Glossary

Definitions as we use them in `ff-rdp`. Cross-reference Mozilla's official glossary in the other slice of this wiki for the canonical terms.

## actor
A server-side entity on the Firefox side identified by an opaque string ID (e.g. `server1.conn0.tabDescriptor1`). All RDP communication is request/reply (or push event) between a client and a named actor. Actors form a tree rooted at the special `"root"` actor; closing a parent closes all descendants. We treat actor IDs as opaque — never parse them. See [[actors-we-use]].

## actor ID
The string identifying an actor. Format observed: `server<N>.conn<N>.<actorClassN>` or, inside child processes, `server<N>.conn<N>.child<N>/<actorClass><N>`. Wrapped in our code as `crate::types::ActorId` (a newtype around `String` for type-safety). Test fixtures normalize `conn\d+` to `conn0` for stability.

## packet (frame)
A single length-prefixed JSON message: `{N}:{json}` where `N` is the ASCII decimal byte length of the JSON payload and `:` is the delimiter. We cap declared frame size at 64 MiB via `MAX_FRAME_BYTES` (`transport.rs:15`) to prevent OOM from a malformed peer. See [[lessons-learned#frame-size-cap]].

## bulk frame
Alternate framing for binary data: `bulk <actor_id> <type> <size>:<bytes>`. We don't currently send or receive bulk frames — everything goes through length-prefixed JSON.

## transport
The TCP byte stream + JSON-framing pair. Our impl: `crate::transport::RdpTransport` wraps `(BufReader<TcpStream>, TcpStream)` with `send(&Value)` / `recv() -> Value` methods. Default Firefox port: 6000.

## connection
A higher-level wrapper around a transport: handles the greeting handshake (first packet from `root` after TCP connect), basic state. See `crate::connection::RdpConnection`.

## handshake / greeting
The unsolicited packet the root actor sends as soon as a client connects: `{"from": "root", "applicationType": "browser", "traits": {...}}`. There is no client-initiated handshake — clients just connect and read this first.

## request
A packet sent by us to a named actor: `{"to": "<actor_id>", "type": "<method>", ...args}`. The `type` here is the method name (different sense from server-side `type` in pushes — see [[lessons-learned#reply-vs-event]]).

## reply
The server's response to a request. Comes back as `{"from": "<actor_id>", ...result_fields}` **with no `type` field** (per the no-type-means-reply convention — though see ThreadActor caveat in [[lessons-learned#reply-vs-event]]).

## push event
An unsolicited packet from an actor: `{"from": "<actor_id>", "type": "<event_name>", ...event_data}`. Examples: `tabListChanged` (root), `consoleAPICall` (console), `tabNavigated` / `willNavigate` (target/console), `target-available-form` (watcher), `resources-available-array` (watcher), `evaluationResult` (console).

## error reply
`{"from": "<actor>", "error": "<error_name>", "message": "<description>"}`. We map these into `ProtocolError::ActorError { actor, kind, error, message }` with `ActorErrorKind::from_code` for known codes.

## grip
A protocol-level wrapper around a JavaScript value. Simple types (`string`, `number`, `bool`, `null`) pass through inline. Complex types use object wrappers:
- **Object grip**: `{type:"object", class:"...", actor:"<id>", preview:{...}, ...}` — references a server-side `ObjectActor`.
- **Function grip**: object grip with `class:"Function"` plus `name`, `url`, `line`.
- **LongString grip**: `{type:"longString", actor:"<id>", initial:"first ~8KB", length:N}` — fetched in chunks via `LongStringActor.substring`.
- **Special values**: `{type:"null"}`, `{type:"undefined"}`, `{type:"NaN"}`, `{type:"Infinity"}`, `{type:"-Infinity"}`.

In our code: `crate::types::Grip` enum, parsed by `Grip::from_result_value`. Grips holding actor references *leak server-side actors unless released* — see [[lessons-learned#actor-leaks]].

## grip lifetime
- **Pause-lifetime**: Valid only while the thread is paused (debugger context).
- **Thread-lifetime**: Persist until explicitly released via `releaseActor` / `ObjectActor::release`.

## scoped grip
Our RAII wrapper `ScopedGrip` (iter-54 task 4) that releases the underlying actor on consumption. Building block; not yet wired into daemon call sites.

## watcher
The `WatcherActor`, obtained via `TabDescriptor.getWatcher()`. Manages resource subscriptions across all targets under that tab. Replaces older per-actor `startListeners` for `network-event`, `console-message`, `error-message`, `cookies`, etc. See [[actors-we-use#watcheractor]].

## resource
A typed event stream the Watcher subscribes us to. Each resource type has its own wire shape, all packed into `resources-available-array` / `resources-updated-array` events as `array: [[type-name, [items...]]]`. Types we use: `"network-event"`, `"console-message"`, `"error-message"`, `"cookies"`.

## resource ID
A monotonic numeric ID assigned by Firefox to each resource (e.g. a network request). Used to correlate `resources-updated-array` entries with the originally-seen `resources-available-array` entry. We parse this as `u64`; items missing it are skipped (not silently mapped to 0).

## target / target actor
The actor that represents an executable JS scope: `WindowGlobalTargetActor` for a document, `WorkerTargetActor` for a worker. Holds the per-document `consoleActor`, `inspectorActor`, `threadActor`, `screenshotContentActor`, `accessibilityActor`, `responsiveActor` IDs.

## target descriptor
A higher-level actor representing a *thing that has a target* (e.g. tab, worker, process). You call `getTarget` on a descriptor to obtain the current target — which may change on navigation. **Tab descriptors wrap the result in `"frame"`; process descriptors wrap it in `"process"`** — see [[lessons-learned#descriptor-wrappers]].

## browsing context
Firefox's term for "tab or iframe": a navigable container with its own document. Identified by a `browsingContextID` (u64). Required for the Firefox 149+ two-step screenshot API (`screenshotActor.capture` takes it as input). We capture it on both `TabInfo` and `TargetInfo`.

## descriptor wrapping
The discovery that the same `getTarget` RPC returns different envelope shapes depending on the descriptor type. Tab descriptors: `{frame: {...}}`. Process descriptors: `{process: {...}}`. Handle both.

## chrome context / chrome scope
A privileged JavaScript context inside Firefox that isn't subject to page CSP and can access `Cu`, `Cc`, `Ci`, `Services`, etc. We opt into it by setting `"chromeContext": true` in `evaluateJSAsync`. Used for CSP-bypassing eval (`evaluate_js_async_chrome`) and for the headless screenshot fallback (iter-61h). Distinct from the *content* context, which is the page's own JS sandbox.

## content scope / content context
The page's own JavaScript execution context. Subject to page CSP and the same-origin policy. Default for `evaluateJSAsync`.

## CSP (Content Security Policy)
HTTP header (`Content-Security-Policy: script-src ...`) that the page sends to restrict where JS may come from and whether `eval()` may run. Pages with strict CSP (HN, lit.dev) reject our `evaluateJSAsync` with `EvalError: call to eval() blocked by CSP` unless we use chrome-context.

## about:neterror
Firefox's built-in error page shown for DNS failures, connection refused, cert errors, etc. From an RDP POV the navigate *succeeded* — Firefox really did navigate to this page — but our caller wants to see it as a failure. about:neterror itself has a restrictive CSP that blocks eval. See [[lessons-learned#about-neterror-csp]].

## listener
A per-actor event subscription set up via `startListeners({listeners: [...]})`. Pre-watcher pattern, still wired alongside watcher for console (`PageError`, `ConsoleAPI`) — see [[open-gaps#legacy-startlisteners-coexistence]].

## resource update / update event
A `resources-updated-array` event carrying *deltas* on an already-known resource (matched by `resourceId`). For network: status, mimeType, totalTime, contentSize, transferredSize, fromCache, remoteAddress, securityState arrive *after* the initial `resources-available-array`.

## ref / ref ID
`ff-rdp`'s own concept (iter-60) — short, stable element identifiers (`e1`, `e2`, …) emitted by `dom`/`snapshot` so subsequent commands can refer to nodes without long actor IDs. Not part of the wire protocol. `refs_registered: false` in command output means the daemon didn't persist them.

## daemon mode
`ff-rdp daemon` keeps a single RDP connection open across multiple CLI invocations to avoid the connect-handshake-discover overhead. Architectural design: [[connection-persistence]], [[gradle-daemon-architecture]]. Has its own subtleties around stale `consoleActor` IDs after navigation, watcher buffering, and grip leaks.

## fixture
A recorded RDP packet exchange used as a test input. Per `.claude/CLAUDE.md`: must be recorded from real Firefox via `crates/ff-rdp-core/tests/live_record_fixtures.rs` with `FF_RDP_LIVE_TESTS_RECORD=1`. Stored in `tests/fixtures/`. Auto-normalized (`conn\d+` → `conn0`).
