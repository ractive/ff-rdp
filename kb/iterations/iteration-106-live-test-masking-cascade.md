---
title: "Iteration 106: live-test masking cascade — chrome CSP bypass regression, DNS-failure error shape, tabs-vs-eval daemon routing audit"
type: iteration
date: 2026-07-09
status: planned
branch: iter-106/live-test-masking-cascade
depends_on:
  - iteration-100-daemon-lifecycle-hardening
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-100-daemon-lifecycle-hardening.md
first_call_sites: []
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate 'data:text/html,<meta http-equiv="Content-Security-Policy" content="script-src '"'"'none'"'"'">'
  ff-rdp eval "1+1" --jq .meta.eval_path
  # expected: "chrome" (currently: "page-await" — the bug this iteration fixes)
tags: [iteration, testing, ci, eval, csp, navigate, dns, review-2026-07]
---

# Iteration 106: live-test masking cascade

While reviewing and merging [[iteration-100-daemon-lifecycle-hardening]]'s
PR, a single bug (`tabs` was used as the "trigger daemon auto-start" call in
several live tests, but `tabs.rs` connects to Firefox directly via
`RdpConnection::connect` and never goes through `resolve_connection_target`
— it has never actually started a daemon) turned out to be masking a whole
cascade of **unrelated, pre-existing** live-test failures. `cargo test`
stops the entire invocation on the first failing test *binary* (no
`--no-fail-fast` in `cargo test-live`), and `live_100_daemon_lifecycle_hardening`
+ `eval_object_leak_soak` sort alphabetically before most other `live_*`
binaries — so every test in a binary that sorts later never actually ran in
CI, for as long as this bug existed (since iter-61t, when the first
tabs-vs-eval test was written).

Fixing the root bug (iter-100 PR review, commits `f140dee`, `cd0ef30`) let
CI progress further and reveals genuine gaps this iteration must close:

## Themes

- **A — Chrome CSP eval bypass regressed.** `live_eval_chrome_csp_bypass`
  (iter-61x Theme A) asserts `meta.eval_path == "chrome"` when a page's CSP
  blocks `eval()`; it comes back `"page-await"` instead — the parent-process
  bypass is not triggering. This is either a real regression since iter-61x
  or a test that never actually passed once written (also masked by the same
  bug, transitively — worth checking git blame/history to distinguish "used
  to work" from "wrote it broken").
- **B — DNS-failure error shape.** `live_navigate_dnsfail` expects a
  neterror-shaped error (`dns_not_found` / `neterror` / `DNS`) but gets a
  generic `readyState did not reach 'complete'` timeout after ~6s instead.
  Root cause could be `navigate.rs`'s error classification not
  distinguishing DNS failures from generic load timeouts, or CI-runner DNS
  resolver behavior for `.invalid` TLDs — needs live investigation to tell
  apart.
- **C — Audit the rest of the masked test surface.** Both A and B were found
  by hitting `live_61l.rs`; alphabetically-later binaries
  (`live_86_perf_field_fixes` and `live_daemon_watch_targets` were *also*
  masked and already fixed/triaged in iter-100's PR — see its plan's
  addendum) may hide more. Run the *entire* `cargo test-live` suite locally
  with `--no-fail-fast` at least once and triage every result before this
  iteration closes, not just the two failures found so far.

## Tasks

### A. Chrome CSP eval bypass [0/2]
- [ ] Root-cause why `meta.eval_path` is `"page-await"` instead of
      `"chrome"` for a CSP `script-src 'none'` page — trace the eval
      command's CSP-detection / chrome-context routing logic
      (`crates/ff-rdp-cli/src/commands/eval.rs`) against the current Firefox
      version in CI. Check `git log -p` on the relevant branch since
      iter-61x for anything that changed the detection heuristic.
- [ ] Land the fix; remove the `FF_RDP_ALLOW_KNOWN_FAILING_CHROME_CSP`
      early-return gate added in iter-100's PR review
      (`crates/ff-rdp-cli/tests/live_61l.rs`) once
      `live_eval_chrome_csp_bypass` passes unconditionally again.

### B. DNS-failure error shape [0/2]
- [ ] Root-cause the ~6s generic timeout instead of a neterror-shaped
      message in `navigate.rs` for a DNS-resolution failure — determine
      whether Firefox's navigation error event isn't being classified
      correctly, or whether the CI runner's DNS resolver doesn't fail fast
      for `.invalid` domains (in which case the fix may be picking a
      resolution-guaranteed-to-fail domain/approach instead of relying on
      DNS behavior).
- [ ] Land the fix; remove the `FF_RDP_ALLOW_KNOWN_FAILING_DNSFAIL`
      early-return gate added in iter-100's PR review
      (`crates/ff-rdp-cli/tests/live_61l.rs`) once `live_navigate_dnsfail`
      passes unconditionally again.

### C. Full masked-surface audit [0/1]
- [ ] Run `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p
      ff-rdp-cli --no-fail-fast -- --ignored --test-threads=1` to completion
      (all ~55 live test binaries) at least once, and for every failure
      found: fix it if in-scope, or add a
      `FF_RDP_ALLOW_KNOWN_FAILING_<NAME>`-gated skip with a doc comment
      pointing at a follow-up plan (the pattern established in iter-100's PR
      review), so the masked-surface debt is fully inventoried rather than
      discovered one CI round-trip at a time.

## Acceptance Criteria [0/3]

- [ ] live_eval_chrome_csp_bypass: `meta.eval_path == "chrome"` for a CSP
      `script-src 'none'` page, unconditionally (no
      `FF_RDP_ALLOW_KNOWN_FAILING_CHROME_CSP` gate needed).
- [ ] live_navigate_dnsfail: exits non-zero with a neterror-shaped message
      for a DNS-resolution failure, unconditionally (no
      `FF_RDP_ALLOW_KNOWN_FAILING_DNSFAIL` gate needed).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
      FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p
      ff-rdp-cli --no-fail-fast -- --ignored --test-threads=1` clean (every
      previously-masked live test either passes or carries a
      `FF_RDP_ALLOW_KNOWN_FAILING_<NAME>` gate with its own tracked
      follow-up).

## Design notes

- This iteration exists because `cargo test-live`'s CI job has no
  `--no-fail-fast`, so a single early-alphabetical binary failure silently
  hides everything after it. Consider whether `live.yml` should add
  `--no-fail-fast` permanently once this iteration's fixes land, so future
  regressions in late-alphabetical binaries are visible immediately instead
  of being masked by whatever fails first.
- The `FF_RDP_ALLOW_KNOWN_FAILING_<NAME>` env-var-gate pattern (added in
  iter-100's PR review for these two tests plus
  `live_daemon_watch_targets`) is a deliberate stopgap: it keeps a test's
  assertion logic live and runnable on demand (`set the var, see the actual
  failure`) without either deleting real coverage or leaving the required
  `live-tests` CI check red for issues outside the landing PR's scope. Once
  a gated test's underlying bug is fixed, delete the gate — don't leave it
  as permanent decoration.

## Out of scope

- `live_daemon_watch_targets` (watchTargets re-engagement) —
  [[iteration-101-daemon-session-correctness]] Theme A already owns this.

## References

- [[iteration-100-daemon-lifecycle-hardening]] — where the root masking bug
  (`tabs` vs `eval` for daemon auto-start) was found and fixed; see its plan
  file for the full PR-review addendum.
- `crates/ff-rdp-cli/tests/live_61l.rs` — both gated tests live here.
