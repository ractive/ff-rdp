## Spec drift — watcher actor

### Methods missing from implementation

None detected in this diff.

### Fields renamed vs spec

| Change | File | Location | Spec reference |
|--------|------|----------|----------------|
| Parameter `resource_types` renamed to `resources` | `watcher.rs` | `watch_resources` fn | `devtools/shared/specs/watcher.js` — `watchResources` method param should be `resourceTypes` (camelCase convention preserved, but the Rust-side name `resource_types` is the conventional mapping) |

### `oneway`/`release`/`bulk` marker mismatches

| Issue | Method | Details |
|-------|--------|---------|
| `oneway: true` marker comment removed | `watch_targets` | The comment documenting `oneway: true` semantics for `watchTargets` was removed. Per `devtools/shared/specs/watcher.js`, `watchTargets` is a one-way call — Firefox sends no reply. Callers must not await a response. Removing this comment risks future callers incorrectly expecting a reply. |

### Summary

2 drift item(s) found:
1. Parameter rename `resource_types` → `resources` in `watch_resources` diverges from the spec's `resourceTypes` parameter name (cosmetic, low risk — but worth noting for consistency with spec vocabulary).
2. Removal of `oneway: true` marker comment on `watch_targets` is a **high-risk** change: this method is documented as fire-and-forget in the Firefox spec. Losing the marker may lead to callers blocking on a reply that never arrives.
