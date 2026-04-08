---
title: "Iteration 29: Code Review & Simplification"
type: iteration
status: planned
date: 2026-04-08
tags: [iteration, refactor, code-quality, simplification]
branch: iter-29/code-review-simplification
---

# Iteration 29: Code Review & Simplification

Post-feature code review findings. Focus on reducing duplication and improving
abstractions now that all features are implemented.

## Part A: Extract Eval Exception Handler

20+ commands repeat the same eval-exception-check pattern. Extract a helper.

- [ ] Create `eval_or_bail(transport, actor, js) -> Result<EvalResult>` in
  `ff-rdp-cli/src/commands/` (or a shared module) that calls
  `WebConsoleActor::evaluate_js_async()` and returns an `AppError` if
  `exception` is present
- [ ] Migrate all commands that use the pattern: eval, click, type, navigate,
  perf, dom, snapshot, geometry, responsive, a11y, cookies, storage, sources
- [ ] Verify no behavior changes via `cargo test --workspace`

## Part B: Extract Navigate Wait Polling

`navigate.rs` `wait_after_navigate()` and `wait.rs` `build_wait_js()` share
timeout polling logic. Extract a reusable polling primitive.

- [ ] Create `poll_js_condition(transport, actor, js_fn, timeout, interval) -> Result<bool>`
- [ ] Refactor `navigate.rs` wait logic to use it
- [ ] Refactor `wait.rs` to use it
- [ ] Verify via tests

## Part C: Consolidate Network Event Paths

`drain_network_events()` (direct) and daemon streaming path share merging logic.

- [ ] Ensure `merge_updates()` is the single source of truth for network event
  aggregation (check for any inline duplicates)
- [ ] Review `daemon/server.rs` event forwarding for unnecessary copies

## Part D: Minor Cleanups

- [ ] Audit for any remaining `.unwrap()` or `.expect()` outside tests
- [ ] Check for unnecessary `pub` visibility on struct fields
- [ ] Remove any dead code flagged by `cargo clippy`
- [ ] Ensure all commands use the output envelope consistently

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
