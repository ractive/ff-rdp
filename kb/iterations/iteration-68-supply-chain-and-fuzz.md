---
title: "Iteration 68: Supply-chain CI gates + parser fuzz harnesses"
type: iteration
date: 2026-05-24
status: in-review
branch: iter-68/supply-chain-and-fuzz
depends_on:
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
first_call_sites: []
dogfood_path: |
  # 1. PR workflow rejects a known-vulnerable dep.
  # (Simulated by adding a temporary advisory to a fixture; revert before merge.)
  gh workflow run ci.yml   # cargo audit step fails on advisory

  # 2. Fuzz harnesses build and run for 60 s each without panicking.
  cargo +nightly fuzz run transport_recv_from -- -max_total_time=60
  cargo +nightly fuzz run parse_page_map_str  -- -max_total_time=60
  cargo +nightly fuzz run parse_script_file   -- -max_total_time=60
tags: [iteration, security]
---

# Iteration 68: Supply-chain CI gates + parser fuzz harnesses

`cargo audit` and `cargo deny check` currently run only in the release
workflow — there's a window between PR merge and release where a malicious
dep update lands without detection. And three parsers exposed to attacker
input (the RDP transport length-prefix framer, the page-map JSON loader,
and the script-format loader) have no fuzz harness. Both are standard
Rust-2026 hygiene; close them together.

## Themes

- **A — Move `cargo audit` and `cargo deny check` to every PR.** Currently
  in `.github/workflows/release.yml:45-51`; promote to a required PR check.
- **B — Add three fuzz harnesses.** `transport_recv_from`,
  `parse_page_map_str`, `parse_script_file`. Each is a small wrapper around
  the existing parser entry point.
- **C — Run fuzz briefly in CI.** A 60 s run per harness on every PR is
  cheap and catches new panics on the parser surface.

## Tasks

### A. PR-time supply-chain checks
- [x] Add a `cargo audit` step to `.github/workflows/ci.yml` (or whichever PR workflow runs `cargo test`). Required check.
- [x] Add a `cargo deny check` step (advisories + licences + bans). Required check.
- [x] Document the policy in `CONTRIBUTING.md`: how to handle a new advisory (yank vs pin vs ignore-with-reason).

### B. Fuzz harnesses
- [x] `cargo install cargo-fuzz` documented in `CONTRIBUTING.md`.
- [x] `fuzz/Cargo.toml` + `fuzz/fuzz_targets/transport_recv_from.rs` wrapping `crates/ff-rdp-core/src/transport.rs::recv_from` with `libfuzzer_sys::fuzz_target!`.
- [x] `fuzz/fuzz_targets/parse_page_map_str.rs` wrapping the public page-map parser entry point.
- [x] `fuzz/fuzz_targets/parse_script_file.rs` wrapping the script-format parser.
- [x] A seed corpus per harness (small valid examples checked in under `fuzz/seeds/`).

### C. CI fuzz run
- [x] PR workflow job that runs each harness for 60 s (`-max_total_time=60`). Fails on panic / sanitizer hit.
- [x] Document recovery procedure when a fuzz finding lands (open issue with minimised crash input).

## Acceptance Criteria [6/6]

- [x] `pr_workflow_runs_cargo_audit`: CI workflow has a `cargo audit` step gated as required (`.github/workflows/ci.yml` `supply-chain` job).
- [x] `pr_workflow_runs_cargo_deny`: CI workflow has a `cargo deny check` step gated as required (`.github/workflows/ci.yml` `supply-chain` job).
- [x] `fuzz_transport_recv_from`: harness exists at `fuzz/fuzz_targets/transport_recv_from.rs`, builds (verified with `cargo check`), seed corpus in `fuzz/seeds/transport_recv_from/`; CI `fuzz` job runs ≥60 s.
- [x] `fuzz_parse_page_map_str`: harness exists at `fuzz/fuzz_targets/parse_page_map_str.rs`, builds, seed corpus in `fuzz/seeds/parse_page_map_str/`; CI `fuzz` job runs ≥60 s.
- [x] `fuzz_parse_script_file`: harness exists at `fuzz/fuzz_targets/parse_script_file.rs`, builds, seed corpus in `fuzz/seeds/parse_script_file/`; CI `fuzz` job runs ≥60 s.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

`cargo-fuzz` requires nightly. CI gets a separate nightly job; local dev
remains on stable. The 60 s budget catches the bulk of "stupid panics" on
the parser surface; longer overnight fuzzing is a separate practice we can
add later (cluster runs, OSS-Fuzz integration).

`cargo deny` ban list should include the usual suspects (`openssl` if we
prefer `rustls`, etc.) — copy from a recent mature Rust project's
`deny.toml`.

## Out of scope

- `cargo vet` review state. Worth doing eventually but needs reviewer
  buy-in; file separately.
- Sigstore-signed releases. Out of scope.
- OSS-Fuzz integration.

## References

- [[iteration-63-daemon-lockrecover-and-quick-sec-fixes]]
- Security review report (2026-05-24), finding F-10
