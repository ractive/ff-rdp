---
name: project-flaky-redact-tests
description: transport::tests::redact_* tests race on global REDACT_THRESHOLD when run via a narrow test-name filter
metadata:
  type: project
---

`crates/ff-rdp-core/src/transport.rs`'s `redact_*` tests (around line
1600+, e.g. `redact_sensitive_key_replaces_value`) share global mutable
state (`REDACT_THRESHOLD` via `REDACT_LOCK`). Some tests in that group
lock `REDACT_LOCK` before mutating the threshold (e.g.
`redact_long_string_replaces_value`) but others don't, assuming the
default threshold.

**Why it matters:** Running `cargo test -p ff-rdp-core --lib
transport::tests::redact -q` (a narrow substring filter) reliably fails
4-5 of these tests, on both `origin/main` and unrelated feature branches
— confirmed pre-existing, not caused by any single PR. It reproduces
because the filtered subset changes which tests run concurrently/in what
order, exposing the missing lock. The full `cargo test --workspace -q`
run does NOT reliably reproduce it (passed 3/3 times in one investigation
session) because the full suite's thread scheduling dilutes the race.

**How to apply:** Do not treat a red result from a narrow
`cargo test ... -- some::filter` run as a regression without first
reproducing it against `origin/main` with the same filter. If it
reproduces on main too, it's this pre-existing issue — file/link a
follow-up iteration to add `let _g = REDACT_LOCK.lock().unwrap();` to
every `redact_*` test that reads `redact_threshold()`, rather than
blocking the current PR on it. Always trust `cargo test --workspace -q`
(the actual CI gate) over a filtered subset for pass/fail decisions.
