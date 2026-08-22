---
title: "Iteration 185: main is red under clippy 1.98 — three one-line lints, none touched by iter-179"
type: iteration
date: 2026-08-22
status: planned
branch: iter-185/main-red-under-clippy-1-98
depends_on: []
first_call_sites: []
dogfood_path: |
  # Lint-only fix, no product behavior changes. Verification is the gate
  # itself, run on the exact toolchain CI uses.
  rustup update stable
  cargo clippy --version   # confirm 1.98.x, matching CI at the time this was filed
  cargo clippy --workspace --all-targets -- -D warnings
  # Expect exit 0. Then confirm MSRV is unaffected:
  cargo +1.95 build --workspace 2>&1 | tail -5   # or: cargo msrv verify, if configured
tags: [iteration, tooling, clippy, ci, maintenance]
---

# Iteration 185: main is red under clippy 1.98

## Why this exists

[[iteration-179-live-62-runner-sees-no-network-events]]'s closing CI run hit `clippy` failing on
PR #212 — a branch that never touched the three offending lines. Provenance was checked with
`git log -S`/`git blame` against `main`, not assumed:

- `crates/ff-rdp-cli/tests/common/mod.rs`: `buf.chunks_exact(3)` (clippy: `chunks_exact_to_as_chunks`,
  suggests `as_chunks::<3>().0`) — introduced by `49a3d88` (2026-08-12), well before iter-179's
  `baa861c` branch point.
- `crates/ff-rdp-cli/build.rs`: `.output().ok().is_some_and(...)` in `git_is_dirty` (clippy:
  `manual_is_variant_and`, suggests `.output().is_ok_and(...)`).
- `crates/ff-rdp-cli/src/commands/profiles.rs:228`: the same `.ok().is_some_and(...)` shape in the
  stale-profile-pruning closure.

All three are pre-existing on `main`. iter-179 fixed all three **on its own branch** (commit
`4d9e83c`, confirmed clean against clippy 1.98 with `rustup update stable` after CI failed with a
skewed local toolchain at 1.97), because it needed a green `clippy` check to merge — but that fix
does not reach `main` until #212 merges, and does nothing for any other open branch in the
meantime. Every PR based on an older `main` will trip the same three lints until `main` itself is
fixed directly.

**Lesson this iteration is also filed to record**: a local `cargo clippy` pass is not evidence CI's
clippy passes. CLAUDE.md's three gates (`cargo fmt`, `cargo clippy`, `cargo test`) are written as
though local green implies CI green; a toolchain skew (stable moved 1.97 → 1.98 between when this
machine last updated and when CI ran) breaks that silently, with no local signal.

## Scope

Land the same three one-line fixes directly on `main`, verified against clippy 1.98 (or whatever
CI is running at merge time — re-check, do not assume 1.98 is still current):

1. `tests/common/mod.rs`: `buf.chunks_exact(3)` → `buf.as_chunks::<3>().0`
2. `build.rs`: `.output().ok().is_some_and(...)` → `.output().is_ok_and(...)`
3. `commands/profiles.rs`: same `.ok().is_some_and(...)` → `.is_ok_and(...)`

Both replacements are clippy's own suggested fix, applied verbatim — no behavior change, only the
`Result`/`Option` conversion is elided. MSRV is 1.95 (`Cargo.toml`); `as_chunks` stabilized in
1.88, `is_ok_and` in 1.70, so neither replacement needs an MSRV bump. Confirm with the dogfood step
above rather than trusting this note by the time this plan runs.

## Tasks

### A. Land the fix [0/2]
- [ ] All three sites fixed on `main`, `cargo clippy --workspace --all-targets -- -D warnings`
      exits 0 on the toolchain CI currently runs
- [ ] MSRV gate (`msrv` CI job, or local equivalent) still passes after the change

### B. Close the toolchain-skew gap, if cheaply possible [0/1]
- [ ] Investigate whether `rust-toolchain.toml` (pinning the exact stable version this repo
      builds with) would have caught this before CI did, and either add one or record explicitly
      why not — a pin trades "always current" for "always reproducible locally", which is a real
      tradeoff, not a free fix; decide deliberately rather than defaulting

## Acceptance Criteria [0/2]

- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0 on `main` at HEAD, on the
      toolchain version CI is running at merge time
- [ ] Task B's investigation is recorded either way — a `rust-toolchain.toml` added, or a stated
      reason not to (e.g. "MSRV policy already forces contributors to test on multiple versions,
      so pinning stable would just add a second toolchain most contributors don't otherwise need")

## Out of scope

- Auditing the rest of the workspace for other latent toolchain-skew lints beyond these three —
  the closing `cargo clippy` run in task A either finds more or it doesn't; this plan does not
  presuppose there are others.
- Any behavior change beyond the three mechanical rewrites above.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — where this was found, in PR #212's
  closing CI run
- PR #212 — carry-over row 13 dispositions this to this plan
