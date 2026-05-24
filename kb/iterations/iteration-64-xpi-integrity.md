---
title: "Iteration 64: XPI integrity — vendor or pin Consent-O-Matic"
type: iteration
date: 2026-05-24
status: planned
branch: iter-64/xpi-integrity
depends_on:
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
first_call_sites: []
dogfood_path: |
  # 1. Auto-consent install still works end-to-end.
  ff-rdp launch --temp-profile --auto-consent
  ff-rdp navigate https://www.theguardian.com/   # consent banner dismissed

  # 2. Tampered XPI is rejected (simulated by editing the on-disk cache file
  #    then re-launching). Expect: "XPI hash mismatch, refusing to install".
  rm -rf ~/.cache/ff-rdp/extensions
  ff-rdp launch --temp-profile --auto-consent   # fresh download succeeds
  echo "tampered" >> ~/.cache/ff-rdp/extensions/gdpr@cavi.au.dk.xpi
  ff-rdp launch --temp-profile --auto-consent   # exits non-zero, "hash mismatch"
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
  preferred (Consent-O-Matic is MPL-2.0, licence-compatible).
- **B — Bound the download.** Independent of integrity, cap the download
  body size so a hostile server can't OOM the process.

## Tasks

### A. Pin or vendor the XPI
- [ ] Decide between (a) `include_bytes!` vendoring vs (b) SHA-256 pinning with download. Record the decision in `kb/decision-log.md`.
- [ ] If vendoring: drop the bytes into `crates/ff-rdp-cli/assets/extensions/consent-o-matic-1.1.5.xpi`; load via `include_bytes!`; remove `XPI_URL` and the ureq call. Add the upstream licence next to the file.
- [ ] If pinning: add `const XPI_SHA256: &str = "..."` next to `XPI_URL` (line 9); compute and verify after `read_to_vec()`; bail with a clear "hash mismatch" error if it differs.

### B. Cap the download
- [ ] If keeping the download path: add `.with_limit(10 * 1024 * 1024)` (or current ureq equivalent) to the `read_to_vec()` call at `commands/auto_consent.rs:56`. Test that a 12 MiB body is rejected.

## Acceptance Criteria [0/4]

- [ ] `xpi_integrity_verified_or_vendored`: either (a) `include_bytes!` is used and the source file is checked in with its licence, or (b) `verify_xpi_hash` returns `Err` on a tampered byte array and `Ok` on the pinned bytes.
- [ ] `xpi_download_capped`: if the download path is retained, `install_consent_o_matic` rejects a body > 10 MiB with a typed error.
- [ ] `live_auto_consent_install`: `ff-rdp launch --temp-profile --auto-consent` still installs and the test page sees the banner dismissed (existing live test path).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
