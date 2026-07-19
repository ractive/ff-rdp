---
title: "Iteration 126: network / navigate --with-network JSON shape flips between object and bare array"
type: iteration
date: 2026-07-19
status: done
branch: iter-126/network-json-shape-consistency
depends_on: []
firefox_refs: []
kb_refs:
- kb/dogfooding/dogfooding-session-61.md
first_call_sites: []
dogfood_path: |
  # The network payload must have ONE canonical object shape on quiet AND busy pages:
  ff-rdp --port <p> navigate https://example.com --with-network \
    --jq '.results.network | {t: (.entries|type), n: .total_requests}'
  # expected: {"t":"array","n":<small N>}   (was: "cannot index array with \"entries\"")
  ff-rdp --port <p> navigate https://www.comparis.ch --with-network \
    --jq '.results.network | {t: (.entries|type), n: .total_requests}'
  # expected: same shape — {"t":"array","n":~110}; never a bare ~13KB array dump,
  #           never a shape flip between the two pages
tags:
- iteration
- network
- navigate
- output-contract
- firefox-152
- dogfood-61
---

# Iteration 126: network JSON shape flips between object and bare array

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152), reproduced clean on a
single instance: `navigate --with-network` (and the standalone `network` command) returns an
**inconsistent JSON shape** — an object `{entries, total_requests, …}` on busy pages but a
bare array on quiet ones. Consequences:

- `.results.network.entries` / `.results.network.total_requests` throw
  `cannot index array` half the time — consumers must probe the type before every access.
- Documented summary fields (`total_requests`, `total_transfer_bytes`, `slowest`, help text at
  `crates/ff-rdp-cli/src/cli/args.rs:555`) are unreachable via `--jq`, because `--jq` itself
  forces the detail path (see below).
- The bare-array path re-serializes the whole entry list (~110 entries, ~13 KB on comparis)
  to stdout when the caller only wanted counts.

Root cause: `apply_network_controls`
(`crates/ff-rdp-cli/src/commands/navigate.rs:1291-1337`) returns **three different shapes**
from one function:

1. Detail mode (`--detail`/`--jq`/`--sort`/`--limit`/`--all`/`--fields`,
   `navigate.rs:1296-1301`) **and** truncated (>20 entries after the default limit,
   `navigate.rs:1320`) → object `{entries, shown, total, truncated, hint}`
   (`navigate.rs:1322-1330`). This is the "busy page" shape.
2. Detail mode, not truncated (quiet page, or `--all`) → bare `json!(limited)` array
   (`navigate.rs:1331-1333`). This is the "quiet page" shape — and also the `--all` shape,
   which is how the full ~110-entry array ends up dumped.
3. Non-detail → `build_network_summary` object `{total_requests, total_transfer_bytes,
   by_cause_type, slowest, …}` (`navigate.rs:1334-1336`, builder at
   `crates/ff-rdp-cli/src/commands/network.rs:638-706`) — but since `--jq` flips to detail
   mode, this documented shape is exactly the one `--jq` users can never reach.

The result lands at `.results.network` (`navigate.rs:1130`, `navigate.rs:1245-1249`). The
standalone `network` command has the same object/array divergence on `.results`: detail mode
(same trigger list, `network.rs:262-269`) emits an array envelope via
`envelope_with_truncation` (`network.rs:384-402`, builder at
`crates/ff-rdp-cli/src/output.rs:28`), summary mode emits the summary object
(`network.rs:416-435`, builder at `output.rs:17`).

## Themes

- **A — One canonical object shape, always.** `apply_network_controls` returns a single
  object `{entries: […], total_requests, shown, total, truncated, …summary fields}` on every
  path — truncated or not, busy or quiet, `--all` or default — with summary fields present
  even when `entries` is empty.
- **B — Align the standalone `network` command and the contract docs.** `network` detail mode
  carries the same summary fields alongside `results`, and the help text
  (`args.rs:555` and the navigate `--with-network` sections) documents the canonical shape
  with a backward-compat note for consumers of the old bare-array form.

## Tasks

### A. Canonical shape in navigate --with-network

- [x] Rework `apply_network_controls` to always return one object: it now delegates to the
      shared `build_canonical_network` builder on BOTH the detail and summary branches, so
      the truncated, non-truncated (was bare array), and summary paths converge on the same
      key set `{entries, shown, total, truncated, total_requests, total_transfer_bytes,
      by_cause_type, slowest, timeout_reached, …}`.
- [x] Keep the default entry limit (20) on the detail path so the canonical shape does not
      reintroduce the ~13 KB dump; `--all` still expands `entries` but keeps the summary
      fields and `total_requests` alongside (summary counts come from the full capture, not
      the truncated view).
- [x] `build_network_summary` already yields sane zero values for an empty slice; the
      canonical object on a zero-request page carries `entries: []` and `total_requests: 0`
      rather than omitting keys — asserted by e2e `navigate_with_network_empty_when_no_events`
      and unit `build_canonical_network_empty_keeps_all_keys`.
- [x] Reused the existing real-Firefox-recorded fixtures (the canonical shape is derived from
      the same recorded entries, so no re-record was needed) and updated the shape assertions
      in `crates/ff-rdp-cli/tests/e2e/navigate.rs` (added `..._detail_mode_is_object_not_array`,
      `..._all_keeps_object_shape`) and `tests/e2e/network.rs` (summary fields in
      `network_detail_shows_requests`). Busy/truncated + quiet-page shapes exercised live in
      `live_126_network_shape.rs`.

### B. Standalone network command + contract docs

- [x] Extended the `network` detail envelope with the same summary fields via
      `merge_summary_fields` (computed from the full capture `summary_source`) so `--jq` users
      are not cut off from `total_requests`/`total_transfer_bytes`/`slowest` by the detail-mode
      trigger.
- [x] Updated the help text: the summary-shape line (`args.rs`) plus the
      `navigate --with-network` `long_about` section now describe the canonical object and
      carry a one-line backward-compat note ("previously a bare array in non-truncated detail
      mode").

## Acceptance Criteria [5/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_navigate_with_network_shape_quiet_and_busy (quiet half): `navigate --with-network
      --jq` on a quiet page (example.com, ≤20 requests) yields `.results.network.entries` of
      type array and a numeric `.results.network.total_requests` — no bare array, no `cannot
      index array`. Also covered by mock e2e `navigate_with_network_captures_requests` and
      unit `build_canonical_network_carries_entries_and_summary`.
- [x] live_navigate_with_network_shape_quiet_and_busy (busy half): the same invocation on a
      busy page (wikipedia, >20 requests) yields the identical key set, with `truncated ==
      true`, `shown == 20`, and `total_requests >= total` — quiet/busy key sets asserted equal
      via `network_keys()`. Truncation math covered by unit
      `build_canonical_network_truncated_summary_reflects_full_capture`.
- [x] live_navigate_with_network_all_keeps_summary: adding `--all` still returns the object
      shape (full `entries`, summary fields intact) — never a bare array. Mock e2e:
      `navigate_with_network_all_keeps_object_shape`.
- [x] live_network_detail_carries_summary: standalone `network --jq` returns summary fields
      (`total_requests`, `total_transfer_bytes`) alongside the entry list on a page with
      captured traffic. Mock e2e: `network_detail_shows_requests`; parity unit
      `network_and_navigate_summary_fields_agree_field_for_field`.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- This is an **output-contract fix**, not a feature: pick the object as the one canonical
  shape because it is the only form that can carry both entries and summary fields, and it
  matches what the `--help` text already promises (`args.rs:555`). The bare-array form
  (`navigate.rs:1332`) is the accident.
- Envelope discipline: standalone `network` keeps the standard
  `{results, total, meta}` envelope (`output.rs:17`/`output.rs:28`); the summary fields ride
  inside it rather than inventing a second envelope. For `navigate`, the canonical object is
  the value of the existing `network` key — no change to the navigate envelope itself.
- `--fields` interaction: field projection applies to `entries` items only, never to the
  summary keys, so `--fields url` cannot strip `total_requests`.
- Daemon-drain and `--follow` streaming paths are unaffected: the shape flip lives entirely
  in the final serialization, not in event collection
  (`build_network_entries`, `drain_network_events`).
- Precedent from [[iteration-125-perf-audit-lcp-unavailable]]: that iteration fixed an
  analogous drift (`perf vitals` vs `perf audit` disagreeing on LCP) by extracting the
  divergent logic into one shared `pub(crate)` helper (`apply_lcp_fields`) that both call
  sites feed identically, plus unit tests asserting the two outputs are equal for the same
  inputs — not just tests of each path in isolation. Apply the same shape here: prefer a
  shared `pub(crate)` builder that `apply_network_controls` (navigate) and the standalone
  `network` command both call, and add an explicit parity assertion (unit and/or live) that
  the two commands' shapes agree field-for-field on the same page, mirroring
  `unit_perf_audit_lcp_unavailable_matches_vitals` / `live_perf_audit_vitals_lcp_parity`. This
  also confirms the `firefox_refs:`/line-number citations in this plan should be re-verified
  against current `main` before starting Task A, since line numbers drift between iterations
  even when the referenced code is untouched by unrelated PRs.

## Out of scope

- The ~7 s default-navigate wait — fixed in [[iteration-122-navigate-dom-complete-ff152]]
  (this bug was explicitly split out from that plan's Out of scope).
- `network --follow` NDJSON streaming format — separate contract, already line-oriented.
- Header/security enrichment (`network.rs:307-374`) — orthogonal to the shape.

## References

- [[dogfooding-session-61]]
- [[iteration-122-navigate-dom-complete-ff152]]
