---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- object
date: 2026-05-24
firefox_files:
- devtools/shared/specs/object.js
- devtools/server/actors/object.js
title: ObjectActor
---

# ObjectActor

Represents a live JavaScript object on the server side. Allows the client to
inspect properties, prototypes, and internal slots of remote objects returned
by the JavaScript debugger or console evaluations.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/object.js` | 1-222 | Protocol spec — property grips, front forms |
| `devtools/server/actors/object.js` | 1-820 | Server implementation |

## Key methods (from spec)

- `prototypeAndProperties()` — returns the prototype grip and own property descriptors.
- `prototype()` — returns just the prototype grip.
- `property(name)` — returns a single property descriptor.
- `release()` — releases the actor reference (release method).

## Status

Protocol overview backfilled in iter-73; the current Rust API boundary is recorded below.

## Iteration 287 — legacy property-names API retirement (2026-09-28)

The Rust `ObjectActor::own_property_names` method has been removed. It sent
`ownPropertyNames`, which is absent from the qualified Firefox 156.0.1 object
spec and server. There were no repository callers. `eval` already uses
`prototype_and_properties` to derive `propertyNames`, following the completed
[[iteration-18-dogfooding-fixes]] repair; `inspect` uses the same supported
operation. Their product behavior is unchanged by this removal.

This is a **breaking Rust library API change**: `ObjectActor` is publicly
reexported from `ff-rdp-core`, and external consumers are unknown. Source that
calls the removed method must be adapted. The next published crate version
containing this removal must use a breaking pre-1.0 minor version (0.4.0 or a
later appropriate minor), not a compatible 0.3.x patch. Iteration 287 does not
bump package versions or publish crates.

For callers that want the existing descriptor map's string keys, the explicit
migration is:

```rust
let properties = ObjectActor::prototype_and_properties(transport, actor_id)?;
let names: Vec<String> = properties.own_properties.into_keys().collect();
```

This selects the existing API's semantics: keys come from parsed `ownProperties`
descriptors and are returned in Rust `BTreeMap` lexicographic string order,
not JavaScript enumeration order. Separate `ownSymbols` and `safeGetterValues`
response fields are not included; non-object descriptors are skipped by the
existing parser. This is not a compatibility substitute promising the old
method's inclusion or ordering behavior. No new property operation is introduced.

See [[iteration-287-legacy-object-property-names-api]] for qualified source
identity and validation status. The old unused property-names fixture remains
historical data, not evidence that the packet is supported.

## Iter-76 update — grip release on Drop

- `ScopedGrip<K: GripKind>` (markers `ObjectGrip`, `LongStringGrip`) wraps an actor ID and a release-queue handle. Drop enqueues a `ReleaseRequest { actor_id, method }` rather than sending from the destructor (avoids re-entrant transport calls; same rationale as iter-71 `gc()`).
- The release queue is drained by the demux reader in daemon mode and by the next `actor_request` in synchronous mode.
- For long-string actors the release method is `release` per `devtools/server/actors/string.js`; for object actors it is `release` per `devtools/shared/specs/object.js:213`.

## Reset 2026-10 update — release queue removed

The Drop-driven release queue above (`release_queue`, `ReleaseQueueTx/Rx`,
`ReleaseRequest`, the `ObjectGrip` marker and `ResourceGripGuard`) only had a
consumer in the daemon, which DEC-056 removed. It was deleted in phase 4b.
What remains: `GripHandle<LongStringGrip>` with `without_queue()` +
explicit `release(transport)` (used by `navigate` for long-string bodies), and
the non-generic `ScopedGrip` used by `eval` for its explicit post-print
release. Nothing is released from a destructor any more.
