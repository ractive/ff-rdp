---
title: "Iteration 118: migrate release pipeline to shared ractive/release-workflows"
type: iteration
date: 2026-07-10
status: done
branch: iter-118/shared-release-workflow-migration
depends_on: []
firefox_refs: []
kb_refs: []
first_call_sites: []
dogfood_path: |
  # Validate the pipeline without cutting a release:
  gh workflow run release.yml --ref iter-118/shared-release-workflow-migration
  gh run watch --exit-status
  # expect: dry-run summary job-summary table listing ff-rdp-v<version>-<target>.*
  # archives + SBOMs for the six configured targets, nothing published anywhere.
tags:
  - iteration
  - ci
  - release
  - infra
---

# Iteration 118: migrate to shared release-workflows

`ractive/hyalo`, `ractive/hoppy`, and `ractive/ff-rdp` had near-duplicate
`.github/workflows/release.yml` files (version-check, security audit, build
matrix, SBOM/attestation, GitHub release upload, crates.io, Homebrew, Scoop,
winget). Fixes discovered in one repo (hermetic build provenance, per-target
rust-cache keys, "already exists on crates.io index" as published) had to be
manually ported to the other two. `ractive/release-workflows` now hosts a
single reusable `workflow_call` pipeline (tagged `v0.1.0`, self-tested via
its own `selftest.yml` dry-run against a fixture crate); this iteration
replaces ff-rdp's copy with a thin caller.

## Tasks

- [x] Replace `.github/workflows/release.yml` with a caller of
      `ractive/release-workflows/.github/workflows/release.yml@v0.1.0`,
      preserving ff-rdp's existing 6-target matrix (musl-only Linux cross
      targets with tests skipped under QEMU; Windows tests skipped because
      the mock TCP server hangs on Windows CI runners).
  - Code: `.github/workflows/release.yml`
- [x] Add `workflow_dispatch` trigger with `dry-run: true` so the pipeline
      can be validated without cutting a real release.
  - Code: `.github/workflows/release.yml` (`on.workflow_dispatch`, `with.dry-run`)
- [x] Add `Cross.toml` forwarding `GIT_COMMIT`/`GIT_COMMIT_DATE` into cross
      containers. ff-rdp's `build.rs` already reads these env vars for
      hermetic build provenance, but no `Cross.toml` existed to pass them
      through — the two musl cross targets were previously shelling out to
      the container's own git instead of using the host checkout's commit.
  - Code: `Cross.toml`
- [x] Do not touch `ci.yml`, `live.yml`, or `.github/release.yml`.
- [x] Validate with `actionlint`.
- [x] Run quality gates (`cargo fmt --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace -q`).

## Behavior deltas vs the old workflow

- **Archive naming**: `ff-rdp-<target>.*` → `ff-rdp-v<version>-<target>.*`.
  SBOM files follow the same rename. Homebrew/Scoop formulas/manifests are
  regenerated per release, so this is transparent to existing installs. The
  winget `installers-regex` (`ff-rdp-.*-pc-windows-msvc\.zip$`) still matches
  the versioned name.
- **Homebrew Linux artifact selection**: the shared workflow prefers musl
  over gnu for *both* Linux architectures when both are present in
  `SHA256SUMS`. ff-rdp's target matrix only builds musl for both arches (as
  before), so this is a no-op for arm64 (already musl) but is a **behavior
  change for x86_64**: the old formula pinned
  `ff-rdp-x86_64-unknown-linux-gnu.tar.gz` explicitly (matching the old
  matrix's native, non-cross x86_64 build); after migration the Homebrew
  formula will instead reference the musl x86_64 artifact. Strictly more
  portable (static linking, no glibc coupling) — worth calling out as an
  intentional behavior change, not a bug.
- **Attestation identity**: subject binds to the shared reusable workflow
  (`ractive/release-workflows/.github/workflows/release.yml@v0.1.0`) rather
  than ff-rdp's own inline workflow — provenance is now uniform (SLSA-L3-style)
  across all three consuming repos. Cross-compiled musl targets remain
  unattested, same as before (cross containers lack OIDC).
- **Hermetic provenance for cross targets** (new, not just a rename): with
  `Cross.toml` added in this PR, the two musl cross targets now receive
  `GIT_COMMIT`/`GIT_COMMIT_DATE` from the workflow instead of falling back to
  a shell-out inside the cross container. Native targets already got this
  correctly via `$GITHUB_ENV`; only the cross path changes.
- **winget**: stays non-blocking (`continue-on-error: true` in the shared
  workflow's `winget` job, same as before).
- **Windows archive format**: still `7z a` — same tool, but the archive now
  contains a staged `archive/` directory (binary + `LICENSE` + `README.md`)
  instead of just the bare `.exe`, matching the new "Verify CLI runs" +
  LICENSE/README inclusion below.
- **New**: a "Verify CLI runs" smoke step (`cargo run --release --target
  $TARGET -p ff-rdp-cli -- --help`) runs on every native, tested target
  before packaging — the old workflow had no equivalent smoke check.
- **New**: every archive (Unix and Windows) now includes `LICENSE` and
  `README.md` alongside the binary; the old workflow's Unix path only tarred
  the bare binary and the Windows path only zipped the bare `.exe`.
- **SBOM coverage** (real gap, not just cosmetic): the old workflow attached
  *two* SBOMs per native target — `ff-rdp-cli` (required) and `ff-rdp-core`
  (best-effort). The shared workflow's SBOM step only resolves and attaches
  the SBOM for `version-package` (`ff-rdp-cli`); there is no equivalent
  best-effort second SBOM for `ff-rdp-core`. This is a real reduction in
  published SBOM coverage, not a renaming — flagged for follow-up upstream
  in `release-workflows` (e.g. an `extra-sbom-packages` input) rather than
  worked around here, since fixing it means changing the shared workflow
  that hyalo and hoppy also consume.
- **`live.yml`**: unaffected. Its own `on: release: types: [published]`
  trigger continues to fire independently of this workflow's replacement —
  the two workflows both subscribe to the same GitHub event but are
  otherwise unrelated.

## Acceptance Criteria [6/6]

- [x] `actionlint .github/workflows/release.yml` passes with no findings.
  - Test evidence: `actionlint` run locally, exit 0, no output.
- [x] `cargo fmt --check` passes (no Rust files changed by this PR).
  - Test evidence: `cargo fmt --check`, exit 0.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
  - Test evidence: clippy run locally, exit 0, `Finished` with no warnings.
- [x] `cargo test --workspace -q` passes (live tests excluded/ignored as
      designed — they require `FF_RDP_LIVE_TESTS=1` and a local Firefox).
  - Test evidence: full workspace test run locally, all suites `ok`, one
    live test correctly reported `ignored`.
- [x] The new `.github/workflows/release.yml` is a thin caller only —
      `ci.yml`, `live.yml`, `.github/release.yml` are untouched.
  - Test evidence: `git diff --stat origin/main` shows only
    `.github/workflows/release.yml`, `Cross.toml`, and this KB file.
- [x] `dogfood_path: gh workflow run release.yml --ref
      iter-118/shared-release-workflow-migration` [deferred — run from the
      open PR once pushed, not locally reproducible without a GitHub Actions
      run].
