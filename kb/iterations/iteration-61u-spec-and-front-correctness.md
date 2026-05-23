---
title: "Iteration 61u: Spec & Front correctness (oneway, longstring, renames, missing watcher methods)"
type: iteration
date: 2026-05-23
status: completed
branch: iter-61u/spec-and-front-correctness
depends_on:
  - iteration-61s-typed-protocol-ides
  - iteration-61t-wire-the-foundations
tags:
  - iteration
  - specs
  - front
  - watcher
  - console
  - network
  - stability-roadmap
---

# Iteration 61u: Spec & Front correctness

The protocol-parity review against `devtools/shared/specs/*.js` found a handful of small but real divergences in the typed spec layer that landed in iter-61s. Each is < 30 LOC. Together they close a class of shape-mismatch and hang bugs that won't show up in mock-server tests but bite under live Firefox load.

## Themes

- **A — Oneway methods.** Firefox declares some methods `oneway: true` (no response packet). `unwatchTargets` is one; `clearResources` is another we want to add. Our `send_and_wait_ack` for these will hang on CLI shutdown.
- **B — Longstring on header values.** `network-event.headers-cookies.value` is `longstring` in the FF spec; modeling it as `String` hard-fails `serde_json::from_value` on real `Set-Cookie` / CSP / UA-CH headers.
- **C — Response key renames.** `console.startListeners` returns `startedListeners`, not `listeners`. With `#[serde(default)]` we silently see an empty Vec.
- **D — Missing Watcher methods.** Six method markers (`getNetworkParentActor`, `getTargetConfigurationActor`, `getBlackboxingActor`, `getBreakpointListActor`, `getThreadConfigurationActor`, `clearResources`) are declared in `devtools/shared/specs/watcher.js` but not in our `specs/watcher.rs`. The first two unblock CORS-aware response-body fetch and viewport / cache-disable / color-scheme without prefs hacks.
- **E — Type cleanups.** `screenshot.dpr` should be `string` per FF spec, not `f64`. Drop the non-spec `chromeContext` field from `console.evaluateJSAsync` and route chrome-context through the parent-process descriptor.

## Tasks

### A. Oneway fix
- [x] `core/src/fronts/watcher.rs:98-109` — `unwatch_targets` calls `transport.send(...)` only; no `recv`.
- [x] Add `oneway: bool` const to the `Method` trait in `specs/mod.rs`; default `false`.
- [x] Set `oneway = true` on `unwatchTargets` and `clearResources` (when added in D).
- [x] `Front::call` dispatches on `Method::ONEWAY` to skip the reply read.

### B. LongString type
- [x] In `core/src/specs/types.rs` (create if absent) add `enum LongString { Inline(String), Actor { actor: ActorId, length: u64, initial: String } }` with custom `Deserialize` matching FF's two shapes.
- [x] Replace `value: String` → `value: LongString` in `specs/network_event.rs:42-45` for `HeaderEntry` and any other `longstring`-declared field.
- [x] Add a helper `LongString::fetch_full(&Transport) -> Result<String>` that fetches the full content via `longstring.substring` calls for the actor variant.
- [x] Live test against a page with a `Set-Cookie` > 10 000 chars: `ff-rdp network --detail --headers` shows the full value.

### C. Console response keys
- [x] `specs/console.rs:73-83`: `#[serde(rename = "startedListeners")]` on the `startListeners` response; `#[serde(rename = "stoppedListeners")]` on `stopListeners` if it exists.
- [x] Add unit test against `tests/fixtures/start_listeners.json` (recorded from live Firefox) asserting the deserialized struct has the listener names.

### D. Watcher methods
- [x] Add to `specs/watcher.rs`: `getParentBrowsingContextID`, `clearResources` (oneway), `getNetworkParentActor`, `getBlackboxingActor`, `getBreakpointListActor`, `getTargetConfigurationActor`, `getThreadConfigurationActor`.
- [x] Each is an empty request returning an actor ref; mirror the existing watcher method patterns.
- [x] Wire `getTargetConfigurationActor` into a new `core/src/fronts/target_configuration.rs` with `set_cache_disabled(bool)`, `set_color_scheme_simulation(&str)`, `set_custom_viewport_size(w, h)`.
- [x] Replace the prefs-based cache-disable workaround in `commands/navigate.rs` with `TargetConfigurationFront::set_cache_disabled(true)`.

### E. Type cleanups
- [x] `specs/screenshot.rs:30-39`: change `dpr: f64` → `dpr: Option<String>`. Serialize as `"2.0"` etc.
- [x] Add `delay: Option<String>` to `screenshot` args per FF spec.
- [x] Drop `chrome_context: Option<bool>` from `specs/console.rs:53`. Add `evaluate_js_async_chrome` path that goes through `DescriptorFront::get_process(0).get_console_actor()`.
- [x] Update `commands/eval.rs` chrome-context branch to use the parent-process console.

## Acceptance Criteria [6/8]

- [x] `ff-rdp tabs && ff-rdp navigate ... && ff-rdp daemon stop` exits cleanly under 200ms (no hang on `unwatchTargets`).
- [ ] `live_network_set_cookie_longstring`: page sets a 50 KB `Set-Cookie`; `network --detail --headers --jq '.requests[0].response.headers[]|select(.name=="set-cookie").value'` returns the full value, not an actor ref dump. _(skeleton — fleshed out in iter-61v)_
- [x] Deserialization round-trip test for `startedListeners` / `stoppedListeners` passes.
- [ ] `live_cache_disable_via_target_config`: cache-disabled state is honored on a `Cache-Control: max-age=3600` resource across navigation. _(skeleton — fleshed out in iter-61v)_
- [x] `ff-rdp screenshot --dpr 2.0 ...` serializes `"dpr":"2.0"` on the wire (verified via `RUST_LOG=...=trace`).
- [x] `commands/eval.rs --chrome-context` no longer references a `chromeContext` request field; instead targets a separate actor.
- [x] All seven new watcher method markers present in `specs/watcher.rs` and unit-tested.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- `LongString::Inline` should serialize as a bare JSON string when re-emitted, matching FF's compact-then-actor escalation.
- Watcher accessors are cheap to add — each is essentially `request("getXActor")` returning an actor form. Don't gold-plate them with abstractions; the `Front` trait already handles lifecycle.
- The chrome-vs-content split via descriptor actor is closer to how FF clients work and removes a brittle field; the cost is one extra round-trip on first chrome eval, amortized via the registry cache.

## References

- [[watcher]] (kb)
- [[evaluate-js]] (kb)
- [[network]] §longstring
- `devtools/shared/specs/watcher.js`
- `devtools/shared/specs/network-event.js:15-18`
- `devtools/shared/specs/webconsole.js:15-21, 149-164`
- `devtools/shared/specs/screenshot.js:13-20`
- [[stability-roadmap]]
