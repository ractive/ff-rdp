---
title: "Iteration 66: Backfill iter-61w security regression tests"
type: iteration
date: 2026-05-24
status: done
branch: iter-66/backfill-security-tests
depends_on:
  - iteration-61w-security-hardening-and-cleanup
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
first_call_sites: []
dogfood_path: |
  # The four backfilled tests run as part of `cargo test --workspace -q`.
  cargo test -p ff-rdp-cli refstore_capped token_comparison_constant_time
  cargo test -p ff-rdp-core nav_boundary_url_truncated
  cargo test -p ff-rdp-cli daemon_poisoned_mutex_recovery
  # Expected: all four pass; failure mode is "future refactor silently removed the cap/sanitizer/CT cmp".
tags: [iteration, security]
---

# Iteration 66: Backfill iter-61w security regression tests

iter-61w shipped real hardening — `MAX_REFS` cap on the RefStore, 4096-byte
truncation on nav-boundary URLs, constant-time token comparison via
`subtle::ConstantTimeEq`, poisoned-mutex recovery via `lock_or_recover!` —
but ticked the four named ACs without writing the regression tests. The
plan itself self-documents this with `_Follow-up_` annotations
(`kb/iterations/iteration-61w-security-hardening-and-cleanup.md:75-80`).
That violates the CLAUDE.md rule: *"An AC without a named test is not done
— do not tick it."* Pay off the test debt before the underlying caps are
silently regressed.

## Themes

- **A — Write the four named tests.** Each one is small (≤50 lines).
- **B — Strengthen the AC-fidelity check.** Today `ac-fidelity-check.sh`
  accepts an AC if the named test slug appears in diff or commit body; it
  doesn't verify the test actually exists in `cargo test --list` output.
  Tighten this so future plans can't tick ACs they haven't written.

## Tasks

### A. The four tests
- [x] `test_refstore_capped` — lives in `crates/ff-rdp-cli/src/daemon/server.rs` alongside `RefStore`; inserts `MAX_REFS + 100` entries via `register()`, asserts the live count plateaus at `MAX_REFS` and a follow-up insert is dropped.
- [x] `test_nav_boundary_url_truncated` — in `crates/ff-rdp-cli/src/daemon/buffer.rs`: feeds a 5000-char URL through `record_nav_boundary` and asserts the stored value is exactly 4096 bytes.
- [x] `test_token_comparison_constant_time` — rewritten as a structural assertion: extracts `compare_tokens()` from the inline auth path and verifies it returns bit-equivalent results to `subtle::ConstantTimeEq::ct_eq` across equal / first-byte-differ / last-byte-differ / length-mismatch / empty cases. No timing measurement.
- [x] `daemon_poisoned_mutex_recovery` — poisons `state.buffer` by panicking while the lock is held, then drives `handle_daemon_message` with a `drain` request; asserts the dispatcher returns a normal `{"from": "daemon", "events": []}` response via `lock_or_recover!`.

### B. AC-fidelity tightening
- [x] `tools/ralph-loop/scripts/ac-fidelity-check.sh` (and its `~/.claude/skills/ralph-loop/scripts/` mirror) now require every named `test_…` / `live_…` / `bench_…` slug to resolve to an actual `fn <slug>` either in the diff or under `crates/`. Adds `--skip-test-existence` / `AC_FIDELITY_SKIP_TEST_EXISTENCE=1` opt-out for sandboxed CI. `check-discipline-regression` keeps the two copies in sync.
- [x] Iter-61w replay documented in `kb/research/iter-66-ac-fidelity-replay-iter61w.md`: three of the four security ACs would have flipped ✅→❌ under the strengthened check.

## Acceptance Criteria [6/6]

- [x] `test_refstore_capped`: insert past `MAX_REFS` saturates the store at `MAX_REFS` and subsequent batches are dropped (`crates/ff-rdp-cli/src/daemon/server.rs`).
- [x] `test_nav_boundary_url_truncated`: 5000-byte URL stored as exactly 4096 bytes (`crates/ff-rdp-cli/src/daemon/buffer.rs`).
- [x] `test_token_comparison_constant_time`: structural assertion that `compare_tokens` is bit-equivalent to `subtle::ConstantTimeEq::ct_eq` across equal/differ/length-mismatch/empty inputs.
- [x] `daemon_poisoned_mutex_recovery`: poisoned `state.buffer` → next `handle_daemon_message(drain)` returns a clean daemon response via `lock_or_recover!`.
- [x] `ac_fidelity_check_validates_test_existence`: integration test in `crates/xtask/tests/ac_fidelity_check.rs` feeds a fake AC `- [x] nonexistent_test_xyzzy_iter66: …` to the script and asserts a non-zero exit; companion test confirms the happy path still passes.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The constant-time test is not a timing test (those are flaky and rarely
informative on a CI runner). It's a structural assertion that the comparator
is the right type — exactly the property the original AC implied.

The poisoned-mutex test needs a deterministic injection point. Cleanest is
a `#[cfg(test)]` hook in the handler that, when an env var is set, panics
on the first call. The test sets the env var, fires one request, clears
it, then asserts the next request returns normally.

Strengthening `ac-fidelity-check.sh` to query `cargo test --list` is one
extra grep in the script; the cost is a `cargo test --list --quiet` run
at check time (~1–2 seconds on this workspace). Cacheable.

## Out of scope

- Re-doing the iter-61w implementations — they exist and work; this iteration
  only adds the missing regression guards.
- Promoting `claim-miss` to a hard gate (covered by iter-61aa).

## References

- [[iteration-61w-security-hardening-and-cleanup]]
- [[iteration-61y-iteration-discipline-tooling]]
- Security review report (2026-05-24), finding F-6
