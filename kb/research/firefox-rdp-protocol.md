---
title: Firefox Remote Debugging Protocol Specification
type: research
date: 2026-04-06
tags: [protocol, firefox, rdp, research]
status: completed
---

# Firefox Remote Debugging Protocol

## Overview

The Firefox Remote Debugging Protocol (RDP) enables debugger clients to connect to Firefox and inspect/control JavaScript execution, DOM, CSS, network, and more. All communication is JSON-based over a reliable byte stream.

Official docs: [[https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html]]

## Transport Layer

### Wire Format

Length-prefixed JSON over TCP:

```
{length}:{json_payload}
```

- `length` is ASCII decimal digits
- `:` delimiter (0x3A)
- JSON payload is UTF-8 encoded
- Example: `36:{"to":"root","type":"listTabs"}`

### Bulk Data Format

For large binary data (screenshots, heap snapshots):

```
bulk {actor_id} {type} {size}:{binary_data}
```

### Connection

- Default: TCP on `localhost:6000`
- Start Firefox: `firefox --start-debugger-server 6000`
- WebSocket mode: `firefox --start-debugger-server ws:6000`
- Headless: `firefox --start-debugger-server 6000 -headless`

No authentication mechanism. Security relies on localhost binding.

## Message Format

### Client-to-Server (Request)

```json
{"to": "<actor_id>", "type": "<method_name>", ...additional_fields}
```

### Server-to-Client (Response)

```json
{"from": "<actor_id>", ...result_fields}
```

### Error Response

```json
{"from": "<actor_id>", "error": "<error_name>", "message": "<description>"}
```

### Communication Patterns

1. **Request/Reply**: Each request gets exactly one response
2. **Events**: Unsolicited notifications from server (e.g., `consoleAPICall`, `networkEvent`)

### Correlation

The protocol does NOT use request IDs. Instead:
- Responses are processed sequentially
- `from` field identifies the sending actor
- Events are distinguished by presence of `type` field in response
- Listeners are registered per actor ID + event type

## Actor Model

### Core Concept

An actor is a server-side entity that can exchange JSON packets. Every actor has a unique string ID (no spaces or colons). Actors form a tree rooted at the `root` actor. Closing a parent closes all descendants.

### Actor Hierarchy

```
RootActor ("root")
├── Global Actors (PreferenceActor, DeviceActor, PerfActor)
├── Descriptor Actors
│   ├── TabDescriptorActor (one per tab)
│   │   ├── WatcherActor
│   │   └── Target Actors
│   │       ├── WindowGlobalTargetActor
│   │       │   ├── WebConsoleActor
│   │       │   ├── InspectorActor
│   │       │   │   └── WalkerActor
│   │       │   │       └── NodeActor(s)
│   │       │   ├── ThreadActor
│   │       │   ├── StorageActor
│   │       │   └── AccessibilityActor
│   │       └── Worker Target Actors
│   ├── WorkerDescriptorActor
│   ├── ParentProcessDescriptorActor
│   └── WebExtensionDescriptorActor
```

### Root Actor

Fixed ID: `"root"`. On connection, sends handshake:

```json
{"from": "root", "applicationType": "browser", "traits": {}}
```

Key methods:
- `listTabs` → `{tabs: [{actor, title, url, selected, browsingContextID}], selected: <index>}`
- `getTab` (by browserId) → tab descriptor
- `getRoot` → root metadata with device/addons/preferences actor IDs
- `listProcesses`, `listAddons`, `listWorkers`

### Tab Descriptor Actor

Represents a browser tab. Methods:
- `getTarget` → WindowGlobalTargetActor with consoleActor, threadActor, inspectorActor IDs
- `getWatcher` → WatcherActor for resource monitoring
- `getFavicon` → tab favicon data

### WindowGlobal Target Actor

Represents a specific document. Methods:
- `navigateTo({url})` → navigate the tab
- `reload` → refresh page
- `goBack`, `goForward` → history navigation
- `focus` → bring tab to front
- `listFrames` → enumerate iframes
- `detach` → disconnect from target

### WebConsole Actor

JavaScript evaluation and console monitoring.

**evaluateJSAsync**:
```json
{
  "to": "<console_actor>",
  "type": "evaluateJSAsync",
  "text": "document.title",
  "eager": false
}
```

Response arrives as an `evaluationResult` event:
```json
{
  "from": "<console_actor>",
  "type": "evaluationResult",
  "resultID": "<id>",
  "result": {"type": "object", "class": "HTMLDocument", "actor": "<grip_actor>"},
  "timestamp": 1347306273605,
  "exception": null
}
```

**startListeners**:
```json
{"to": "<console_actor>", "type": "startListeners", "listeners": ["PageError", "ConsoleAPI"]}
```

**getCachedMessages**:
```json
{"to": "<console_actor>", "type": "getCachedMessages", "messageTypes": ["PageError", "ConsoleAPI"]}
```

Console events:
- `pageError` — JS errors with errorMessage, sourceName, lineNumber, category
- `consoleAPICall` — console.log/warn/error with level, arguments, filename, lineNumber

### Watcher Actor

Monitors resources across targets. Key for network + console monitoring.

**watchResources**:
```json
{
  "to": "<watcher_actor>",
  "type": "watchResources",
  "resourceTypes": ["network-event", "console-message", "error-message", "document-event"]
}
```

Emits `resource-available-form` events:
```json
{
  "type": "resource-available-form",
  "from": "<watcher_actor>",
  "array": [["network-event"], [{...event_data}]]
}
```

**watchTargets**:
```json
{"to": "<watcher_actor>", "type": "watchTargets", "targetType": "frame"}
```

Also provides: `getNetworkParentActor`, `getTargetConfigurationActor`, `getThreadConfigurationActor`

### Network Event Actor

Individual network requests. Available after `watchResources(["network-event"])`.

Methods for each request:
- `getRequestHeaders` → headers array
- `getResponseHeaders` → response headers
- `getRequestCookies` / `getResponseCookies`
- `getRequestPostData` → POST body
- `getResponseContent` → response body
- `getEventTimings` → timing breakdown
- `getSecurityInfo` → TLS details

Network event data:
```json
{
  "actor": "<net_event_actor>",
  "url": "https://example.com/api",
  "method": "GET",
  "isXHR": false,
  "cause": {"type": "document"},
  "startedDateTime": "2025-01-01T00:00:00.000Z"
}
```

### Inspector + Walker Actors

DOM inspection:
- `getWalker` → WalkerActor for DOM traversal
- `querySelector(node, selector)` → matching NodeActor
- `querySelectorAll(node, selector)` → NodeListActor
- `document()` → root document node

NodeActor methods:
- `getNodeValue` / `setNodeValue`
- `getUniqueSelector` / `getCssPath` / `getXPath`
- `scrollIntoView`
- `modifyAttributes`

### Thread Actor

Debugger control:
- `attach` / `detach`
- `resume` (with optional `resumeLimit`: `next`, `step`, `finish`)
- `interrupt`
- `frames` → call stack with variables
- `sources` → loaded JS sources
- `setBreakpoint({location: {url, line, column}})`
- `skipBreakpoints`

### Source Actor

- `source` → retrieve source text
- `blackbox` / `unblackbox` → skip during debugging

## Grip Types

Grips represent JavaScript values in protocol messages.

Simple values pass through directly: strings, numbers, booleans.

Complex values use wrappers:

| Type | Format |
|------|--------|
| Object | `{"type": "object", "class": "ClassName", "actor": "<id>"}` |
| Function | `{"type": "object", "class": "Function", "actor": "<id>", "name": "fn", "url": "...", "line": N}` |
| Long string | `{"type": "longString", "initial": "first chars...", "length": N, "actor": "<id>"}` |
| null | `{"type": "null"}` |
| undefined | `{"type": "undefined"}` |
| NaN | `{"type": "NaN"}` |
| Infinity | `{"type": "Infinity"}` or `{"type": "-Infinity"}` |

Grip lifetimes:
- **Pause-lifetime**: Valid only while thread is paused
- **Thread-lifetime**: Persist until explicitly released

## Actor Spec System (protocol.js)

Firefox defines actors using a spec/implementation split:
- Specs in `devtools/shared/specs/*.js` using `generateActorSpec()`
- Implementations in `devtools/server/actors/*.js`
- Type system: `Arg(index, "type")`, `RetVal("type")`, arrays (`"array:type"`), nullable (`"nullable:type"`)
- Source browsable at: https://searchfox.org/mozilla-central/source/devtools/server/actors/

## Key Protocol Facts for Implementation

1. **Sequential**: No concurrent request pipelining — wait for response before next request
2. **No request IDs**: Correlation by actor ID in `from` field
3. **Events interleaved**: Async events (console, network) arrive between request/response pairs
4. **Actor IDs are opaque strings**: Format like `server1.conn0.tabDescriptor1` but don't parse them
5. **No formal JSON schema**: Must discover actor capabilities from source code or reference implementations
6. **Handshake is implicit**: First message received after TCP connect is root actor's greeting
