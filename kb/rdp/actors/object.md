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

- `prototypeAndProperties()` — returns the prototype chain and own property descriptors.
- `prototype()` — returns just the prototype grip.
- `property(name)` — returns a single property descriptor.
- `release()` — releases the actor reference (release method).

## Status

Stub — backfilled in iter-73; expand on next touch.

## Iter-76 update — grip release on Drop

- `ScopedGrip<K: GripKind>` (markers `ObjectGrip`, `LongStringGrip`) wraps an actor ID and a release-queue handle. Drop enqueues a `ReleaseRequest { actor_id, method }` rather than sending from the destructor (avoids re-entrant transport calls; same rationale as iter-71 `gc()`).
- The release queue is drained by the demux reader in daemon mode and by the next `actor_request` in synchronous mode.
- For long-string actors the release method is `release` per `devtools/server/actors/string.js`; for object actors it is `release` per `devtools/shared/specs/object.js:213`.

## Iteration259: deferred release connection identity

`GripHandle<K>` now carries an optional `ConnectionKey`. `with_origin` preserves
that key when Drop enqueues `ReleaseRequest { origin, actor_id, method }`;
Debug includes the origin. Object and long-string releases keep their existing
method and best-effort queue behavior. Origin-free direct/library constructors
remain available; a daemon release without an origin is refused.

The daemon sends a queued release only through its recorded primary/lazy
connection generation, never through a replacement or fallback primary writer.
It installs an actor-specific terminal reply sink before the release bytes and
awaits no release ACK. Conflicting true in-flight work refuses the release and
retires the origin; it is not erased by the sink. An already retired origin is
left to connection teardown. These are origin/reply-safety rules, not proof of
final-consumer lifetime or lossless queues: iteration266's lifetime, extraction
and queue-pressure obligations remain open. Direct Firefox wire packets are
unchanged.

### Unused legacy helper — pending287

Installed156.0.1 has no ownPropertyNames method. The public Rust
ObjectActor::own_property_names helper remains but has no in-repository caller;
eval/inspect use prototypeAndProperties. Iteration18 already repaired the old
eval callsite. [[iteration-287-legacy-object-property-names-api]] owns the remaining
API compatibility/cleanup decision; this is not a new current CLI failure.
