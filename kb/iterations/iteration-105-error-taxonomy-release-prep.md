---
title: "Iteration 105: error taxonomy completion + release hygiene — lossless Protocol bridge, non_exhaustive, one exit-code map, MSRV, serde_yaml"
type: iteration
date: 2026-07-09
status: planned
branch: iter-105/error-taxonomy-release-prep
depends_on: []
firefox_refs: []
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
first_call_sites:
  - primitive: >-
      RdpError::Protocol transparent variant (lossless ProtocolError
      passthrough replacing the flattening From impl)
    site: crates/ff-rdp-cli/src/error.rs
  - primitive: >-
      unified AppError::exit_code() as the single exit-code authority
      consumed by main.rs
    site: crates/ff-rdp-cli/src/main.rs
dogfood_path: |
  ff-rdp eval 'x' --tab-id nonexistent ; echo "exit=$?"
  # expected: JSON error envelope with a stable snake_case error_type and the
  # documented exit code — identical before/after this iteration (frozen table)
tags: [iteration, errors, semver, release, hygiene, msrv, review-2026-07]
---

# Iteration 105: error taxonomy completion + release hygiene

ff-rdp-core is published (0.2.x) but its error story is a half-finished
migration the deep review ([[deep-review-2026-07-fable5]]) flagged as the
top design debt — worth completing **before the next release cut**:

1. **Lossy bridge.** `RdpError` and `ProtocolError` coexist, and
   `From<ProtocolError> for RdpError` (`error.rs:57-138`) destroys
   information: `Timeout` is rebuilt with fabricated `after_ms: 0`;
   `ActorError` drops the `ActorErrorKind` discriminant so consumers can't
   tell `noSuchActor` from `wrongState`; everything else is stringified into
   `Shape { got: … }`, severing `io::Error` source chains. The doc comment
   admits the migration stalled.
2. **Semver trap.** None of `RdpError` / `ProtocolError` / `ActorErrorKind`
   / `NavCause` is `#[non_exhaustive]`, yet recent iterations added variants
   — each addition is a breaking change for downstream matches.
3. **Split-brain exit codes.** `AppError::exit_code()` documents itself as
   *the* mapping but returns 1 for variants `main.rs:208-223` maps to 4/5/6;
   correct today only by call-graph accident.
4. **Discriminant drift.** JSON `error_type` values mix PascalCase
   (`"User"`, `"Protocol"`) and snake_case (`"actor_destroyed"`,
   `"nav_dns_fail"`) with nothing freezing them.

Riding along, three review hygiene items that belong in the same
release-prep PR: no MSRV anywhere (the project has already been bitten by
clippy-version drift), the archived `serde_yaml` dependency, and the missing
`[workspace.lints.rust]` table (core hand-rolls `#![forbid(unsafe_code)]` +
a source-scan test instead).

## Themes

- **A — Lossless error bridge.** `ProtocolError` passes through intact.
- **B — Semver armor.** `#[non_exhaustive]` on the public error enums.
- **C — One exit-code map.** `AppError::exit_code()` becomes the only
  authority; a frozen table test pins codes *and* `error_type` strings.
- **D — Release hygiene.** MSRV, lints table, serde_yaml replacement.

## Tasks

### A. Lossless error bridge [0/2]
- [ ] Replace the flattening `From<ProtocolError> for RdpError` with a
      `#[error(transparent)] Protocol(#[from] ProtocolError)` variant (or
      complete the full migration if the call-site count turns out small —
      decide in the PR, document in [[decision-log]]); no fabricated fields,
      no dropped `ActorErrorKind`, no severed sources.
- [ ] Update CLI error mapping (`AppError`) to match on the preserved
      variants; land `unit_protocol_error_roundtrip_preserves_kind`
      (an `ActorErrorKind::WrongState` protocol error is still
      distinguishable from `NoSuchActor` after crossing the bridge).

### B. Semver armor [0/1]
- [ ] Add `#[non_exhaustive]` to `RdpError`, `ProtocolError`,
      `ActorErrorKind`, `NavCause`; fix the resulting non-defining-crate
      matches (CLI gains explicit `_ =>` arms with intentional fallbacks);
      land `unit_error_enums_non_exhaustive` (source-pin test in the style
      of the existing forbid-unsafe scan).

### C. One exit-code map [0/2]
- [ ] Fold the mapping from `main.rs:208-223` (`error_exit_code`) into
      `AppError::exit_code()`; `main.rs` calls only `exit_code()`; delete
      the shadow function.
- [ ] Land `unit_exit_code_and_error_type_frozen`: a table test enumerating
      every `AppError` variant with its exit code **and** its `error_type`
      string — existing values frozen exactly as-is (renaming shipped
      discriminants is a breaking change we are not taking); the table doc
      states "new discriminants MUST be snake_case".

### D. Release hygiene [0/3]
- [ ] Add `rust-version` to `[workspace.package]` (the version CI actually
      validates) and a CI job pinned to it, so MSRV breakage is a red check,
      not a user report.
- [ ] Add `[workspace.lints.rust]` with `unsafe_code = "forbid"` for core
      (retiring the hand-rolled source-scan test) and an explicit decision
      for the CLI crate (it has real `libc`/`windows-sys` FFI — scope its
      allowance narrowly and note it in the lints table comment).
- [ ] Replace archived `serde_yaml` (6 call sites: `script/format.rs`,
      `commands/index.rs`, `page_map/mod.rs`, xtask) with a maintained fork
      (`serde_norway` or equivalent); bump the direct `getrandom` 0.2 pin to
      converge duplicate versions where the tree allows.

## Acceptance Criteria [0/6]

- [ ] unit_protocol_error_roundtrip_preserves_kind: `ActorErrorKind`
      discriminants and timeout durations survive ProtocolError → RdpError →
      AppError conversion (no `after_ms: 0` fabrication).
- [ ] unit_error_enums_non_exhaustive: source-pin test asserts the attribute
      is present on all four public error enums.
- [ ] unit_exit_code_and_error_type_frozen: table test covers every AppError
      variant; all pre-iteration exit codes and error_type strings are
      unchanged.
- [ ] e2e_exit_codes_regression: the existing `tests/exit_codes.rs` and
      `tests/error_shapes.rs` suites pass without modification (proof of no
      behavior change).
- [ ] unit_yaml_output_unchanged: recorded YAML output fixtures
      (script/format, index, page_map) are byte-identical after the
      serde_yaml replacement.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The transparent-variant route is strictly additive and keeps
  `ProtocolError`'s carefully-typed surface (`is_transient()` exhaustiveness
  stays inside the defining crate, unaffected by `#[non_exhaustive]`).
  The full-migration alternative is better long-term but must not balloon
  this PR — if chosen, it needs its own follow-up plan per the carry-over
  rule.
- Freezing `error_type` strings is deliberately conservative: downstream
  agents already branch on them (JSON-only CLI contract). Consistency is
  achieved going forward, not retroactively.
- MSRV choice: whatever the current CI toolchain is at branch time — the
  point is *declaring* it, not supporting old compilers.

## Out of scope

- Process-global transport knobs → `TransportLimits` struct
  (review finding D-8) — worthwhile core API change, separate plan.
- `FrontState` field privatization (review finding on `registry.rs:52-63`) —
  small, but touches core API; bundle with the TransportLimits plan.
- Migrating remaining `serde_json::Value` actor APIs onto the typed
  `specs::call` layer — long-running background effort.

## References

- [[deep-review-2026-07-fable5]] — Rust findings D1–D4, MSRV, serde_yaml.
- [[decision-log]] — record the bridge-vs-migration decision.
- `crates/ff-rdp-core/src/error.rs:57-138`, `crates/ff-rdp-cli/src/main.rs:208-223`.
