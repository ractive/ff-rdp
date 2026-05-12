---
title: Dogfooding session 43 — iter-39 (squad-member + auth onboarding)
type: dogfooding
date: 2026-05-12
---

# Session 43 — iter-39 implementation, no ff-rdp use

## TL;DR

This iteration was implemented without using ff-rdp. Reproductions of the
§A items were inferred from the Notes.md trace + a code-only read of the
relevant components, not from live browser repro. Documenting that here so
the next person doesn't think ff-rdp was exercised silently.

## Why no ff-rdp

- §A.1 (row click no-op) — pinpointed from source by diffing
  `MyBookingsTable.tsx` vs `BookingsTable.tsx`: the former renders rows
  with no wrapping `<Link>`. No browser needed.
- §A.2 (confirm/decline on detail) — already implemented (page renders
  `AssignmentInlineActions` + `WithdrawAssignmentDialog`); the
  user-visible breakage was the §A.1 nav, not missing actions.
- §A.3 (ChunkLoadError) — classic deploy-skew symptom; the admin's SW is
  push-only (no fetch handler) and `register("/sw.js", { updateViaCache:
  "none" })` is correct. No code change; documented in PR.
- §A.4 (Manifest syntax error) — root cause obvious from `proxy.ts`:
  unauthenticated `/manifest.webmanifest` 302s to `/login`, browser
  parses the redirect HTML as JSON, logs the syntax error. Fixed by
  adding the path to `PUBLIC_PATH_RE`.

## Suggestion for next time

If §A had been more ambiguous, a 5-minute ff-rdp pass to capture the
console + network trace would have beaten a 30-minute code spelunking
session. Worth keeping `ff-rdp` in the loop even for fixes that "look
obvious" from source.
