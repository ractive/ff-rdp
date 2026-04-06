---
title: "Firefox RDP Protocol Deep Dive"
type: research
date: 2026-04-06
status: completed
tags: [research, protocol, rdp, firefox, documentation]
---

# Firefox RDP Protocol Deep Dive

Research from the official Firefox source docs at `firefox-source-docs.mozilla.org/devtools/backend/`.

## Protocol Format

### Framing
Length-prefixed JSON over TCP: `{decimal_length}:{utf8_json}`. Also supports bulk data packets (`bulk actor type length:data`) but these are rarely used by external clients.

### Message Structure
- **Client → Server**: `{ "to": actor, "type": method, ... }`
- **Server → Client (reply)**: `{ "from": actor, ... }`
- **Server → Client (error)**: `{ "from": actor, "error": name, "message": description }`
- **Server → Client (event)**: `{ "from": actor, "type": eventType, ... }` (unsolicited)

### Key Invariant
"At no point may either client or server send an unbounded number of packets without receiving a packet from its counterpart." This prevents flooding.

## Actor Hierarchy

```
RootActor ("root")
├── Global-scoped actors (PreferenceActor, DeviceActor, PerfActor)
├── TabDescriptorActor
│   └── WatcherActor (via getWatcher)
│       └── WindowGlobalTargetActor(s)
│           ├── WebConsoleActor
│           ├── ThreadActor (breakpoints, pause/resume/step)
│           ├── InspectorActor → WalkerActor, NodeActor
│           ├── StorageActor
│           └── AccessibilityActor
├── WorkerDescriptorActor
├── ParentProcessDescriptorActor
└── WebExtensionDescriptorActor
```

### Actor Naming Convention
IDs are dynamically assigned: `server{N}.conn{N}.child{N}/{actorType}{N}` (e.g., `server1.conn0.child2/consoleActor3`). Only the root actor has a fixed ID (`"root"`).

### Actor Lifetimes
- **Connection-scoped**: RootActor, descriptor actors
- **Target-scoped**: WebConsoleActor, ThreadActor, InspectorActor — destroyed when target closes
- **Pause-scoped**: Grip actors for paused thread inspection — destroyed when thread resumes

## Thread Actor State Machine

```
Detached ──attach──→ Paused ──resume──→ Running
                     ↑                    │
                     └──breakpoint/────────┘
                        interrupt

Paused ──detach──→ Detached
Any state ──exit──→ Exited (spontaneous)
```

### Execution Control
- `resume` — optionally with `resumeLimit`: `{type:"next"}` (step over), `{type:"step"}` (step into), `{type:"finish"}` (step out)
- `interrupt` — pause a running thread
- `clientEvaluate` — evaluate expression in a paused frame

### Pause Reasons
`attached`, `interrupted`, `resumeLimit`, `debuggerStatement`, `breakpoint`, `watchpoint`, `clientEvaluated`, `exception`

## Grip System (Value Representation)

### Primitives
Direct JSON values: `42`, `true`, `"string"`

### Special Types
`{ "type": "null" }`, `{ "type": "undefined" }`, `{ "type": "Infinity" }`, `{ "type": "-Infinity" }`, `{ "type": "NaN" }`, `{ "type": "-0" }`

### Object Grips
`{ "type": "object", "class": "Object", "actor": "server1.conn0.child2/obj123" }` — can be inspected via `prototypeAndProperties`, `property`, `ownPropertyNames`

### Function Grips
Extended object grip with `name`, `displayName`, `url`, `line`, `column`

### Long Strings
`{ "type": "longString", "initial": "first 1000 chars...", "length": 50000, "actor": "..." }` — full content via `substring(start, end)` request

### Property Descriptors
- Data: `{ enumerable, configurable, value, writable }`
- Accessor: `{ enumerable, configurable, get, set }`

### Completion Values
`{ "return": grip }`, `{ "throw": grip }`, `{ "terminated": true }`

## Watcher Architecture

The WatcherActor coordinates all observation from the parent process.

### Resource Watching
- `watchResources(resourceTypes)` — subscribe to events, resolves after existing resources notified
- Events: `resources-available-array`, `resources-updated-array`, `resources-destroyed-array`
- Resource types: `"console-message"`, `"source"`, `"stylesheet"`, `"network-event"`, etc.

### Target Watching
- `watchTargets(targetType)` — observe debuggable contexts
- Events: `target-available-form`, `target-destroyed-form`
- Target types: `"frame"`, `"worker"`, `"service_worker"`, `"shared_worker"`, `"process"`

## Backward Compatibility

### Traits System
Actors expose `traits` objects to signal feature availability. Three patterns:
1. **Form-based**: traits in actor's `form()` response
2. **Root actor traits**: via `TargetMixin::getTrait()` / `DevToolsClient.traits`
3. **`getTraits()` method**: dedicated method returning trait object

### Version Detection
- `hasActor(typeName)` — synchronous check for actor availability (since Firefox 36)
- No explicit protocol version numbers — rolling compatibility via traits

## Error Types

| Error | Meaning |
|-------|---------|
| `unrecognizedPacketType` | Unknown message type |
| `missingParameter` | Required field absent |
| `badParameterType` | Wrong parameter type |
| `noSuchActor` | Actor doesn't exist |
| `wrongState` | Invalid state for operation |
| `notReleasable` | Can't release pause-lifetime grip |
| `threadWouldRun` | Operation would execute JS (proxy/getter/setter) |
| `unknownFrame` | Frame not on current stack |
| `exited` | Thread exited |

## Source Locations

Locations can nest for eval/Function constructor contexts:
```json
{ "url": "file.js", "line": 10, "column": 5 }
{ "eval": { "url": "file.js", ... }, "id": "eval-1", "line": 3 }
```

## Object Inspection

### prototypeAndProperties
Returns `{ prototype: grip, ownProperties: { name: descriptor, ... }, safeGetterValues: {} }`

### Environment Inspection
Four types: `object`, `function`, `with`, `block` — each with `actor`, `parent` chain, and `bindings` (for function/block).
