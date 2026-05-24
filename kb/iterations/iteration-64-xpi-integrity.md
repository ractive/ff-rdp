---
title: "Iteration 64: XPI integrity — vendor or pin Consent-O-Matic"
type: iteration
date: 2026-05-24
status: in-review
branch: iter-64/xpi-integrity
depends_on:
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
first_call_sites: []
dogfood_path: |
  # 1. Auto-consent install still works end-to-end with the vendored XPI.
  ff-rdp launch --temp-profile --auto-consent
  ff-rdp navigate https://www.theguardian.com/   # consent banner dismissed

  # 2. No AMO network call happens at launch time (the vendored bytes are
  #    embedded via include_bytes!). Confirm with a network monitor or by
  #    running offline:
  ff-rdp launch --temp-profile --auto-consent    # succeeds with no internet
  # The installed file is the vendored XPI:
  ls "$(ff-rdp profile path)/extensions/gdpr@cavi.au.dk.xpi"

  # 3. Tamper-detection is now compile-time: editing the vendored asset on
  #    disk without updating XPI_SHA256_HEX fails
  #    `vendored_xpi_matches_pinned_sha256`, and editing it without
  #    updating LICENSE-consent-o-matic.txt fails
  #    `vendored_xpi_provenance_file_is_in_sync`. Both run in `cargo test`.
tags: [iteration, security]
---

# Iteration 64: XPI integrity — vendor or pin Consent-O-Matic

`auto_consent` downloads the Consent-O-Matic XPI over HTTPS from AMO and
writes it directly into the Firefox profile with no SHA-256 pin, no
signature verification beyond TLS, and no size cap on `read_to_vec()`. A
MITM (compromised CA, hostile network, AMO compromise) substitutes the XPI
for a malicious WebExtension that then runs in the user's Firefox with full
WebExtension permissions. This is the highest-impact finding from the
security review — RCE in the user's browser via a single network swap.

## Themes

- **A — Pin or vendor.** Either embed a known-good SHA-256 of the pinned XPI
  version and verify after download, OR vendor the XPI bytes via
  `include_bytes!` and remove the network call entirely. The latter is
  preferred (Consent-O-Matic is MIT-licensed, licence-compatible — see
  `crates/ff-rdp-cli/assets/extensions/LICENSE-consent-o-matic.txt`).
- **B — Bound the download.** Independent of integrity, cap the download
  body size so a hostile server can't OOM the process.

## Tasks

### A. Pin or vendor the XPI
- [x] Decide between (a) `include_bytes!` vendoring vs (b) SHA-256 pinning with download. Record the decision in `kb/decision-log.md`. → [[decision-log#DEC-017]] picks (a) vendoring.
- [x] Vendoring: bytes live at `crates/ff-rdp-cli/assets/extensions/consent-o-matic-1.1.5.xpi`; loaded via `include_bytes!` in `auto_consent::XPI_BYTES`; `XPI_URL` + ureq call removed. Upstream MIT licence + provenance (source URL, SHA-256) in `LICENSE-consent-o-matic.txt` next to the file.

### B. Cap the download
- [x] [deferred — not applicable: task A vendored the XPI via `include_bytes!`, removing the network download entirely. No request body remains to cap.]

## Acceptance Criteria [4/4]

- [x] `xpi_integrity_verified_or_vendored`: `include_bytes!` is used (`auto_consent::XPI_BYTES`), and `assets/extensions/consent-o-matic-1.1.5.xpi` + `LICENSE-consent-o-matic.txt` are checked in. Test `vendored_xpi_matches_pinned_sha256` asserts the byte hash stays pinned to `a2119abc329638d6e7af1ab4e5548a348465e02eec11de08dee0af84919923dc`.
- [x] `xpi_download_capped`: [deferred — not applicable: vendoring (task A) removed the download path entirely; there is no remaining body to cap.]
- [x] `live_auto_consent_install`: `install_writes_xpi_into_profile_extensions_dir` covers the file-placement path; the dogfood block at the top of this plan exercises the full live `--auto-consent` launch.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

Vendoring is the safer call: it removes the network step entirely from a
security-sensitive path. The downside is a ~200 KB binary blob in the
crate, and manual re-vendoring on upstream XPI bumps. The upstream is slow-
moving (last version is 1.1.5), so the maintenance cost is low.

If we go with pinning, the SHA-256 must come from a trusted source (AMO
listing page over HTTPS, then verified out of band — never trust the same
URL that delivered the bytes).

## Out of scope

- Sandboxing the Firefox process beyond what `--temp-profile` already
  achieves.
- A general "verified extension" install primitive for other extensions
  (file a follow-up if a second extension ever appears).

## References

- [[iteration-63-daemon-lockrecover-and-quick-sec-fixes]]
- Security review report (2026-05-24), finding F-3
- Consent-O-Matic on AMO: https://addons.mozilla.org/firefox/addon/consent-o-matic/
