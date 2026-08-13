---
title: fixture — ticked live AC with no run evidence and no denied wording
---

Fixture for iter-154 (`shell_154_missing_run_evidence_fails`). Isolates Theme B from
Theme A: the AC names a real live test, uses no denied phrase, and simply omits the
`[verified: …]` annotation. Without this fixture a regression that disabled the
run-evidence requirement — e.g. an over-broad filename-stem exclusion — would leave
every other test green (PR #193 finding 9).

## Acceptance Criteria [1/1]

- [x] live_110_replace_never_kills_foreign_firefox: replacing a managed Firefox leaves a
      foreign instance untouched
