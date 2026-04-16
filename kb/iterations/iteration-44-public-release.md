---
title: "Iteration 44: First Public Release (v0.1.0)"
date: 2026-04-13
type: iteration
status: in-progress
branch: iter-44/public-release
tags:
  - iteration
  - release
  - ci
  - cd
  - packaging
---

# Iteration 44: First Public Release (v0.1.0)

Ship `ff-rdp` publicly by **cloning hyalo's release pipeline verbatim** and swapping names. Everything in `~/devel/hyalo/` is already known to work end-to-end. No re-invention.

## Source of Truth

`~/devel/hyalo/` is the template. For every file under `.github/` and every crates.io-relevant field in `Cargo.toml`, **copy and do a mechanical `hyalo` → `ff-rdp` substitution**. The gap analysis that used to live in this file has been removed — hyalo is newer and more complete than ff-rdp's current workflows, so its versions win in every conflict.

## Tasks

### 1. Replace CI/Release workflows

- [x] `cp ~/devel/hyalo/.github/workflows/ci.yml .github/workflows/ci.yml`, then replace `hyalo` → `ff-rdp` and `hyalo-cli` → `ff-rdp-cli`.
- [x] `cp ~/devel/hyalo/.github/workflows/release.yml .github/workflows/release.yml`, then do the same substitutions. Specifically: binary name (`hyalo` → `ff-rdp`), package names (`hyalo-core` → `ff-rdp-core`, `hyalo-cli` → `ff-rdp-cli`), tap/bucket repo names (`ractive/homebrew-tap` stays, `ractive/scoop-hyalo` → `ractive/scoop-ff-rdp`), winget identifier (`ractive.hyalo` → `ractive.ff-rdp`), description strings.
- [x] `cp ~/devel/hyalo/.github/release.yml .github/release.yml` (no substitution needed — label taxonomy is generic).

### 2. Replace `deny.toml`

- [x] `cp ~/devel/hyalo/deny.toml deny.toml` if current differs materially; otherwise leave.

### 3. Align `Cargo.toml` metadata with hyalo

- [x] Diff `~/devel/hyalo/Cargo.toml` against `Cargo.toml`; port any missing workspace-level fields (`authors`, `homepage`, `readme`, `keywords`, `categories`, `description`).
- [x] Diff `~/devel/hyalo/crates/hyalo-core/Cargo.toml` and `hyalo-cli/Cargo.toml` against ours; port any missing crate-level fields the same way.
- [x] `cargo publish --dry-run --package ff-rdp-core --locked` succeeds.
- [x] `cargo publish --dry-run --package ff-rdp-cli --locked` succeeds.

### 4. Documentation polish

- [x] Update `README.md`: drop "Early development" wording, add install sections for Homebrew / Scoop / winget / `cargo install ff-rdp-cli` / binaries (mirror hyalo's README structure).
- [x] Copy `~/devel/hyalo/AI_NOTICE` if we want parity.

### 5. GitHub-side manual setup

See [[iterations/iteration-44-github-setup-guide]] for the step-by-step.

## Acceptance Criteria

- [x] CI green on the PR.
- [x] Cut `v0.1.0-rc.1` as a pre-release → all jobs succeed (or fail only on documented pre-release-incompatible jobs like winget).
- [x] Cut `v0.1.0` → `cargo install ff-rdp-cli`, `brew install ractive/tap/ff-rdp`, `scoop install ff-rdp` all work.

## Non-Goals

Whatever hyalo doesn't do, we don't do either.
