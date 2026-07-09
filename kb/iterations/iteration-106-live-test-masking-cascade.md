---
title: "Iteration 106: live-test masking cascade — chrome CSP bypass regression, DNS-failure error shape, cross-invocation daemon buffer visibility"
type: iteration
date: 2026-07-09
status: completed
branch: iter-106/live-test-masking-cascade
depends_on:
  - iteration-100-daemon-lifecycle-hardening
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-100-daemon-lifecycle-hardening.md
first_call_sites:
  - primitive: ResourceBuffer::record_boundary_and_insert (atomic nav-boundary + event inserts)
    site: crates/ff-rdp-cli/src/daemon/server.rs
  - primitive: reclassify_timeout_as_neterror (map navigate commit-wait timeout to a neterror)
    site: crates/ff-rdp-cli/src/commands/navigate.rs
dogfood_script: iteration-106-live-test-masking-cascade.dogfood.sh
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate --allow-unsafe-urls 'data:text/html,<meta http-equiv="Content-Security-Policy" content="script-src '"'"'none'"'"'">'
  ff-rdp eval "1+1" --jq .results
  # expected: 2 — eval succeeds despite the page CSP via Debugger.evalInGlobal
  # (meta.eval_path is "page-await"; iter-93 removed the chrome bypass — DEC-020)
  ff-rdp navigate 'https://this-domain-does-not-exist-106.invalid'; echo "exit=$?"
  # expected: exit=7, error_type "nav_dns_fail" (Theme B)
  ff-rdp navigate 'https://example.com/' --with-network >/dev/null
  ff-rdp network --detail --jq '.results[0] | {source,status,transfer_size}'
  # expected: {"source":"watcher","status":200,"transfer_size":<n>} in a SECOND
  # invocation reading the daemon buffer (Theme D)
tags:
  - iteration
  - testing
  - ci
  - eval
  - csp
  - navigate
  - dns
  - review-2026-07
---

# Iteration 106: live-test masking cascade

## Execution policies (2026-07-09, per James)

**Live tests:** do NOT run the full live Firefox suite during this iteration.
Run only the specific live tests this iteration's themes/ACs actually touch
(filtered, e.g. `cargo test -p ff-rdp-cli --test live <filter> --
--include-ignored`) plus the dogfood script. Full-suite validation happens
exactly once, in [[iteration-110-post-batch-live-sweep]], after iteration 109.

**Scoped testing — don't run everything N times:** while developing, run only
the tests affected by the change (`cargo test -p <crate> <filter>`). Run the
full `cargo test --workspace -q` exactly ONCE, as part of the final pre-PR
quality gates. The review agent must NOT re-run the full workspace suite
(implement's gate run + CI cover it); after review fixes, re-run only the
tests covering the files those fixes touched, then rely on CI.

**CI-wait:** merge once the required lanes pass (fmt, clippy, discipline,
supply-chain, fuzz, ubuntu/macos tests, verify-attestation). Do not block on
`live-tests` (advisory by design) or `test (windows-latest)` (known-red,
tracked in [[iteration-108-windows-ci-preexisting-reds]]) — but if windows
shows failures OTHER than the known 5, that IS a regression: stop and fix.


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
- **D — Cross-invocation daemon-buffer visibility.** `live_network_default_watcher`
  and `live_network_detail_headers` (`live_61q_resource_bus.rs`) navigate
  with `--with-network` in one CLI invocation, then query `ff-rdp network`
  in a second, separate invocation against the same (now genuinely-started)
  daemon — and get zero entries back. This reproduced identically on both a
  local macOS sandbox and CI's `ubuntu-latest` live-tests runner, so it is
  not environment flakiness. Likely related to the daemon RPC-writer
  replacement / buffer-visibility gap [[iteration-101-daemon-session-correctness]]
  Theme B already targets, but confirm whether it is the *same* root cause
  or a distinct one before assuming Theme B's fix covers it.

Found during the same review but explicitly **not** included above because
they are unconfirmed in real CI (only reproduced in a heavily-loaded local
sandbox against a stray local server) and self-documented as incomplete:
`live_index_local_fixture` / `live_runner_page_map_resolution`
(`live_62_page_map_index.rs`) both target `http://localhost:18080`, and the
file's own doc comment says "the fixture site is not yet committed... skip
if unreachable." If these ever show up as a genuine CI failure (not just a
local port collision), re-triage them then — don't assume Theme A-D covers
them.

## Tasks

### A. Chrome CSP eval bypass [2/2]
- [x] Root-caused (`eval.rs`): `commit 4b18939` (iter-93) **deliberately
      removed** the chrome-context bypass — `evaluateJSAsync` routes through
      `Debugger.evalInGlobal` (eval-with-debugger.js:119-247), which bypasses
      page CSP at the Debugger-API level, so the extra `getProcess(0)` parent-
      process hop was dead weight and was dropped. `eval_path` is now
      hard-set to `"page-await"` (`eval.rs:274-276`); the `"chrome"` value no
      longer exists. Verified live: `eval "1+1"` on a `script-src 'none'`
      page returns `2` with `meta.eval_path == "page-await"`.
- [x] `live_eval_chrome_csp_bypass` rewritten to assert the still-load-bearing
      guarantee — eval succeeds and returns `2` via the CSP-safe page-await
      path (`meta.eval_path == "page-await"`) — and the
      `FF_RDP_ALLOW_KNOWN_FAILING_CHROME_CSP` gate is removed. Passes live.

### B. DNS-failure error shape [2/2]
- [x] Root-caused (`navigate.rs`): on a DNS failure Firefox loads
      `about:neterror?e=dnsNotFound&…`, which never reaches the awaited
      `dom-complete` / `readyState === 'complete'` state — so the plain
      `navigate` wait (`run_core`) exhausted its budget and returned a generic
      `AppError::Timeout`. `run_with_network` already checked `listTabs` for a
      neterror landing via `check_real_tab_url_for_neterror`; `run_core` did
      **not**. (Verified live: the tab lands on `about:neterror?e=dnsNotFound`
      but the CLI surfaced a 124 timeout.) Not a `.invalid`-resolver issue.
- [x] Added `reclassify_timeout_as_neterror` in `run_core`: on an
      `AppError::Timeout`, query `listTabs` for an `about:neterror` landing and
      return the classified `AppError::Navigation { DnsFail }`
      ("DNS resolution failed", `error_type: "nav_dns_fail"`, exit 7). Removed
      the `FF_RDP_ALLOW_KNOWN_FAILING_DNSFAIL` gate. Verified live: exit 7,
      message contains "DNS".

### C. Full masked-surface audit [1/1]
- [x] Ran `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p
      ff-rdp-cli --test live --no-fail-fast -- --ignored --test-threads=1` to
      completion once (clean sequential run, no concurrent cargo): **68 passed,
      31 failed** of 99 ignored live tests. Triage:
      - **Fixed in iter-106's scope:** the four themed gated tests
        (`CHROME_CSP`/`DNSFAIL`/`NETWORK_WATCHER` × 2) now pass; plus
        `live_cookie_longstring_value` (`live_102_longstring_and_reload.rs`) — a
        **real masked bug**: the test set a 20 000-char cookie, but Firefox
        rejects cookies whose `name=value` exceeds 4096 bytes (RFC 6265 §6.1) so
        `document.cookie` stayed empty and `cookies` returned `[]`. Introduced a
        cookie-sized `COOKIE_LEN` (4 000) under the limit; the test now asserts
        the full value round-trips. Passes.
      - **Inventoried and handed to [[iteration-110-post-batch-live-sweep]]:**
        the remaining **29** failures are *pre-existing* masked failures
        unrelated to iter-106's themes (they never ran in CI, masked since
        iter-61t's tabs-vs-eval bug). Rather than bolt 29
        `FF_RDP_ALLOW_KNOWN_FAILING_*` gates onto a focused eval/DNS/network PR,
        the complete pass/fail inventory + root-cause categories are recorded in
        iter-110's Results section (the plan's designated full-suite-fallout
        sweep — see its Theme B, which already cross-references iter-106). This
        satisfies Theme C's goal: the masked-surface debt is now **fully
        inventoried rather than discovered one CI round-trip at a time.**
        Dominant category: the `data:`-URL security gate (landed iter-63) — many
        fixtures navigate to `data:` URLs without `--allow-unsafe-urls`; the
        symptom is `navigate failed` with empty stderr (the
        `URL scheme 'data:' is not allowed by default` error is emitted as JSON
        on stdout). Others are stale test-assertion shapes (same class iter-106
        fixed for the network tests) and real-site network/timing flakiness.
      - **Triage-method note (important):** `--test-threads=1` is mandatory and
        NO other `cargo test` (or the `dogfood` script) may run on the same
        machine during the sweep — the live tests share one Firefox/daemon per
        machine, so a concurrent run steals that state and produces *spurious*
        failures (observed `live_emulate_offline` / `live_manifest_fetch_canonical`
        "failing" only during an overlapping run; both pass in isolation and in
        the clean sweep). Category-(a) `data:`-URL failures were confirmed to
        reproduce in isolation on fresh Firefox, so they are real debt, not load
        artifacts.
      - Re-triaged `live_index_local_fixture` / `live_runner_page_map_resolution`
        (`live_62_page_map_index.rs`): they self-skip when
        `http://localhost:18080` is unreachable (fixture site not committed), so
        they are not real failures — unchanged, as Theme D's design note directs.

### D. Cross-invocation daemon-buffer visibility [2/2]
- [x] Root-caused — **distinct** from iter-101 Theme B (RPC-writer
      cross-delivery), as the plan predicted. Two compounding bugs:
      (1) **Boundary-ordering race** — `navigate --with-network` streams
      events, then sends `store-events`; the daemon's reader loop records the
      matching `tabNavigated` nav boundary on a *different thread*, which could
      land *after* the inserts and scope `--since -1` (the default) past every
      stored event → empty results. (2) **Lossy serializer round-trip** —
      `serialize_network_resources_for_buffer` wrote updates as
      `{"resourceUpdates": [{"resourceId": …}]}`, but
      `parse_network_resource_updates` reads a *top-level* `resourceId` and an
      *object*-valued `resourceUpdates`, so on the second-invocation drain every
      update was dropped, leaving `status`/`transfer_size` null. (Verified live:
      buffer held 4 network-event items but a separate `network` invocation
      surfaced 1 entry with null status.)
- [x] Landed both fixes: (1) `store-events` now takes `navUrl` and records the
      boundary **atomically** with the inserts via
      `ResourceBuffer::record_boundary_and_insert`; (2) the serializer emits the
      real wire shape (top-level `resourceId` + object `resourceUpdates`).
      Removed the `FF_RDP_ALLOW_KNOWN_FAILING_NETWORK_WATCHER` gate on both
      tests. Also fixed the masked sibling
      `live_network_watcher_source_after_navigate_with_network` (Theme C) and
      corrected the three network tests' stale output-shape assumptions
      (default `network` is a summary object since iter-49; per-entry field
      assertions need `--detail`; headers live under `headers.response`).

## Acceptance Criteria [4/4]

- [x] live_eval_chrome_csp_bypass: `eval "1+1"` returns `2` on a CSP
      `script-src 'none'` page via the CSP-safe page-await path
      (`meta.eval_path == "page-await"`), unconditionally (no
      `FF_RDP_ALLOW_KNOWN_FAILING_CHROME_CSP` gate). *AC reworded from the
      obsolete `== "chrome"` — see [[decision-log#DEC-020]]; iter-93 removed the
      chrome bypass because Debugger.evalInGlobal already bypasses page CSP.*
      Passed live.
- [x] live_navigate_dnsfail: exits non-zero (exit 7, `error_type:
      "nav_dns_fail"`, message "DNS resolution failed") for a DNS-resolution
      failure, unconditionally (no `FF_RDP_ALLOW_KNOWN_FAILING_DNSFAIL` gate).
      Backed by `reclassify_timeout_as_neterror` in `run_core`. Passed live.
- [x] live_network_default_watcher + live_network_detail_headers: a second CLI
      invocation's `network` query sees entries (with populated
      status/transfer_size and, for `--detail --headers`, `headers.response`)
      populated by a prior `navigate --with-network` invocation,
      unconditionally (no `FF_RDP_ALLOW_KNOWN_FAILING_NETWORK_WATCHER` gate).
      Backed by `ResourceBuffer::record_boundary_and_insert` + the fixed
      `serialize_network_resources_for_buffer` wire shape. Both passed live.
- [x] live_cookie_longstring_value: the masked-surface audit's one in-scope
      real bug — `COOKIE_LEN` (4 000) replaces the rejected 20 000-char cookie so
      the full value round-trips. fmt + clippy green; the full `--no-fail-fast`
      audit ran to completion once (Theme C). Every test in iter-106's scope
      passes: the three themed gates (`CHROME_CSP`/`DNSFAIL`/`NETWORK_WATCHER`)
      are deleted and pass unconditionally, backed by `record_boundary_and_insert`
      and `reclassify_timeout_as_neterror`. The 29 remaining pre-existing masked
      failures (unrelated to this iteration's eval/DNS/network themes — dominated
      by the iter-63 `data:`-URL gate) are **fully inventoried** in
      [[iteration-110-post-batch-live-sweep]]'s Results and handed to that plan's
      Theme B (the designated full-suite-fallout sweep), per this plan's
      execution policy ("Full-suite validation happens exactly once, in
      iteration-110"). [deferred — new plan: kb/iterations/iteration-110-post-batch-live-sweep.md]
      The only `FF_RDP_ALLOW_KNOWN_FAILING_*` gate left in the tree is
      `WATCH_TARGETS`, out of scope here (owned by
      [[iteration-101-daemon-session-correctness]] Theme A).

## Design notes

- This iteration exists because `cargo test-live`'s CI job had no
  `--no-fail-fast`, so a single early-alphabetical binary failure silently
  hid everything after it. **DONE:** `live.yml`'s live-tests step now runs
  `cargo test --workspace --no-fail-fast -- --include-ignored --test-threads=1`
  directly (rather than the `cargo test-live` alias, which cannot inject a
  pre-`--` cargo flag), so future regressions in late-alphabetical binaries are
  visible immediately. `--test-threads=1` is included because the live tests
  share one Firefox/daemon per runner — the same reason the local Theme C sweep
  must not run concurrently with any other `cargo test`.
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
