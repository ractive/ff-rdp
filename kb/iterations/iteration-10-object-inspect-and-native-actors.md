---
title: "Iteration 10: Object Inspection & Native Actor Access"
type: iteration
date: 2026-04-06
status: planned
branch: iter-10/object-inspect
tags: [iteration, grips, inspection, storage, sources]
---

# Iteration 10: Object Inspection & Native Actor Access

Add the ability to inspect remote JavaScript object grips, access httpOnly cookies via the native StorageActor, and list page sources.

## Background

An [[implementation-gap-analysis]] on 2026-04-06 compared our implementation against the [[rdp-protocol-deep-dive]] (official Firefox docs). Three high-value gaps were identified:

1. **Object grips are opaque**: When `eval` returns an object, we show `{"type":"object","class":"Object"}` but cannot drill into properties. The protocol supports `prototypeAndProperties` on any object grip actor.

2. **httpOnly cookies invisible**: Our `cookies` command uses `document.cookie` via JS eval, which cannot see httpOnly cookies. Firefox's native StorageActor can access all cookies including httpOnly ones.

3. **No source listing**: No way to see what scripts are loaded on a page. The ThreadActor's `sources` request provides this.

## Part A: Object Grip Inspection

### Design

Add an `inspect <grip_actor_id>` command that fetches properties of any object grip. When `eval` returns an object, the output already includes the grip's `actor` field — users can copy that ID into `inspect`.

Also enhance `eval` output: when the result is an Object grip, automatically include a `properties` field in the output with the top-level property names and values (shallow, one level deep).

### Tasks

- [ ] Add `ObjectActor` to `ff-rdp-core/src/actors/` with:
  - `prototype_and_properties(actor_id)` → `{ prototype: Value, own_properties: Map<String, PropertyDescriptor> }`
  - `property(actor_id, name)` → `PropertyDescriptor`
  - `own_property_names(actor_id)` → `Vec<String>`
- [ ] Add `PropertyDescriptor` type to `types.rs`:
  - Data variant: `{ value: Grip, writable: bool, enumerable: bool, configurable: bool }`
  - Accessor variant: `{ get: Option<Grip>, set: Option<Grip>, enumerable: bool, configurable: bool }`
- [ ] Add `inspect` CLI command in `commands/inspect.rs`:
  - Takes actor ID as argument
  - Outputs JSON: `{ "class": "Object", "prototype": ..., "properties": { name: value, ... } }`
  - Support `--depth <N>` for recursive inspection (default 1)
  - Handle function grips (show name, url, line)
- [ ] Enhance `eval` command: when result is an Object grip, auto-fetch `ownPropertyNames` and include a `"propertyNames"` field in output
- [ ] Add live test `live_prototype_and_properties` — eval `({a:1, b:[2,3]})`, fetch properties on the result grip
- [ ] Add live test `live_own_property_names` — verify property name listing
- [ ] Add e2e tests for `inspect` command with mock server
- [ ] Record fixtures: `prototype_and_properties_response.json`, `own_property_names_response.json`

### Acceptance Criteria

1. `ff-rdp eval "({a:1, b:'hello'})"` shows property names in the output
2. `ff-rdp inspect <actor_id>` shows full property details
3. `ff-rdp inspect <actor_id> --depth 2` shows nested object properties
4. Function grips show name/location info
5. All new code has e2e tests

## Part B: Native Cookie Access via StorageActor

### Design

Firefox's StorageActor provides access to all cookies, including httpOnly ones that `document.cookie` cannot see. Replace the JS eval approach for cookies with native actor access.

The StorageActor is target-scoped. Access path: getTarget → frame → StorageActor ID.

**Note**: The StorageActor protocol is not fully documented in the official docs, so we'll need to discover the exact message format via live recording. The general pattern is:
- `getStoreObjects(host)` → returns cookie data
- Cookie objects include: name, value, path, domain, expires, httpOnly, secure, sameSite

### Tasks

- [ ] Record StorageActor protocol messages from live Firefox:
  - Discover the actual StorageActor method names and response formats
  - Record fixture: `storage_actor_cookies_response.json`
- [ ] Add `StorageActor` to `ff-rdp-core/src/actors/storage.rs`:
  - Parse cookie response into `Vec<Cookie>` struct
  - `Cookie`: `name`, `value`, `path`, `domain`, `expires`, `http_only`, `secure`, `same_site`
- [ ] Update `cookies` command to use StorageActor instead of JS eval:
  - Fall back to JS eval if StorageActor is not available (older Firefox)
  - Add `--http-only` flag to show only httpOnly cookies
  - Add `--secure` flag to show only secure cookies
  - Add `--domain <domain>` flag to filter by domain
- [ ] Update live recording tests for cookies
- [ ] Add e2e tests with StorageActor mock fixtures
- [ ] Mark old JS-eval cookie fixtures as deprecated (keep for fallback tests)

### Acceptance Criteria

1. `ff-rdp cookies` shows httpOnly cookies that the old command missed
2. `--http-only` flag works correctly
3. Falls back to JS eval on older Firefox without StorageActor error
4. Cookie response includes all fields: name, value, path, domain, expires, httpOnly, secure, sameSite

## Part C: Source Listing

### Design

Add a `sources` command that lists all JavaScript/WASM sources loaded on the page. This uses the ThreadActor's `sources` request.

Access path: getTarget → threadActor → attach → sources → detach.

### Tasks

- [ ] Add `ThreadActor` to `ff-rdp-core/src/actors/thread.rs` with minimal methods:
  - `attach(actor_id)` → paused response
  - `sources(actor_id)` → `Vec<SourceInfo>`
  - `detach(actor_id)`
  - `SourceInfo`: `actor`, `url`, `is_black_boxed`
- [ ] Add `sources` CLI command in `commands/sources.rs`:
  - Output: `{ "results": [{ "url": "...", "actor": "..." }], "total": N }`
  - `--filter <substring>` — filter by URL
  - `--pattern <regex>` — filter by URL regex
- [ ] Record thread attach/sources/detach from live Firefox
- [ ] Add live tests for source listing
- [ ] Add e2e tests with mock fixtures
- [ ] Record fixtures: `thread_attach_response.json`, `sources_response.json`, `thread_detach_response.json`

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
