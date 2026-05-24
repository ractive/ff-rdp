---
title: "Iteration 70: Spec drift fixes — dpr-as-string, parent chain, kb refresh"
type: iteration
date: 2026-05-24
status: in_progress
branch: iter-70/spec-drift
depends_on:
  - iteration-61u-spec-and-front-correctness
  - iteration-61p-actor-registry-and-front-lifecycle
first_call_sites: []
dogfood_path: |
  # 1. Wire-level: screenshot.capture sends dpr as a JSON string.
  ff-rdp --log-rdp-trace screenshot https://example.com /tmp/x.png
  grep '"dpr":"' ~/.cache/ff-rdp/rdp-trace.log    # expect string-quoted dpr
  ! grep '"dpr":[0-9]' ~/.cache/ff-rdp/rdp-trace.log    # expect no numeric dpr

  # 2. Walker → nodeActor invalidation cascade.
  # When the walker's target is destroyed, child node actors must be invalidated too.
  ff-rdp dom query 'body' --json | jq .    # then navigate; second query must re-derive.
tags: [iteration, protocol]
---

# Iteration 70: Spec drift fixes — dpr-as-string, parent chain, kb refresh

Three places where ff-rdp's wire / lifecycle behaviour drifts from the
authoritative Firefox spec or from our own kb:
(1) `ScreenshotActor::capture` sends `dpr` as a JSON number; the spec at
`devtools/shared/specs/screenshot.js:18` types it as `nullable:string`, and
`kb/rdp/actors/screenshot.md:98` flagged this long ago.
(2) `Registry::invalidate_target` does a one-level sweep by `target_root`;
walker→nodeActor→nodeListActor chains aren't modelled because nodeActors
register against the walker's target, not the walker itself.
(3) Several kb files describe behaviour that differs from the merged code.

## Themes

- **A — `dpr` as string.** Match the Firefox spec.
- **B — Parent-chain invalidation.** Add `parent: Option<ActorId>` to
  `FrontState`; implement BFS invalidation from a destroyed root.
- **C — kb refresh.** Update the three kb files whose claims no longer
  match code so the kb stays trustworthy.

## Tasks

### A. `dpr` as string
- [x] At `crates/ff-rdp-core/src/actors/screenshot.rs:60-71`, send `prep.window_dpr.to_string()` (or `format!("{}", ...)` if precision matters) instead of the raw f64.
- [x] Update the unit-test fixture to assert the field is a JSON string.
- [x] Add a live test (gated on `FF_RDP_LIVE_TESTS=1`) that captures a screenshot and asserts no error from Firefox's spec validator. — `live_screenshot_dpr_string_accepted`

### B. Parent-chain invalidation
- [x] Add `parent: Option<ActorId>` to `FrontState` in `crates/ff-rdp-core/src/registry.rs:46-71`.
- [x] Update `Registry::register` to accept an optional parent and store it. — done via additive `register_with_parent` to keep existing call sites unchanged.
- [x] Rewrite `invalidate_target` to BFS from the destroyed target through the parent graph: mark every descendant `alive = false`.
- [x] [deferred — new plan: kb/iterations/iteration-71-resource-and-session.md] Inspector/walker `nodeActor` registration sites — no inspector/node fronts exist yet; the BFS infra is in place for when they land.
- [x] Test: register `walker → nodeActor → nodeListActor`; invalidate `walker`; assert all three are dead. — `registry_parent_chain_invalidation`

### C. kb refresh
- [x] Edit `kb/rdp/actors/screenshot.md:98` — note that ff-rdp now sends `dpr` as a string (closed; remove the warning).
- [x] Edit `kb/rdp/from-our-codebase/wired-vs-primitive.md:74-89` — correct the claim that navigate waits on `tabNavigated + document-event`; navigate waits on `document-event` only. `tabNavigated` is consumed only as an abort signal in `evaluate_js_async`.
- [x] Edit `kb/rdp/from-our-codebase/open-gaps.md:36-50` — refresh the status of `actor-leak-in-daemon` (still partial) and `legacy-startlisteners-coexistence` (closed by iter-71 if it lands before this iter).

## Acceptance Criteria [5/5]

- [x] `screenshot_dpr_serialised_as_string`: outbound packet JSON has `dpr` as `Value::String`, not `Value::Number`.
- [x] `registry_parent_chain_invalidation`: BFS test with 3-deep chain → all three marked `alive = false` after root invalidation.
- [x] `kb_refresh_screenshot_dpr`: `hyalo find` shows the screenshot.md note updated.
- [x] `kb_refresh_wired_vs_primitive`: the navigate-wait claim is corrected.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The `dpr` fix is one line of code plus a fixture update. Worth a dedicated
AC because the bug has been documented in the kb since iter-3-ish and
never closed.

`parent: Option<ActorId>` is additive — every existing registration site
passes `None` by default. Only inspector/walker call sites need to thread
the parent. Old behaviour (one-level invalidation via `target_root`) stays
for actors that opt out by omitting `parent`.

kb edits use `hyalo` per CLAUDE.md.

## Out of scope

- Dropping `target_root` entirely in favour of pure parent chains — keep
  both during the transition.
- Resource-command lifecycle and Session integration (iter-71).

## References

- [[iteration-61u-spec-and-front-correctness]]
- [[iteration-61p-actor-registry-and-front-lifecycle]]
- Protocol review report (2026-05-24), §2.3, §2.6, §3
- `kb/rdp/actors/screenshot.md`
- `kb/rdp/from-our-codebase/wired-vs-primitive.md`
