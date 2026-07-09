---
title: "Iteration 105: error taxonomy completion + release hygiene — lossless Protocol bridge, non_exhaustive, one exit-code map, MSRV, serde_yaml"
type: iteration
date: 2026-07-09
status: done
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

## CI-wait policy (2026-07-09, per James)

When waiting on PR checks before merging, wait ONLY for the required lanes:
fmt, clippy, discipline, supply-chain, fuzz, test (ubuntu-latest),
test (macos-latest), verify-attestation. Do NOT wait for or block on:
- `live-tests` — advisory by design (continue-on-error); failures belong to
  [[iteration-106-live-test-masking-cascade]] / [[iteration-107-post-105-live-sweep]].
- `test (windows-latest)` — known-red with 5 pre-existing failures tracked in
  [[iteration-108-windows-ci-preexisting-reds]]. Do glance at its failure
  list once: if it shows failures OTHER than those 5, that IS a regression —
  stop and fix before merging.

## Live-test policy (2026-07-09, per James)

Do NOT run the full live Firefox suite (`cargo test-live`, or `--test live --
--include-ignored` without a filter) during this iteration — neither while
implementing nor while reviewing. Run ONLY (1) the specific live tests this
plan's ACs name, filtered (e.g. `cargo test -p ff-rdp-cli --test live
<filter> -- --include-ignored`), and (2) this iteration's dogfood script
(required by check-iteration-ready). Full-suite validation is deferred to
[[iteration-107-post-105-live-sweep]], which runs once after iteration 105
merges and fixes all fallout there.

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
   — each addition is a breaking change for downstream matches. iter-104
   is a fresh example of the pattern one layer over: it added `FrontKind::Manifest`
   to `registry.rs`, also a public, non-`#[non_exhaustive]` enum that gains a
   variant nearly every iteration (iter-103 added `TargetConfiguration` the
   same way). `FrontKind` already carries an `Other(String)` catch-all, which
   softens but does not eliminate the same breaking-change risk. Out of scope
   for Theme B itself (that's the four error enums), but worth a look-and-decide
   pass in this PR: either add `#[non_exhaustive]` to `FrontKind` alongside the
   error enums (same mechanical fix, same PR), or explicitly note in
   [[decision-log]] why the `Other(String)` catch-all is judged sufficient and
   defer it.
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

### A. Lossless error bridge [2/2]
- [x] Replace the flattening `From<ProtocolError> for RdpError` with a
      `#[error(transparent)] Protocol(#[from] ProtocolError)` variant (or
      complete the full migration if the call-site count turns out small —
      decide in the PR, document in [[decision-log]]); no fabricated fields,
      no dropped `ActorErrorKind`, no severed sources.
      → chose the transparent-variant route (DEC-018); only one in-tree
      consumer (`WatcherActor::unwatch_targets`) relied on the old `From` impl.
      `error.rs` `RdpError::Protocol(#[from] ProtocolError)`.
- [x] Update CLI error mapping (`AppError`) to match on the preserved
      variants; land `unit_protocol_error_roundtrip_preserves_kind`
      (an `ActorErrorKind::WrongState` protocol error is still
      distinguishable from `NoSuchActor` after crossing the bridge).
      `From<RdpError> for AppError` delegates `Protocol(pe)` to
      `From<ProtocolError>`; `unit_protocol_error_roundtrip_preserves_kind`.

### B. Semver armor [1/1]
- [x] Add `#[non_exhaustive]` to `RdpError`, `ProtocolError`,
      `ActorErrorKind`, `NavCause`; fix the resulting non-defining-crate
      matches (CLI gains explicit `_ =>` arms with intentional fallbacks);
      land `unit_error_enums_non_exhaustive` (source-pin test in the style
      of the existing forbid-unsafe scan).
      Also added `#[non_exhaustive]` to `registry::FrontKind` (DEC-019, the
      look-and-decide pass). `unit_error_enums_non_exhaustive`.

### C. One exit-code map [2/2]
- [x] Fold the mapping from `main.rs:208-223` (`error_exit_code`) into
      `AppError::exit_code()`; `main.rs` calls only `exit_code()`; delete
      the shadow function. `main.rs` now calls `err.exit_code()`;
      `error_exit_code` deleted.
- [x] Land `unit_exit_code_and_error_type_frozen`: a table test enumerating
      every `AppError` variant with its exit code **and** its `error_type`
      string — existing values frozen exactly as-is (renaming shipped
      discriminants is a breaking change we are not taking); the table doc
      states "new discriminants MUST be snake_case".
      `unit_exit_code_and_error_type_frozen`.

### D. Release hygiene [3/3]
- [x] Add `rust-version` to `[workspace.package]` (the version CI actually
      validates) and a CI job pinned to it, so MSRV breakage is a red check,
      not a user report. `rust-version = "1.95"`; `msrv` job in `ci.yml`.
- [x] Add `[workspace.lints.rust]` with `unsafe_code = "forbid"` for core
      (retiring the hand-rolled source-scan test) and an explicit decision
      for the CLI crate (it has real `libc`/`windows-sys` FFI — scope its
      allowance narrowly and note it in the lints table comment).
      `[workspace.lints.rust] unsafe_code = "forbid"`; CLI crate overrides to
      `deny` + file-scoped `#![allow(unsafe_code)]` in the 4 FFI modules.
- [x] Replace archived `serde_yaml` (6 call sites: `script/format.rs`,
      `commands/index.rs`, `page_map/mod.rs`, xtask) with a maintained fork
      (`serde_norway` or equivalent); bump the direct `getrandom` 0.2 pin to
      converge duplicate versions where the tree allows.
      Swapped all 6 call sites + fuzz to `serde_norway`. getrandom
      convergence was **not** attempted: `Cargo.lock` still resolves three
      versions (0.2.17 direct + transitive 0.3.4/0.4.2 via `ahash`/`tempfile`
      dev-deps) — `cargo deny check` passes regardless (no advisory/ban hit),
      so this is deferred rather than a regression. Correcting the plan's
      original "single resolved version" assumption, found during
      /review-pr on PR #145.

## Acceptance Criteria [6/6]

- [x] unit_protocol_error_roundtrip_preserves_kind: `ActorErrorKind`
      discriminants and timeout durations survive ProtocolError → RdpError →
      AppError conversion (no `after_ms: 0` fabrication).
- [x] unit_error_enums_non_exhaustive (`#[non_exhaustive]` source-pin test):
      asserts the attribute is present on all four public error enums
      (`RdpError`, `ProtocolError`, `ActorErrorKind`, `NavCause`).
- [x] unit_exit_code_and_error_type_frozen (`AppError::exit_code` table test):
      covers every `AppError` variant with its exit code and error_type string;
      all pre-iteration values are unchanged.
- [x] e2e_exit_codes_regression (`error_exit_code` shadow deleted, callers use
      `AppError::exit_code`): the existing `exit_codes.rs`/`error_shapes.rs`
      suites pass without modification — proof of no behavior change.
- [x] unit_yaml_output_unchanged (`serde_norway` drop-in swap): YAML output is
      byte-identical — the existing `commands/index.rs`/`page_map`/`script`
      round-trip tests pass unmodified.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
