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
