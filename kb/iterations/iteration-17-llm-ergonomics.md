---
title: "Iteration 17: LLM Ergonomics & Convenience"
status: completed
date: 2026-04-06
tags:
  - iteration
  - ux
  - llm
---

# Iteration 17: LLM Ergonomics & Convenience

From dogfooding session (2026-04-06): improvements that make ff-rdp more efficient
for LLM agents and power users. No bugs — all enhancements.

## Improvements

- [x] **LLM-optimized help output** — add a top-level `--llm` or `llm-help` subcommand
      that dumps all commands, flags, and usage examples in a single compact block. LLM
      agents currently need to call `help` on each subcommand individually. A single
      structured dump (JSON or markdown) would save many round-trips.

- [x] **`perf` summary mode** — add a `perf summary` subcommand that aggregates resource
      entries: total transfer size, number of requests by type (script, img, font, css),
      slowest resources, third-party domain breakdown. Currently an LLM must process the
      raw array itself.

- [x] **Third-party domain analysis** — add `perf --type resource --group-by domain` or
      similar to automatically group resources by domain and show counts/sizes. Very useful
      for privacy/tracking audits (comparis.ch loads from 10+ third-party domains).

- [x] **`dom` count mode** — add `--count` flag to return only the number of matching
      elements instead of all their content. Useful for quick structural queries like
      "how many scripts/images/forms on this page?"

- [x] **Navigation + wait combo** — add `navigate --wait-text` or `navigate --wait-selector`
      that combines navigation with waiting in a single call. LLM agents always call
      `navigate` then `wait` sequentially; merging them saves a round-trip and is less
      error-prone.

- [x] **`cookies` on consent-gated sites** — cookies returned empty on comparis.ch because
      the CMP consent banner hadn't been accepted. Consider adding a note in the output
      (e.g., `"note": "0 cookies found — consent banner may be blocking"`) or detecting
      CMP presence via common selectors.

- [x] **`cookies` output should include domain and path** — even when cookies are returned,
      the current output may lack the domain/path context needed to understand which cookies
      belong to which third-party. Verify and add if missing.

## Acceptance Criteria

- [x] At least 4 improvements are implemented
- [x] `ff-rdp llm-help` (or equivalent) returns a complete, structured reference in one call
- [ ] Re-run the dogfood session (ractive.ch + comparis.ch) end-to-end and verify the
      workflow is smoother with fewer round-trips
