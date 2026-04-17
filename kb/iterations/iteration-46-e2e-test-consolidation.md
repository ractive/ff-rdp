---
title: "Iteration 46: E2E Test Binary Consolidation"
type: iteration
date: 2026-04-17
status: in-progress
branch: iter-46/e2e-test-consolidation
tags: [iteration, testing, build-performance, dx]
---

# Iteration 46: E2E Test Binary Consolidation

Consolidate 29 separate e2e test binaries into a single binary to dramatically reduce compilation time. Same pattern as hyalo iter-111.

## Motivation

Each `tests/*_e2e_test.rs` file compiles and links as its own binary against the full CLI crate. Clean `cargo test` takes ~71s, with most time spent on redundant linking. The hyalo project had the same problem (31 binaries, 3m13s) and solved it by merging into a single `tests/e2e/mod.rs` — build dropped to ~25s (7.5x speedup).

## Tasks

### 1. Consolidate e2e test files into single binary [5/5]

- [x] Create `tests/e2e/` directory
- [x] Move `tests/*_e2e_test.rs` → `tests/e2e/*.rs` (strip `_e2e_test` suffix)
- [x] Move `tests/support/` → `tests/e2e/support/`
- [x] Create `tests/e2e/main.rs` with `mod` declarations for all test modules
- [x] Add `[[test]]` entry in `crates/ff-rdp-cli/Cargo.toml` pointing to `tests/e2e/main.rs`

### 2. Fix imports [2/2]

- [x] Update `use support::` → `use super::support::` in all moved test files
- [x] Remove `mod support;` from each test file (now declared once in main.rs)

### 3. Validate [3/3]

- [x] All tests pass: `cargo test --workspace -q`
- [x] Clean build is measurably faster
- [x] Incremental rebuild stays fast

## Acceptance Criteria

- [x] Single e2e test binary instead of 29 separate ones
- [x] All existing tests pass unchanged
- [x] Clean `cargo test --workspace -q` is noticeably faster (target: <40s)
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace -q`
