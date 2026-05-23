---
type: rdp-note
tags: [rdp, firefox-client]
date: 2026-05-23
firefox_files:
  - devtools/client/devtools-client.js
  - devtools/shared/protocol.js
  - devtools/client/fronts/root.js
  - devtools/client/fronts/descriptors/tab.js
  - devtools/client/fronts/targets/browsing-context.js
---

# DevToolsClient + RootFront

`DevToolsClient` (`devtools/client/devtools-client.js`) is the top-level client
object. One per RDP connection. It owns:

- the transport (`this._transport`),
- pending requests indexed by actorID (`this._pendingRequests`, `this._activeRequests`),
- the actor pool / Front registry (via the Pool base class),
- the `mainRoot` Front — the entrypoint to *everything*.

## Construction & handshake

`new DevToolsClient(transport)` (`devtools-client.js:75-121`):

1. Saves the transport, sets `this._transport.hooks = this`.
2. Registers a one-shot `expectReply("root", callback)` before opening the
   stream — this waits for the server's greeting JSON packet from actor
   `"root"`.
3. When the greeting arrives, the callback runs:
   - `this.mainRoot = createRootFront(this, packet)` — builds the `RootFront`
     pre-populated with `traits` from the greeting.
   - Calls `this.mainRoot.connect({ frontendVersion: ... })` — Fx 133+ tells
     the server what the client claims to be.
   - Emits `"connected"`.

`client.connect()` (`devtools-client.js:146-156`) is the public API: returns a
Promise resolved after `"connected"` fires. It starts the transport via
`this._transport.ready()` *after* hooks are wired so no packets get lost.

## `RootFront`

Spec: `devtools/shared/specs/root.js`. Methods include:

- `listTabs()` → `array:tabDescriptor`
- `getTab({ browserId })` → `tabDescriptor`
- `listAddons({ iconDataURL })` → `array:webExtensionDescriptor`
- `listWorkers()`, `listServiceWorkerRegistrations()`, `listProcesses()`
- `getProcess(id)` → `processDescriptor` (process 0 = parent process)
- `watchResources([types])` — root-scoped resource watching
- `getFront("screenshot")` → the parent-process `ScreenshotFront`

Events: `tabListChanged`, `workerListChanged`, `addonListChanged`,
`processListChanged`, `resources-available-array`,
`resources-destroyed-array`.

`RootFront` is the singleton actor — it never goes away until the connection
closes. Everything reachable on the server is reached *through it*.

## Descriptor → Target → Child Fronts

The 2021-era refactor split things in two:

- **Descriptor fronts** (e.g. `TabDescriptorFront`, `ProcessDescriptorFront`,
  `WebExtensionDescriptorFront`) are returned by Root list-methods. They are
  cheap metadata wrappers (URL, title, browserId, ...) and a handle to get
  the actual `TargetFront`.
- `descriptor.getTarget()` lazily creates and attaches the
  **`BrowsingContextTargetFront`** (or `WorkerTargetFront`, etc.).
  Once attached, the target exposes child fronts: `console`, `inspector`,
  `thread`, `responsive`, `accessibility`, `screenshotContent`, etc., via
  `target.getFront(typeName)`.
- For tab targets there's also a **`WatcherFront`** (got via
  `descriptor.getWatcher()`) used for cross-target resource subscription.

See [[../flows/connect-and-list-tabs]] and [[../flows/attach-target]].

## Request lifecycle

`client.request(packet)` (`devtools-client.js:238-293`) is the low-level send:

1. Validates `packet.to` is set, otherwise throws.
2. If transport already closed: returns `{error:"connectionClosed"}`.
3. Creates a `Request` object, queues it under `packet.to`.
4. Returns a thenable that resolves on the next packet `from: packet.to` —
   or rejects if that packet has an `error` field.

Per-actor request **serialization**: only one in-flight request per actor at a
time (RDP rule). Additional requests for the same actor are queued until the
prior reply arrives. This is why long-running operations (notably
`evaluateJSAsync` — see [[../flows/evaluate-js]]) have a deferred-response
pattern: the immediate reply is just `{resultID}`, the actual result comes
later via an event so other console requests aren't blocked.

The framework code in `protocol/Front.js` wraps this lower-level API: a Front
method invocation runs `Request.write()` to build the packet, calls
`client.request()`, and runs `Response.read()` to unmarshal the reply.

## Public side-effect methods

- `attachConsole`, `attachTab`, etc. — older convenience helpers, mostly
  superseded by `descriptor.getTarget()`.
- `close()` — closes transport, rejects all pendings, emits `"closed"`.
- `purgeRequests(prefix)` — used when an actor pool is torn down: rejects all
  in-flight requests whose `actorID` starts with the given prefix.
