---
title: "Iteration 75c: PR-time smoke check that `gh attestation verify` works on a freshly built artifact"
type: iteration
date: 2026-05-24
status: planned
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

- [ ] Add a `verify-attestation` job in `.github/workflows/ci.yml` that runs
      on `pull_request` and:
  - Builds the Linux-x86_64 binary.
  - Runs `actions/attest-build-provenance@v2` against it.
  - Runs `gh attestation verify <artifact> --owner ractive` and fails the PR
      if verification fails.
- [ ] Document the verification recipe in `README.md` under "Releases".

## Acceptance Criteria [0/1]

- [ ] `ci.attestation-smoke-passes`: a CI run on a PR with no production
      changes succeeds end-to-end, including the verification step.
