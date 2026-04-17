---
title: "Iteration 46: E2E Test Binary Consolidation"
type: iteration
date: 2026-04-17
status: planned
branch: iter-46/e2e-test-consolidation
tags: [iteration, testing, build-performance, dx]
---

# Iteration 46: E2E Test Binary Consolidation

Consolidate 29 separate e2e test binaries into a single binary to dramatically reduce compilation time. Same pattern as hyalo iter-111.

## Motivation

Each `tests/*_e2e_test.rs` file compiles and links as its own binary against the full CLI crate. Clean `cargo test` takes ~71s, with most time spent on redundant linking. The hyalo project had the same problem (31 binaries, 3m13s) and solved it by merging into a single `tests/e2e/mod.rs` — build dropped to ~25s (7.5x speedup).

## Tasks

### 1. Consolidate e2e test files into single binary [0/5]

- [ ] Create `tests/e2e/` directory
- [ ] Move `tests/*_e2e_test.rs` → `tests/e2e/*.rs` (strip `_e2e_test` suffix)
- [ ] Move `tests/common/` → `tests/e2e/common/`
- [ ] Create `tests/e2e/mod.rs` with `mod` declarations for all test modules
- [ ] Add `[[test]]` entry in `crates/ff-rdp-cli/Cargo.toml` pointing to `tests/e2e/mod.rs`

### 2. Fix imports [0/2]

- [ ] Update `use common::` → `use super::common::` in all moved test files
- [ ] Fix any other path-dependent imports (fixtures, etc.)

### 3. Validate [0/3]

- [ ] All tests pass: `cargo test --workspace -q`
- [ ] Clean build is measurably faster
- [ ] Incremental rebuild stays fast

## Acceptance Criteria

- [ ] Single e2e test binary instead of 29 separate ones
- [ ] All existing tests pass unchanged
- [ ] Clean `cargo test --workspace -q` is noticeably faster (target: <40s)
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace -q`
