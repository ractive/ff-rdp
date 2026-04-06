---
title: "Implementation Gap Analysis"
type: research
date: 2026-04-06
status: completed
tags: [research, gaps, protocol, planning]
---

# Implementation Gap Analysis

Comparison of our ff-rdp implementation vs the documented Firefox RDP protocol capabilities, based on the [[rdp-protocol-deep-dive]].

## What We Have (Fully Implemented)

| Feature | Quality |
|---------|---------|
| TCP transport (length-prefixed JSON framing) | Solid |
| Connection + greeting validation | Solid |
| Root actor (listTabs) | Solid |
| Tab descriptor → getTarget | Solid |
| WebConsoleActor (evaluateJSAsync, getCachedMessages, startListeners) | Solid |
| Navigation (navigateTo, reload, goBack, goForward) | Solid |
| WatcherActor (watchResources/unwatchResources for network-event) | Solid |
| NetworkEventActor (headers, content, timings) | Solid |
| Long string handling (substring, chunked fetch) | Solid |
| Grip system (null, undefined, Infinity, NaN, -0, objects, longStrings) | Solid |
| All CLI commands (tabs, eval, dom, click, type, cookies, storage, screenshot, console, network, navigate, wait, launch) | Solid |
| jq filter pipeline | Solid |
| Performance API network mode (--cached) | Solid |
| Cross-platform Firefox launch | Solid |

## Gaps: High-Value Features Missing

### 1. **Thread/Debugger Actor — NOT IMPLEMENTED**
The ThreadActor is a core part of the protocol enabling:
- Breakpoint management (setBreakpoint, deleteBreakpoint)
- Pause/resume/step execution (attach, detach, resume, interrupt)
- Stack frame inspection (frames)
- Variable/scope inspection during pauses (bindings, prototypeAndProperties)
- Exception pause configuration (pauseOnExceptions)
- Source listing and blackboxing

**Impact**: Without this, ff-rdp cannot be used as a debugger. However, this is extremely complex and may be out of scope for the current project (which focuses on automation/inspection).

### 2. **Object Grip Inspection — IMPLEMENTED (iteration 10)**
The protocol supports inspecting remote objects via:
- `prototypeAndProperties` — get all properties of an object grip ✅
- `ownPropertyNames` — list property names ✅
- `property(name)` — get single property (not yet needed)
- `prototype` — get prototype chain (returned by prototypeAndProperties)

**Status**: `inspect` command added. `eval` now auto-enriches object results with property names.

### 3. **InspectorActor / WalkerActor / NodeActor — NOT IMPLEMENTED**
The native DOM inspection actors provide:
- Full DOM tree traversal
- Node attribute reading/modification
- CSS computed style inspection
- Element highlighting
- Box model data

**Impact**: Our current DOM command works via JS `document.querySelectorAll()` eval, which is functional but limited. Native DOM actors would provide richer capabilities (CSS styles, box model, mutation events).

### 4. **StorageActor — NOT IMPLEMENTED (using JS eval instead)**
Firefox has a native StorageActor for:
- Cookie access (including httpOnly cookies — invisible to document.cookie!)
- LocalStorage/SessionStorage
- IndexedDB
- Cache API
- Extension storage

**Impact**: Our JS eval approach cannot read httpOnly cookies. The native StorageActor can. This is a real correctness gap.

### 5. **Source Listing — IMPLEMENTED (iteration 10)**
Sources management:
- List all JavaScript/WASM sources loaded in page ✅ (via ThreadActor attach/sources/resume/detach)
- Read source content (not yet needed)
- Blackbox sources (not yet needed)

**Status**: `sources` command added with `--filter` and `--pattern` flags.

### 6. **Proper Error Protocol — PARTIALLY IMPLEMENTED**
The protocol defines structured error responses (`{ "from": actor, "error": name, "message": msg }`). We handle `ActorError` but:
- Don't distinguish all error types from the spec
- Don't handle `threadWouldRun` with its `cause` field
- Don't handle `wrongState` gracefully

### 7. **Target Watching — NOT IMPLEMENTED**
The Watcher can observe target lifecycle:
- `watchTargets("frame")` — get notified of new documents (navigations, iframes)
- `target-available-form` / `target-destroyed-form` events
- Automatic handling of target changes during navigation

**Impact**: Currently we reconnect after navigation. Proper target watching would enable seamless handling of page transitions.

### 8. **Resource Types Beyond network-event — NOT IMPLEMENTED**
WatcherActor supports many resource types beyond network events:
- `console-message` — real-time console messages
- `source` — script sources
- `stylesheet` — CSS changes
- `error-message` — page errors

**Impact**: We only watch `network-event` currently. Adding `console-message` watching would enable real-time console monitoring (vs our current snapshot approach).

## Gaps: Lower Priority

| Feature | Notes |
|---------|-------|
| Bulk data transport | For binary transfer; our JSON-only approach is fine |
| Worker debugging | WorkerDescriptor/WorkerTargetActor |
| Extension debugging | WebExtensionDescriptor |
| Process debugging | ParentProcessDescriptor (Browser Toolbox) |
| Accessibility inspection | AccessibilityActor |
| Profiler integration | PerfActor |
| Preference management | PreferenceActor |

## Recommended Next Iteration

Based on impact and complexity, the highest-value next step is **Object Grip Inspection** combined with **StorageActor adoption**:

1. **Object inspect command** — Fetch properties of grip objects via `prototypeAndProperties`
2. **StorageActor for cookies** — Read httpOnly cookies that document.cookie can't see
3. **Source listing** — List loaded scripts via sources request

These are moderate complexity (each is a single actor interaction) and provide tangible user value without the massive scope of implementing a full debugger.
