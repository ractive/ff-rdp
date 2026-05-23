---
type: rdp-note
tags: [rdp, firefox-client, protocol-framework]
date: 2026-05-23
firefox_files:
  - devtools/shared/protocol.js
  - devtools/shared/protocol/types.js
  - devtools/shared/protocol/Front.js
  - devtools/shared/protocol/Actor.js
  - devtools/shared/protocol/Request.js
  - devtools/shared/protocol/Response.js
  - devtools/shared/protocol/Actor/generateActorSpec.js
  - devtools/shared/protocol/Front/FrontClassWithSpec.js
  - devtools/shared/specs/*.js
---

# Spec/Front Framework

DevTools never writes RDP packets by hand. Each server actor has a matching
**spec** in `devtools/shared/specs/` that declares its methods, events, and
typed signatures. The spec is consumed twice:

- On the **server**, `Actor` + `generateActorSpec` (see
  `protocol/Actor.js`, `protocol/Actor/generateActorSpec.js`) generates the
  packet-handling boilerplate around the actor's JS method implementations.
- On the **client**, `FrontClassWithSpec(spec)` (`protocol/Front/FrontClassWithSpec.js:182`)
  generates a `Front` subclass whose JS methods serialize args, send a packet,
  and unmarshal the reply.

The result is symmetric: a client calls `front.foo(bar)` and the actor's
`foo(bar)` method runs on the server with marshalled args. RDP itself is just
the wire format between them.

**Why this matters for ff-rdp:** we don't run this framework — we craft RDP
JSON packets directly. But the spec files **are** the wire contract. When in
doubt about packet shape, read the spec, not random console captures.

## Type placeholders: `Arg`, `Option`, `RetVal`

From `protocol/Request.js` and `Response.js`:

- `Arg(index, type)` — positional arg, gets its own packet key.
- `Option(index, type)` — like `Arg`, but reads a *named property* off
  `arguments[index]` and hoists it into the packet. Letting client code call
  `front.foo({a, b, c})` while the wire sees `{type:"foo", a, b, c}`.
- `RetVal(type)` — single return value: the named response field is unmarshalled
  back into the JS return value.

`Option` is what makes `evaluateJSAsync({ text, frameActor, ... })` work — see
`specs/webconsole.js:149-164`. All those `Option(0, "string")` declarations mean
*"caller passes one object, each key becomes a top-level packet field"*.

## Type system (`protocol/types.js`)

Built-ins: `string`, `boolean`, `number`, `json`, `array:T`, `nullable:T`,
`longstring` (lazy-streamed strings via a `LongStringFront`).

Actor types: every actor's typeName (e.g. `"webconsole"`, `"target"`,
`"screenshot"`) is registered automatically. A `RetVal("targetDescriptor")`
declaration causes the wire form `{actor: "server1.conn0.tabDescriptor3"}` to
be marshalled into a `TabDescriptorFront` instance on the client (with the
correct actorID set, and registered in the connection's actor pool).

Dict types: `types.addDictType("screenshot.args", {fullpage: "nullable:boolean", ...})`
groups a recursive object schema under a name — used for nested arg/response
records.

Specials worth noting:

- `nullable:dom-node` — used widely in walker/inspector specs; coerces to a
  `NodeFront` (or null) on the client.
- `array:targetDescriptor` — `listTabs` returns `Arg(0, "array:tabDescriptor")`,
  so the client gets back a JS array of `TabDescriptorFront` objects, each
  already attached to the connection's pool. See `specs/root.js:38-43`.
- `grip` — short, one-shot description of a debuggee JS value used by the
  console. Sometimes a primitive, sometimes `{type: "object", actor: "..."}` —
  the client wraps the latter in an `ObjectFront`.

## `Event(...)` declarations

A spec's `events: {}` block declares which unsolicited server packets the
Front should re-emit as JS events. Example from `specs/watcher.js:108-115`:

```js
"resources-available-array": {
  type: "resources-available-array",       // packet `type` field
  array: Arg(0, "array:json"),             // packet `array` field -> arg 0
}
```

When a packet `{from: <watcher>, type: "resources-available-array", array: [...]}` arrives,
the Front calls `this.emit("resources-available-array", arrayValue)`. Subscribers
attach via `watcherFront.on("resources-available-array", handler)`.

The framework also lets specs rename: `serverNetworkEvent: {type: "networkEvent", ...}`
in `specs/webconsole.js:87-90` receives wire-type `networkEvent` but re-emits
under a different name to avoid colliding with another listener.

## Front creation lifecycle

1. Client receives some response whose return type is an actor type.
2. `Front` framework looks up that actorID in the connection's pool.
3. If not present, instantiate the appropriate `FrontClassWithSpec` subclass
   (via `getFront(typeName)` or via response auto-unmarshalling).
4. Front fires `initialize()` once, manages its child fronts via `Pool`.

`Front.destroy()` rejects all pending requests on it and unregisters from the
pool — used both on explicit `detach` and on connection close.

See [[devtools-client]] for the `DevToolsClient` glue.
