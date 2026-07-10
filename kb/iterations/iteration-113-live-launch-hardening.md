---
title: "Iteration 113: live-launch hardening — launch timeout, mandatory #[ignore] guard, eol pinning"
type: iteration
date: 2026-07-10
status: completed
branch: iter-113/live-launch-hardening
depends_on:
  - kb/iterations/iteration-112-windows-live-firefox-launch-hang.md
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-110-post-batch-live-sweep.md
first_call_sites: []
dogfood_path: |
  # Guard rejects ungated live tests (inject a bare #[test] to see it fail):
  cargo run -p xtask -- check-live-test-layout
  # Plain test run stays fast and Firefox-free:
  cargo test -p ff-rdp-cli --test live
tags:
  - iteration
  - tests
  - ci
  - windows
---

# Iteration 113: live-launch hardening

Carry-over of [[iteration-112-windows-live-firefox-launch-hang]]'s remaining
scope. The CI-unblocking half (gating 8 bare `#[test]` live_61l tests +
CRLF-tolerant `unit_error_enums_non_exhaustive`) already landed as hotfix
PR #148; what remains is making the whole failure class impossible.

## Execution policies (standing, per James)

Scoped testing: while developing, run only the tests affected by the change;
one full `cargo test --workspace -q` in the final pre-PR gates; the review
agent does not re-run the full workspace suite. Live tests: only the ones
this plan's ACs name (this iteration's changes are harness/guard-level — no
full-suite run; the next batch sweep covers integration). CI-wait: merge on
required lanes; `test (windows-latest)` is expected GREEN since PR #148 —
any windows failure is real and blocks.

## Theme A — Firefox launch timeout in the live harness

The `LiveFirefox` helper waits indefinitely for the debugger port when
Firefox is absent or wedged — that is what turned ungated tests into 10-min
CI timeouts instead of immediate failures. Add a bounded wait (default ~30s,
env-overridable) that panics with a clear message naming the binary path and
port it waited on.

## Theme B — `check-live-test-layout` enforces `#[ignore]`

Extend the xtask check (from [[iteration-100b-live-test-consolidation]]):
every `#[test]` under `crates/ff-rdp-cli/tests/live/` must carry `#[ignore]`.
The two intentionally-unignored runtime-gated fast probes must either gain
`#[ignore]` (preferred if nothing depends on them running by default) or an
explicit `// allow-ungated-live: <reason>` annotation the check understands.
Wire stays in check-iteration-ready + the CI discipline job (already
consuming the check — no new pub surface).

Note (iter-111): a new file landed in this directory since this plan was
written — `live_111_daemon_follow_cross_process.rs` — and correctly follows
the manual `#[ignore = "requires a live Firefox instance — set
FF_RDP_LIVE_TESTS=1"]` convention this theme aims to make mandatory. Use it
(alongside the existing `live_61l.rs` set) as a positive fixture when writing
the layout-guard's test: the guard must pass on the current tree without
requiring any change to this file.

## Theme C — pin line endings

Add `.gitattributes` (`* text=auto eol=lf`) so Windows checkouts stop
producing CRLF sources; keep the iter-112 CRLF normalization in
`unit_error_enums_non_exhaustive` as belt-and-braces. Verify the windows
lane stays green on the PR.

## Acceptance Criteria [3/3]

- [x] `launch_times_out_fast`: a harness test pointing the live launcher at an
      unreachable/nonexistent Firefox fails within the bounded wait (not
      indefinitely) with a message naming binary + port.
- [x] `layout_guard_rejects_ungated_test`: xtask unit test proves
      check-live-test-layout fails when a bare `#[test]` (no `#[ignore]`, no
      allow annotation) is injected under tests/live/, and passes on the
      current tree.
- [x] `eol_pinned_windows_green`: `.gitattributes` lands and CI job
      `test (windows-latest)` passes on this PR's final head (0 failures).
