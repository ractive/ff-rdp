---
title: "Iteration 10: Object Inspection & Native Actor Access"
type: iteration
date: 2026-04-06
status: completed
branch: iter-10/object-inspect
tags:
  - iteration
  - grips
  - inspection
  - storage
  - sources
---

# Iteration 10: Object Inspection & Native Actor Access

Add the ability to inspect remote JavaScript object grips, access httpOnly cookies via the native StorageActor, and list page sources.

## Background

An [[research/implementation-gap-analysis]] on 2026-04-06 compared our implementation against the [[research/rdp-protocol-deep-dive]] (official Firefox docs). Three high-value gaps were identified:

1. **Object grips are opaque**: When `eval` returns an object, we show `{"type":"object","class":"Object"}` but cannot drill into properties. The protocol supports `prototypeAndProperties` on any object grip actor.

2. **httpOnly cookies invisible**: Our `cookies` command uses `document.cookie` via JS eval, which cannot see httpOnly cookies. Firefox's native StorageActor can access all cookies including httpOnly ones.

3. **No source listing**: No way to see what scripts are loaded on a page. The ThreadActor's `sources` request provides this.

## Part A: Object Grip Inspection

### Design

Add an `inspect <grip_actor_id>` command that fetches properties of any object grip. When `eval` returns an object, the output already includes the grip's `actor` field — users can copy that ID into `inspect`.

Also enhance `eval` output: when the result is an Object grip, automatically include a `properties` field in the output with the top-level property names and values (shallow, one level deep).

### Tasks

- [x] Add `ObjectActor` to `ff-rdp-core/src/actors/object.rs` with:
  - `prototype_and_properties(actor_id)` → `PrototypeAndProperties`
  - `own_property_names(actor_id)` → `Vec<String>`
- [x] Add `PropertyDescriptor` type to `object.rs`:
  - Data variant: `{ value: Grip, writable: bool, enumerable: bool, configurable: bool }`
  - Accessor variant: `{ get: Option<Grip>, set: Option<Grip>, enumerable: bool, configurable: bool }`
- [x] Add `inspect` CLI command in `commands/inspect.rs`:
  - Takes actor ID as argument
  - Support `--depth <N>` for recursive inspection (default 1)
  - Tracks visited actor IDs to prevent circular reference loops
- [x] Enhance `eval` command: when result is an Object grip, auto-fetch `ownPropertyNames` and include a `"propertyNames"` field in output
- [ ] Add live test `live_prototype_and_properties` — eval `({a:1, b:[2,3]})`, fetch properties on the result grip
- [ ] Add live test `live_own_property_names` — verify property name listing
- [x] Add e2e tests for `inspect` command with mock server
- [x] Create fixtures: `prototype_and_properties_response.json`, `own_property_names_response.json`

### Acceptance Criteria

1. `ff-rdp eval "({a:1, b:'hello'})"` shows property names in the output
2. `ff-rdp inspect <actor_id>` shows full property details
3. `ff-rdp inspect <actor_id> --depth 2` shows nested object properties
4. Function grips show name/location info
5. All new code has e2e tests

## Part B: Native Cookie Access via StorageActor — NOW ITERATION 11

Protocol discovery is complete — see [[research/storage-actor-protocol]] for findings. Implementation is underway as [[iterations/iteration-11-native-cookie-access]].

## Part C: Source Listing

### Design

Add a `sources` command that lists all JavaScript/WASM sources loaded on the page. This uses the ThreadActor's `sources` request.

Access path: getTarget → threadActor → attach → sources → detach.

### Tasks

- [x] Add `ThreadActor` to `ff-rdp-core/src/actors/thread.rs` with:
  - `attach(actor_id)` → paused response
  - `sources(actor_id)` → `Vec<SourceInfo>`
  - `resume(actor_id)` → resume response (Paused → Running)
  - `detach(actor_id)` → detach response
  - `list_sources(actor_id)` → convenience: attach, sources, resume, detach with cleanup on error
  - `SourceInfo`: `actor`, `url`, `is_black_boxed`
- [x] Add `sources` CLI command in `commands/sources.rs`:
  - Output: `{ "results": [{ "url": "...", "actor": "..." }], "total": N }`
  - `--filter <substring>` — filter by URL
  - `--pattern <regex>` — filter by URL regex
- [ ] Record thread attach/sources/resume/detach from live Firefox
- [ ] Add live tests for source listing
- [x] Add e2e tests with mock fixtures
- [x] Create fixtures: `thread_attach_response.json`, `sources_response.json`, `thread_resume_response.json`, `thread_detach_response.json`

### Acceptance Criteria

1. `ff-rdp sources` lists all loaded scripts with their URLs
2. `--filter` and `--pattern` narrow results
3. ThreadActor is properly attached and detached (no leaked state)
4. Works on example.com (at least one source shown)

## Design Notes

### Object inspection depth
Recursive inspection must be bounded to prevent infinite loops (circular references). Default depth=1 (top-level properties only). Firefox handles circular references in grips by returning the same actor ID, so we track seen actor IDs and stop recursion.

### StorageActor discovery
The StorageActor protocol isn't fully documented. We'll use live Firefox recording to discover the exact protocol. If the format is too complex or unstable, Part B can be deferred to a later iteration.

### Thread attach/detach safety
The ThreadActor has a state machine (Detached → Paused → Running). We must:
1. Attach (transitions to Paused)
2. Read sources
3. Resume (transitions to Running — important: don't leave page paused!)
4. Detach

If we skip resume, the page freezes. Ensure cleanup happens even on errors.
