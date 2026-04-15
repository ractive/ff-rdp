---
title: "Iteration 44: GitHub Manual Setup Guide"
date: 2026-04-13
type: guide
status: planned
tags: [release, github, secrets, setup, manual, iteration-44]
---

# GitHub Manual Setup Guide — v0.1.0 Release

Since ff-rdp reuses the same pipeline as hyalo under the same GitHub user (`ractive`), most secrets and repos already exist. This guide lists only what's **new or ff-rdp-specific**.

## What You Already Have (from hyalo)

These carry over unchanged — verify they exist, don't recreate:

- `ractive/homebrew-tap` repo → reused, new formula `Formula/ff-rdp.rb` lands next to `hyalo.rb`.
- PAT behind `HOMEBREW_TAP_TOKEN` → reuse the **same** token value. Just add it as a secret on `ractive/ff-rdp` (secrets are per-repo, not per-user).
- Forked `ractive/winget-pkgs` → reused.
- Classic PAT behind `WINGET_TOKEN` → reuse value, add to `ff-rdp` repo secrets.
- crates.io account → reused. You can reuse the same `CARGO_TOKEN` **if** its crate scope isn't restricted to `hyalo-*`; otherwise generate a new one scoped to `ff-rdp-*`.

## What's New for ff-rdp

### 1. Scoop bucket repo

Hyalo's bucket is `ractive/scoop-hyalo`. ff-rdp needs its own.

1. Create empty public repo `ractive/scoop-ff-rdp` with a `bucket/` folder and a short `README.md` showing `scoop bucket add ff-rdp https://github.com/ractive/scoop-ff-rdp && scoop install ff-rdp`.
2. PAT behind `SCOOP_BUCKET_TOKEN`: either widen the existing hyalo one's repo scope to include `scoop-ff-rdp`, or create a new fine-grained PAT (`Contents: Read and write` on `ractive/scoop-ff-rdp` only).
3. Add it as a secret on `ractive/ff-rdp`.

### 2. crates.io name availability

Confirm `ff-rdp-core` and `ff-rdp-cli` are free:

```
cargo search ff-rdp-core
cargo search ff-rdp-cli
```

### 3. Repo secrets on `ractive/ff-rdp`

At https://github.com/ractive/ff-rdp/settings/secrets/actions, confirm all four exist:

- [ ] `CARGO_TOKEN`
- [ ] `HOMEBREW_TAP_TOKEN`
- [ ] `SCOOP_BUCKET_TOKEN`
- [ ] `WINGET_TOKEN`

### 4. Labels for auto-changelog

Copy the label set from `ractive/hyalo` to `ractive/ff-rdp`. Names must match `.github/release.yml`: `breaking-change`, `enhancement`, `feature`, `bug`, `fix`, `performance`, `documentation`, `chore`, `dependencies`, `refactor`, `ignore-for-release`.

(`gh label clone ractive/hyalo --repo ractive/ff-rdp` does this in one shot.)

### 5. Repo settings

Match what hyalo has. Key ones: branch protection on `main` requiring `fmt`/`clippy`/`test-*` checks; "Automatically delete head branches" on; squash merging **off** (project policy — see `CLAUDE.md`).

## Cutting the Release

1. Merge the iteration-43 PR into `main`.
2. Cut `v0.1.0-rc.1` as a **pre-release** via the GitHub UI — verify all jobs go green (except winget, which may refuse pre-release tags).
3. Cut `v0.1.0` as a normal release.
4. Verify: `cargo install ff-rdp-cli`, `brew install ractive/tap/ff-rdp`, `scoop bucket add ff-rdp https://github.com/ractive/scoop-ff-rdp && scoop install ff-rdp`. winget PR opens upstream and merges whenever Microsoft processes it.

## Troubleshooting

Same failure modes as hyalo's first release — if something breaks, look at how hyalo handled it in git history for `ractive/hyalo`.
