---
title: "Iteration 125: perf audit fabricates a false \"good\" 0 ms LCP where vitals says \"unavailable\""
type: iteration
date: 2026-07-19
status: in-progress
branch: iter-125/perf-audit-lcp-unavailable
depends_on: []
firefox_refs: []
kb_refs:
- kb/dogfooding/dogfooding-session-61.md
first_call_sites: []
dogfood_path: |
  # perf vitals and perf audit MUST agree on LCP on a page where LCP is unmeasurable
  # (comparis.ch class: no LCP entry, DOM approximation finds no load timing):
  ff-rdp --port <p> navigate https://www.comparis.ch
  ff-rdp --port <p> perf vitals --jq '.results | {lcp_ms, lcp_rating}'
  # expected: {"lcp_ms": null, "lcp_rating": "unavailable"}
  ff-rdp --port <p> perf audit --jq '.results.vitals | {lcp_ms, lcp_rating}'
  # expected: {"lcp_ms": null, "lcp_rating": "unavailable"}
  #           (was: {"lcp_ms": 0.0, "lcp_rating": "good"} — a false all-clear)
tags:
- iteration
- perf
- audit
- vitals
- firefox-152
- dogfood-61
---

# Iteration 125: perf audit fabricates a false "good" 0 ms LCP

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152), reproduced clean on a
single instance: on pages where LCP is unmeasurable (comparis.ch), `perf vitals` correctly
reports `lcp_ms: null, lcp_rating: "unavailable"` — but `perf audit` on the **same page**
reports `lcp_ms: 0.0, lcp_rating: "good"`. A regression agent could *not* repro on pages with
a measurable LCP (~587 ms); the bug is specific to the unmeasurable case — which means audit
produces a dangerous false all-clear on exactly the pages that need scrutiny.

Root cause: `run_vitals` and `run_audit` carry **duplicated** vitals logic that has drifted:

- `run_vitals` (`crates/ff-rdp-cli/src/commands/perf.rs:423`) applies the iter-83 N7 guard at
  `perf.rs:527-541`: `lcp_unavailable = lcp.is_none() || (lcp_approximate && lcp == 0.0)`,
  emitting `"unavailable"` + JSON null instead of rating a meaningless zero (`perf.rs:543-545`),
  plus `lcp_approximate`/`lcp_note` context at `perf.rs:555-568`.
- `run_audit` (`perf.rs:797`) rebuilds the same vitals block independently at `perf.rs:929-982`
  **without** the guard: `"lcp_ms": lcp, "lcp_rating": lcp.map(|v| rate(v, 2500.0, 4000.0))`
  (`perf.rs:962-963`). The collection JS's DOM approximation fallback (`perf.rs:822-856`)
  fabricates an LCP entry with `startTime: 0` when the candidate element has no resource
  timing, so `compute_lcp` (`perf.rs:1361`) returns `Some(0.0)`, `is_lcp_approximate`
  (`perf.rs:1371`) returns true — and `rate(0.0, …)` (`perf.rs:1494-1497`) rates it `"good"`.
- The bogus block ships at `.results.vitals` (`perf.rs:1106-1116`) and the text renderer
  prints the same lie via the `("LCP", "lcp_ms", "lcp_rating", "ms")` row (`perf.rs:1149`).

The vitals side is already pinned by `test_perf_vitals_emits_unavailable_when_lcp_missing`
(`perf.rs:1772`) and `perf_vitals_emits_unavailable_when_lcp_approximate` (`perf.rs:1814`);
audit has no such coverage — which is how the drift went unnoticed.

## Themes

- **A — One LCP-rating code path.** Extract the N7 unavailable-guard + note/approximate
  annotation from `run_vitals` into a shared `pub(crate)` helper in `perf.rs` and call it from
  both `run_vitals` and `run_audit`, so the two commands cannot drift again.
- **B — Prove parity.** Unit-test the audit block against the same missing/approximate-zero
  inputs as the vitals tests, and live-test that audit and vitals agree on a page with no
  measurable LCP.

## Tasks

### A. Shared LCP handling

- [x] Extract the `lcp_unavailable` computation + `lcp_ms`/`lcp_rating` selection
      (`perf.rs:527-541`) and the `lcp_approximate`/`lcp_note` annotation (`perf.rs:555-568`)
      into one `pub(crate)` helper (no new `pub` API) that both callers feed with
      `(lcp, lcp_approximate)`.
- [x] Replace the bare `lcp.map(|v| rate(…))` audit block (`perf.rs:957-982`) with the shared
      helper so `.results.vitals` in audit carries the identical
      `lcp_ms`/`lcp_rating`/`lcp_approximate`/`lcp_note` semantics as `perf vitals`.
- [x] Update `render_audit_text` (`perf.rs:1143`) so the LCP row renders `"unavailable"`
      (no `0 ms good`) when the rating is unavailable.

### B. Parity coverage

- [x] Add audit-side twins of the vitals unit tests (`perf.rs:1772`, `perf.rs:1814`):
      missing-LCP input and approximate-zero input must both yield
      `lcp_rating: "unavailable"`, `lcp_ms: null` in the audit vitals block.
- [x] Add a live parity test comparing `perf audit`'s `.results.vitals` LCP fields against
      `perf vitals` on the same page (extend the existing
      `live_dom_stats_perf_audit_parity` harness or add a sibling live test).

## Acceptance Criteria [4/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_perf_audit_lcp_unavailable: on a page with no measurable LCP (text-only local
      fixture page, no resource-timed image), `perf audit` reports
      `.results.vitals.lcp_rating == "unavailable"` and `.results.vitals.lcp_ms == null` —
      never `"good"` / `0.0`.
- [x] live_perf_audit_vitals_lcp_parity: on the same page in the same session, `perf audit`'s
      `.results.vitals.{lcp_ms, lcp_rating, lcp_approximate?}` equals `perf vitals`'
      `.results.{lcp_ms, lcp_rating, lcp_approximate?}` field-for-field.
- [x] unit_perf_audit_lcp_unavailable_matches_vitals: audit vitals block built from
      missing-LCP and approximate-zero inputs yields `"unavailable"`/null via the shared
      helper (twin of `perf.rs:1772` / `perf.rs:1814`).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Prefer the shared-helper extraction over copy-pasting the guard into `run_audit`: the bug
  *is* the duplication — iter-83 Theme F (N7) fixed vitals only because the audit copy was
  invisible to that change. See [[iteration-83-dogfood-55-real-fixes]].
- Measurable-LCP pages must be unaffected: the helper reduces to the current
  `rate(v, 2500.0, 4000.0)` path when `lcp` is Some(non-zero-or-real) — the regression
  agent's ~587 ms case is the guard's negative control.
- The other vitals fields (`fcp`, `ttfb`, `cls`, `tbt`) already agree between the two
  commands; only the LCP triple is in scope. Resist widening into a full vitals-block
  extraction unless it falls out for free.

## Out of scope

- Improving the DOM LCP approximation itself (`perf.rs:822-856`) — Firefox does not implement
  the LCP PerformanceObserver; the note text already documents this platform limitation.
- `perf summary` / `perf compare` — neither emits an LCP rating today.
- The network JSON shape bug and a11y contrast total bug from the same session — filed as
  [[iteration-126-network-json-shape-consistency]] and
  [[iteration-127-a11y-contrast-fail-only-total]].

## References

- [[dogfooding-session-61]]
- [[iteration-83-dogfood-55-real-fixes]]
- [[iteration-86-perf-field-report-fixes]]
