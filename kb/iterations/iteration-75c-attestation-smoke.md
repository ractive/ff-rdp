---
title: "Iteration 75c: PR-time smoke check that `gh attestation verify` works on a freshly built artifact"
type: iteration
date: 2026-05-24
status: in-progress
branch: iter-75c/attestation-smoke
depends_on:
  - iteration-75-security-hardening-defense-in-depth
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-75-security-hardening-defense-in-depth.md
first_call_sites: []
dogfood_path: |
  # Trigger a PR build and confirm the smoke job passed.
  gh pr checks --watch
  # Locally re-verify a downloaded artifact end-to-end.
  gh attestation verify ff-rdp-x86_64-apple-darwin.tar.gz --owner ractive
tags: [iteration, security, supply-chain, ci]
---

Carry-over from [[iteration-75-security-hardening-defense-in-depth]] task F.4.
Iter-75 wired `actions/attest-build-provenance` into the release workflow but
did **not** add a PR-time dry-run that proves `gh attestation verify` would
succeed against a newly built binary.

## Tasks

- [x] Add a `verify-attestation` job in `.github/workflows/ci.yml` that runs
      on `pull_request` and:
  - Builds the Linux-x86_64 binary.
  - Runs `actions/attest-build-provenance@v2` against it.
  - Runs `gh attestation verify <artifact> --owner ractive` and fails the PR
      if verification fails.
  - Code: `.github/workflows/ci.yml` (`verify-attestation` job)
- [x] Document the verification recipe in `README.md` under "Releases".
  - Code: `README.md` ("Verifying release artifacts" subsection under `## Releasing`)

## Acceptance Criteria [1/1]

- [x] `ci.attestation-smoke-passes`: the `verify-attestation` job in
      `.github/workflows/ci.yml` runs `attest-build-provenance` then
      `gh attestation verify ff-rdp-x86_64-unknown-linux-gnu.tar.gz --owner ractive`
      on a PR with no production changes and succeeds end-to-end.
  - Test evidence: the `verify-attestation` job in `.github/workflows/ci.yml`
    runs `actions/attest-build-provenance` then `gh attestation verify
    --owner ractive` (offline via bundle + online via the GitHub API) on the
    freshly built `ff-rdp-x86_64-unknown-linux-gnu.tar.gz` for this PR.
