---
title: "Iteration 75: Security hardening — defense in depth (bulk cap, profile dir, supply chain)"
type: iteration
date: 2026-05-24
status: completed
branch: iter-75/security-hardening-defense-in-depth
depends_on:
  - iteration-72-transport-polish
  - iteration-73-spec-fidelity-gates
firefox_refs:
  - lines: 138-200
    path: devtools/shared/transport/transport.js
    why: >-
      Bulk-packet send/receive contract — confirms the byte-length header is the only
      gate; ff-rdp must apply its own cap on receive.
  - lines: 490-512
    path: devtools/shared/transport/transport.js
    why: "onBulkPacket read path: streaming bytes, no upper bound enforced server-side; loopback peer cannot be trusted to honour our limits."
  - lines: 11-22
    path: devtools/shared/specs/heap-snapshot-file.js
    why: >-
      BULK_RESPONSE example — heap-snapshot transfer is the realistic large-bulk path
      the cap must accommodate but not be infinite for.
kb_refs:
  - kb/rdp/protocol/transport.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: ff_rdp_cli::util::profile_dir::secure_profile_root
    site: >-
      crates/ff-rdp-cli/src/commands/launch.rs (replaces env::temp_dir() at the
      temp-profile creation site)
  - primitive: ff_rdp_core::transport::BulkFrameTooLarge
    site: >-
      crates/ff-rdp-core/src/transport.rs (returned from recv_bulk_frame when the
      announced length exceeds max_frame_bytes())
dogfood_path: |
  # 1. Bulk-frame cap rejects oversized announcements promptly.
  ff-rdp --max-frame-mb 8 memory heap-snapshot https://example.com /tmp/snap.bin
  # If snapshot > 8 MiB, expect BulkFrameTooLarge error within ~50ms, not a hung reader.
  
  # 2. Profile lives under the secure state dir, mode 0700 on Unix.
  ff-rdp launch --headless https://example.com &
  ls -ld ~/.local/state/ff-rdp/profiles/*    # mode drwx------; owner = $USER
  # On Windows: %LOCALAPPDATA%\ff-rdp\profiles\<rand> with ACLs restricted to current user.
  
  # 3. Re-validated URL after a cross-scheme redirect logs a warning.
  ff-rdp --log-rdp-trace navigate https://example.com/redirect-to-file
  grep 'scheme changed' ~/.cache/ff-rdp/rdp-trace.log
  
  # 4. Verify release attestation + SBOM on a tagged build (CI dry-run).
  gh attestation verify dist/ff-rdp-x86_64-apple-darwin --owner ractive
  ls dist/*.cdx.json   # CycloneDX SBOM present per artifact.
tags:
  - iteration
  - security
  - supply-chain
---

Seven defense-in-depth gaps surfaced in the security review. None are
known-exploitable today (ff-rdp is a localhost CLI talking to a
user-launched Firefox), but each is the kind of thing that turns into
a CVE when someone runs ff-rdp under a different threat model
(daemon-mode on a multi-user box; remote Firefox; a CI runner with
attacker-controlled redirects). This iter knocks them all down in one
bundle because they're individually too small to justify a PR each.

## Themes

- **A — Bulk frame size cap (M-1).** `recv_bulk_frame` currently
  trusts the wire-announced length.
- **B — Secure temp-profile path (H-1).** Move out of `env::temp_dir()`.
- **C — Forbid `unsafe` in core (L-9).**
- **D — JSON depth bomb regression test (M-2).**
- **E — Post-navigate URL re-validation (Hg-8).**
- **F — Release supply chain: attestation + SBOM + yanked-deny (Hg-3, Hg-2).**

## Tasks

### A. Bulk frame cap
- [x] In `crates/ff-rdp-core/src/transport.rs:792-808` (`recv_bulk_frame`), compare the parsed `length: u64` against `max_frame_bytes()` (the iter-72 knob) before allocating any buffer or advancing the reader. Return a new `RdpError::BulkFrameTooLarge { announced, cap }` (use `thiserror` per the core-crate rule).
- [x] Same cap on the outbound side in `send_bulk_frame` (refuse to send larger than our own cap; surfaces local bugs).
- [x] Wire CLI to map the error to a clean exit-code-78 message.

### B. Secure temp-profile path
- [x] Add `crates/ff-rdp-cli/src/util/profile_dir.rs` with `pub fn secure_profile_root() -> Result<PathBuf>`. Resolution: `dirs::state_dir().or_else(dirs::data_local_dir).context("no state dir")?.join("ff-rdp/profiles")`. On Unix, create with mode `0o700`; on Windows, restrict ACLs to the current SID using `windows-sys` `SetNamedSecurityInfoW` (or document why default per-user `%LOCALAPPDATA%` ACLs suffice and link to MS docs).
- [x] Replace `env::temp_dir()` at `crates/ff-rdp-cli/src/commands/launch.rs:246-253`. Sub-directory name remains a random 16-char hex via `getrandom`.
- [x] Cleanup hook: existing `Drop` impl on the launch handle remains; verify it still removes the directory tree.

### C. Forbid unsafe in core
- [x] Add `#![forbid(unsafe_code)]` at the top of `crates/ff-rdp-core/src/lib.rs`. (Pre-check: `rg -n "unsafe" crates/ff-rdp-core/src` should be empty after this; CLI's daemon/process.rs + script/vars.rs continue to use `unsafe` and stay as-is.)
- [x] If clippy surfaces any non-FFI unsafe that escaped review, file it as carry-over — do not silence with `allow_unsafe_code`.

### D. JSON depth-bomb regression
- [x] Add `transport_rejects_deep_json` to `crates/ff-rdp-core/src/transport.rs` tests: feed a 200-level nested JSON object via the framed reader; assert `RdpError::Json` (or whatever serde_json returns at recursion limit), NOT a panic and NOT stack overflow.

### E. Post-navigate URL re-validation
- [x] In the `tabNavigated` event handler in `crates/ff-rdp-core/src/actors/tab.rs`, after parsing the new URL, call `url_validation::validate_url_with_opts` against the *original* policy. If the scheme changed (e.g. http→file, https→javascript), emit a `tracing::warn!` with both URLs. Do not abort — Firefox already blocks the dangerous transitions; this is observability so a user notices.
- [x] Tests: `tab_navigated_scheme_change_warns` — unit test with synthetic `tabNavigated` packets.

### F. Release supply chain
- [x] In `.github/workflows/release.yml`, add an `actions/attest-build-provenance@v2` step after each artifact build. Requires `id-token: write` + `attestations: write` permissions block.
- [x] Add a `cargo cyclonedx` step (or `cyclonedx-bom` cargo plugin) per platform; upload `*.cdx.json` as release assets alongside the binary.
- [x] Update `deny.toml`: add `[advisories] yanked = "deny"` (currently absent or `warn`).
- [ ] CI smoke: a dry-run job that runs `gh attestation verify` against the artifact on PR (using `actions/attest-build-provenance`'s preview verify flow, or a local cosign re-verify). [deferred — new plan: kb/iterations/iteration-75b-attestation-smoke.md]

## Acceptance Criteria [10/10]

- [x] `live_bulk_frame_oversize_rejected`: `crates/ff-rdp-cli/tests/live_bulk_cap.rs::live_bulk_frame_oversize_rejected` — connects to a local mock that announces `bulk … length:<2*max_frame>`, asserts `BulkFrameTooLarge` returned promptly, no body bytes read.  Gated `FF_RDP_LIVE_TESTS=1` (uses mock server, not Firefox).
- [x] `bulk_frame_cap_send_side`: `crates/ff-rdp-core/src/transport.rs::bulk_frame_cap_send_side` — unit test, `check_outbound_bulk_size` refuses an oversize length.
- [x] `secure_profile_root_mode_0700`: `crates/ff-rdp-cli/src/util/profile_dir.rs::secure_profile_root_mode_0700` — Unix-only (`#[cfg(unix)]`) unit test asserts created directory has mode 0o700 and is under `dirs::state_dir()` / `data_local_dir()`.
- [x] `secure_profile_root_windows_per_user`: `#[cfg(windows)]` unit test that the directory is under `%LOCALAPPDATA%\ff-rdp\profiles` and exists.
- [x] `core_lib_forbids_unsafe`: `crates/ff-rdp-core/src/lib.rs::core_lib_forbids_unsafe` — pinned via an `include_str!`-based test that asserts the `#![forbid(unsafe_code)]` attribute is present in `lib.rs`.
- [x] `transport_rejects_deep_json`: `crates/ff-rdp-core/src/transport.rs::transport_rejects_deep_json` — 200-level nested input returns `InvalidPacket`, does not panic or stack-overflow.
- [x] `tab_navigated_scheme_change_warns`: `crates/ff-rdp-core/src/actors/tab.rs::tab_navigated_scheme_change_warns` — synthetic `tabNavigated` with scheme delta exercises the warn-level branch in `note_tab_navigated_scheme_change`.
- [x] `workspace_lint_and_test_clean`: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` all pass cleanly on the branch tip.
- [x] `release_yml_has_attest_and_sbom`: `.github/workflows/release.yml` contains `actions/attest-build-provenance@…` and `cargo cyclonedx` steps for non-cross matrix entries.
- [x] `deny_toml_yanked_deny`: `deny.toml` `[advisories]` block contains `yanked = "deny"`.

## Design notes

Bulk cap uses the same `max_frame_bytes()` knob as JSON frames
(iter-72). One number for both is intentional: operators reason about
"the largest frame ff-rdp will accept" without having to track two
limits. Heap-snapshot users who legitimately need >256 MiB can raise
`--max-frame-mb` — the cap is on the *trusted* default, not the
maximum the codepath supports.

The Windows ACL story is the soft spot. `%LOCALAPPDATA%` is already
per-user with default deny for Everyone, so the simplest correct
answer is "create the directory and rely on inheritance"; we'll
document that decision in `profile_dir.rs` with a link to Microsoft's
"Default ACLs for user profile folders". If a reviewer wants explicit
SDDL we can layer it on later — that's a follow-up, not a blocker.

URL re-validation is observability not enforcement: Firefox already
blocks `http→file` etc. The point is the *user* should know
ff-rdp followed a redirect that crossed a scheme boundary, because
their automation may not expect it.

## Out of scope

- Confidentiality of the loopback socket — TCP-on-localhost remains
  the transport per Firefox.
- A formal threat model document. (`kb/security/` carries the
  informal one.)
- Sigstore-keyed signing — `attest-build-provenance` already provides
  Sigstore-backed attestations; adding a separate signing key is
  noise.

## References

- [[iteration-72-transport-polish]] (the `max_frame_bytes` knob)
- [[iteration-73-spec-fidelity-gates]]
- Security review report (2026-05-24): M-1, H-1, L-9, M-2, Hg-8, Hg-3, Hg-2
- `kb/rdp/protocol/transport.md`
