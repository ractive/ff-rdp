---
title: "Iteration 66: Backfill iter-61w security regression tests"
type: iteration
date: 2026-05-24
status: planned
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
- [ ] `test_refstore_capped` — in `crates/ff-rdp-core/src/refstore.rs` (or wherever `RefStore` lives): tight loop inserting `MAX_REFS + 100` entries; assert insert returns `Err(RefStoreFull)` past the cap and the live count == `MAX_REFS`.
- [ ] `test_nav_boundary_url_truncated` — in the navigate-boundary module: feed a 5000-char URL; assert the stored boundary URL is exactly 4096 chars and the truncation is signalled (suffix marker or typed flag).
- [ ] `test_token_comparison_constant_time` — use `subtle::ConstantTimeEq` directly in the assertion (the property under test is "we use a CT comparator", not microbenchmark timing variance). Confirm `compare_tokens(a, b)` delegates to `ct_eq`.
- [ ] `daemon_poisoned_mutex_recovery` — spawn the daemon in-process; inject a panic in a handler via a test hook; reconnect; assert the next request succeeds (mutex re-initialized via `lock_or_recover!`).

### B. AC-fidelity tightening
- [ ] Extend `tools/ralph-loop/scripts/ac-fidelity-check.sh` (mirror in `~/.claude/skills/ralph-loop/scripts/`) to additionally cross-check each named AC slug against `cargo test --list` output. An AC naming `test_foo` whose function doesn't exist anywhere in the workspace fails the check.
- [ ] Replay iter-61w through the strengthened check; confirm it would have failed at merge time. Document the replay in `kb/research/`.

## Acceptance Criteria [0/6]

- [ ] `test_refstore_capped`: `RefStore::insert` returns `Err(RefStoreFull)` at the cap boundary and live count plateaus at `MAX_REFS`.
- [ ] `test_nav_boundary_url_truncated`: 5000-byte URL stored as exactly 4096 bytes with truncation flag set.
- [ ] `test_token_comparison_constant_time`: assertion verifies `compare_tokens` routes through `subtle::ConstantTimeEq`.
- [ ] `daemon_poisoned_mutex_recovery`: handler panic → next reconnect succeeds → mutex contents reset to default.
- [ ] `ac_fidelity_check_validates_test_existence`: feeding a fake AC `- [x] nonexistent_test: …` to the script returns non-zero.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
