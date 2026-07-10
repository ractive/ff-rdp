---
title: "Iteration 119: deb/rpm packaging + Cloudsmith publishing"
type: iteration
date: 2026-07-11
status: planned
branch: iter-119/linux-packages
depends_on: ["iter-118/shared-release-workflow-migration"]
firefox_refs: []
kb_refs: []
first_call_sites: []
dogfood_path: |
  # Validate the packaging path without cutting a release (dry-run still
  # builds the .deb/.rpm, only publishing to Cloudsmith is skipped):
  gh workflow run release.yml --ref iter-119/linux-packages
  gh run watch --exit-status
  # expect: linux-packages job succeeds, dry-run summary lists
  # ff-rdp-v<version>-x86_64-linux.deb and .rpm as build artifacts.
tags:
  - iteration
  - ci
  - release
  - infra
  - packaging
---

# Iteration 119: deb/rpm packaging + Cloudsmith publishing

Iteration 118 migrated ff-rdp's release pipeline to the shared
`ractive/release-workflows` reusable workflow, but left Linux packaging
disabled. `ractive/hoppy` already exercises the shared workflow's
`enable-linux-packages`/`cloudsmith-repo` inputs (built natively via
`cargo deb`/`cargo generate-rpm`, published to the `ractive/ractive-pkgs`
apt/yum repos on Cloudsmith). This iteration turns the same inputs on for
ff-rdp so `ff-rdp` ships as a native Linux package, not just a tarball.

Unlike hoppy, ff-rdp has no shell completions or man pages, so the package
payload is just the binary plus `LICENSE`/`README.md` — no
`pre-package-command` or `extra-archive-paths` needed.

## Tasks

- [x] Add `[package.metadata.deb]` and `[package.metadata.generate-rpm]` to
      `crates/ff-rdp-cli/Cargo.toml`, modeled on hoppy's pattern, scoped to
      binary + LICENSE + README (no completions/man assets).
  - Code: `crates/ff-rdp-cli/Cargo.toml`
- [x] Enable Linux packaging and Cloudsmith publishing in the release caller.
  - Code: `.github/workflows/release.yml`
      (`enable-linux-packages`, `linux-package-crate`, `cloudsmith-repo`)
- [x] Validate with `actionlint`.
- [x] Run quality gates (`cargo fmt --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace -q`).

## What's new

- **`.deb`/`.rpm` build**: the shared workflow's `linux-packages` job builds
  `ff-rdp-cli` natively on `ubuntu-latest` (`cargo build --release`), then
  runs `cargo deb -p ff-rdp-cli --no-build --no-strip` and
  `cargo generate-rpm -p crates/ff-rdp-cli`, reading the new
  `[package.metadata.deb]` / `[package.metadata.generate-rpm]` tables. Output
  is renamed to `ff-rdp-v<version>-x86_64-linux.{deb,rpm}` and uploaded as a
  build artifact.
- **GitHub release assets**: on a real (non-dry-run) release, the `release`
  job downloads all artifacts (including the new deb/rpm) and uploads them
  alongside the existing tarballs/zips, all covered by one `SHA256SUMS`.
- **Cloudsmith publishing**: a new `cloudsmith` job (gated on
  `enable-linux-packages && cloudsmith-repo != '' && !dry-run`) pushes the
  `.deb`/`.rpm` to `ractive/ractive-pkgs` via
  `uvx --from cloudsmith-cli==1.19.0 cloudsmith push {deb,rpm}
  ractive/ractive-pkgs/any-distro/any-version`, using the repo's existing
  `CLOUDSMITH_API_KEY` secret (already present — no new secret to
  provision). This job is independent of the GitHub release upload, so a
  publishing failure there does not affect the tarball release.
- **Dry-run behavior**: `enable-linux-packages: true` means the
  `linux-packages` job (build + `cargo deb`/`cargo generate-rpm`) always
  runs, dry-run or not — only the Cloudsmith push and the GitHub release
  upload are skipped under dry-run. This is what makes the dry-run a
  meaningful smoke test for the deb/rpm build path.
- **No `pre-package-command`**: hoppy needs one to stage completions/man
  pages into the crate directory before packaging (cargo-deb/generate-rpm
  resolve asset paths relative to the crate dir). ff-rdp ships no such
  assets, so the binary + `../../LICENSE` + `../../README.md` assets in the
  metadata tables are sufficient without any pre-package staging step.

## Install (post-merge, once a release publishes)

```sh
# Debian/Ubuntu
curl -1sLf 'https://dl.cloudsmith.io/public/ractive/ractive-pkgs/cfg/setup/bash.deb.sh' | sudo bash
sudo apt install ff-rdp

# Fedora/RHEL
curl -1sLf 'https://dl.cloudsmith.io/public/ractive/ractive-pkgs/cfg/setup/bash.rpm.sh' | sudo bash
sudo dnf install ff-rdp
```

## Acceptance Criteria [4/5]

- [x] actionlint passes with no findings.
  - Test evidence: `actionlint .github/workflows/release.yml` run locally,
    exit 0, no output.
- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
      warnings`, `cargo test --workspace -q` all pass.
  - Test evidence: all three run locally in order, exit 0 each; test suite
    all `ok`, live tests correctly `ignored` (not run, `FF_RDP_LIVE_TESTS`
    unset).
- [x] `crates/ff-rdp-cli/Cargo.toml` carries valid `[package.metadata.deb]`
      and `[package.metadata.generate-rpm]` tables that `cargo metadata`
      can parse.
  - Test evidence: `cargo metadata --format-version 1 >/dev/null` and
    `cargo check -p ff-rdp-cli -q` both exit 0 after adding the tables.
- [x] `.github/workflows/release.yml` stays a thin caller — only the diff
      needed to turn on Linux packaging/Cloudsmith is added, nothing else
      restructured.
  - Test evidence: `git diff --stat iter-118/shared-release-workflow-migration`
    shows only `.github/workflows/release.yml`,
    `crates/ff-rdp-cli/Cargo.toml`, and this KB file.
- [ ] dogfood_path verified end-to-end [deferred — requires a pushed branch
      and a live GitHub Actions dry-run to actually build the .deb/.rpm on
      ubuntu-latest; not reproducible in a local checkout since cargo-deb
      output layout (`target/debian/`) and cargo-generate-rpm
      (`target/generate-rpm/`) are Linux-specific packaging steps this repo
      doesn't otherwise invoke].
  - `gh workflow run release.yml --ref iter-119/linux-packages`
