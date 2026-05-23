---
title: "Iteration 61x: Honest commits — close the iter-61t..61v claim/code gap + spec/perf cleanup"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61x/honest-commits-and-cleanup
depends_on:
  - iteration-61t-wire-the-foundations
  - iteration-61u-spec-and-front-correctness
  - iteration-61v-navigate-and-screenshot-completion
tags: [iteration, eval, navigate, screenshot, specs, cleanup, stability-roadmap]
---

# Iteration 61x: Honest commits — close the iter-61t..61v claim/code gap

The post-61v cross-cutting review found three commit-message claims from iter-61u/v that are demonstrably false in the merged code, plus a handful of small spec/perf cleanups that the previous iterations left behind. Each item here closes one such claim or hangover. The iteration is named "honest commits" because the diff of this PR will look like correcting prior PR descriptions — not adding new features.

This is the smaller sibling of [[iter-61y-iteration-discipline-tooling]], which introduces the structural pressure to prevent the pattern from recurring.

## Themes

- **A — Chrome-context eval actually uses the parent-process descriptor.** iter-61u's spec-layer comment claims `chromeContext` was removed; the actor and CLI still send/branch on it. Implement `DescriptorFront::getProcess(0)` and route chrome-context eval through it, as iter-61u promised.
- **B — `RdpError::Navigation{cause}` actually exists.** iter-61v's PR description says it added a typed `Navigation` error variant with `DnsFail/CertError/ConnReset/Timeout`. The enum is not in `error.rs`. Add it and return it from the neterror branches.
- **C — `dom-interactive` is observed.** iter-61v claimed gating on `dom-loading | dom-interactive | dom-complete`; only the first and last are matched. Add the third arm and honor `--wait interactive`.
- **D — DPR=2 live screenshot test (3rd attempt).** Deferred from 61r and 61v. Land it.
- **E — Flesh out the skeleton live tests carried over from iter-61u.** `live_network_set_cookie_longstring` and `live_cache_disable_via_target_config` currently only verify protocol round-trips; add the actual assertions about returned value content and cache bypass.
- **F — Delete dead navigate polling helper.** `wait_for_commit` and the `JSON.stringify({ready,url})` polling stub are unreachable but compile. Remove.
- **G — Screenshot spec types match the FF dict.** `snapshot_scale: f64` → `Option<f64>`; doc-comment the three non-spec fields that the server reads directly.
- **H — `Arc<Resource>` in bus fan-out.** Currently each subscriber gets a clone. With >4 subscribers under a 1000-event burst that's measurable. Replace with `Arc<Resource>`.
- **I — Close the iter-61w test-coverage gap.** iter-61w landed the security hardening code (constant-time token compare, refstore cap, nav-URL truncation, terminal-escape sanitizer, poisoned-mutex recovery) but only shipped 5 of 12 promised tests. Add the remaining 5 so the ACs are honest, matching the pattern this iteration enforces elsewhere.

## Tasks

### A. Chrome-context via getProcess
- [ ] Add `getProcess(id: u32)` method marker to `crates/ff-rdp-core/src/specs/descriptor.rs`. FF wire name: `getProcess`, request `{ id: number }`, returns a `processDescriptor` actor form.
- [ ] Add `crates/ff-rdp-core/src/fronts/descriptor.rs::DescriptorFront::get_process(0)` returning a `ProcessDescriptorFront`; that front's `get_target()` returns the parent-process target form with its own `consoleActor`.
- [ ] Update `commands/eval.rs` chrome-context branch: replace the `chromeContext: true` field with a separate request through the parent-process console actor.
- [ ] Delete the `chrome_context` field handling at `actors/console.rs:226, 657` and `commands/eval.rs:240, 333, 341`.
- [ ] Delete the spec-layer comment in `specs/console.rs:46` that lies about the field having been removed (it'll be honest once A above lands).
- [ ] Live test `live_eval_chrome_csp_bypass`: load a page with `Content-Security-Policy: script-src 'none'`; assert `ff-rdp eval --chrome-context "1+1"` returns 2; the same call without `--chrome-context` returns a CSP exception.

### B. Typed Navigation error
- [ ] Add to `crates/ff-rdp-core/src/error.rs`:
  ```rust
  #[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
  pub enum NavCause {
      DnsFail, CertError, ConnReset, Timeout,
      ContentBlocked, Unknown(String),
  }
  // and in RdpError:
  #[error("navigation failed: {cause:?}")]
  Navigation { cause: NavCause, url: String },
  ```
- [ ] Map this through `crates/ff-rdp-cli/src/error.rs` so the CLI exit code is deterministic (`Navigation::DnsFail` → 7, `CertError` → 8, etc.).
- [ ] Replace every `AppError::User(format!(...))` neterror return in `commands/navigate.rs:163-170, 462, 517` with the typed variant.
- [ ] Unit test on `classify_neterror` that each `e=` value maps to the correct `NavCause`.
- [ ] Live tests carried from iter-61v plan: `live_navigate_neterror_dns_fail`, `live_navigate_neterror_cert` — must now match on the typed cause via `--json` output, not on stderr substrings.

### C. dom-interactive arm
- [ ] `commands/navigate.rs:155-189`: add a `"dom-interactive"` match arm that records the interactive timestamp and short-circuits the wait if `--wait interactive` is in effect.
- [ ] Update the `--wait` CLI flag to accept `loading|interactive|complete`; default stays `complete`.
- [ ] Live test `live_navigate_wait_interactive`: navigate to a page with a deferred `<script>` that blocks `dom-complete`; assert `--wait interactive` returns within a reasonable time, while `--wait complete` blocks until the script finishes.

### D. DPR=2 live screenshot
- [ ] `tests/live_screenshot_full_page_dpr2.rs`: launch headless Firefox with `--remote-debugging-port` and a profile that pre-sets `layout.css.devPixelsPerPx = "2"`; navigate to a 5000px-tall test page (use an existing fixture or generate inline); run `screenshot --full-page --output /tmp/x.png`; assert the PNG has `width = viewport_css_px * 2` and `height >= 5000 * 2`.
- [ ] Remove the `// live_screenshot_full_page_dpr2 is not implemented` placeholder at `commands/screenshot.rs:427`.
- [ ] If this test reveals a real bug in the two-step `getRoot.getFront("screenshot")` path on DPR=2, fix it in scope — that was the original deferred work.

### E. Skeleton live tests fleshed out
- [ ] `crates/ff-rdp-core/tests/live_61u.rs::live_network_set_cookie_longstring`: extend beyond the round-trip — assert that the header value coming back is the full ≥10 000-char string content (via `LongString::fetch_full`), not just an actor ref or a truncated initial. Use a fixture page that sets a deterministic `Set-Cookie: aaaa...` with a length checksum in the last 8 chars.
- [ ] `crates/ff-rdp-core/tests/live_61u.rs::live_cache_disable_via_target_config`: navigate to `/etag-resource` with `Cache-Control: max-age=3600`; observe the first response status; call `TargetConfigurationFront::set_cache_disabled(true)`; navigate again; assert the second response is a fresh 200 (not 304, not from disk-cache), measured by the network-event `fromCache` field.
- [ ] Tick the two corresponding ACs in `kb/iterations/iteration-61u-spec-and-front-correctness.md` and update the AC header from `[6/8]` to `[8/8]`.

### F. Delete dead polling helper
- [ ] Delete `wait_for_commit` and the `JSON.stringify({ready,url})` JS program at `commands/navigate.rs:69-83, 291, 332-333, 390, 673`. Adjust any tests that still reference them.
- [ ] `grep -c 'wait_for_commit\|JSON.stringify({ready' crates/` should return 0.

### G. Screenshot spec hygiene
- [ ] `crates/ff-rdp-core/src/specs/screenshot.rs:42`: `snapshot_scale: f64` → `snapshot_scale: Option<f64>`. Treat absent as 1.0 server-side per `devtools/server/actors/utils/capture-screenshot.js`.
- [ ] Add a doc comment above `CaptureArgs` listing the three fields the FF spec dict does not declare but the server reads directly (`browsingContextID`, `rect`, `snapshotScale`), and link to `devtools/shared/specs/screenshot.js:13-20` for the formal dict.

### H. Arc<Resource> fan-out
- [ ] `crates/ff-rdp-core/src/resources/command.rs:227-232`: change the subscriber sink type from `mpsc::Sender<Resource>` to `mpsc::Sender<Arc<Resource>>`; dispatcher constructs `Arc::new(resource)` once and clones the `Arc` per subscriber.
- [ ] Update `daemon/buffer.rs::on_resource` and any other subscribers to take `&Arc<Resource>` or deref as needed.
- [ ] Bench: `bench_bus_fanout_4_subscribers` shows a measurable wall-clock improvement on a 1000-event burst with 4 subscribers.

### I. iter-61w test-coverage carry-over
- [ ] `test_token_comparison_constant_time` in `crates/ff-rdp-cli/src/daemon/server.rs`: 1000 iterations of token compare, median time of full-token vs first-byte-mismatch within 5%. Use `std::time::Instant`; allow a generous tolerance for CI jitter.
- [ ] `test_refstore_capped` in `crates/ff-rdp-cli/src/daemon/server.rs`: register `MAX_REFS + 100` entries in a tight loop; assert `refs.len() == MAX_REFS` and subsequent inserts in the *same* batch are dropped (regression-guards the per-insert cap from iter-61w post-review fix).
- [ ] `test_nav_boundary_url_truncated` in `crates/ff-rdp-cli/src/daemon/buffer.rs`: push a 1 MB URL containing non-ASCII chars; assert the stored value is `<= MAX_NAV_URL_LEN` bytes AND starts/ends on a UTF-8 char boundary (`std::str::from_utf8` round-trips).
- [ ] `test_terminal_escape_sanitized_e2e` (live or fixture-driven): eval throws an exception whose message contains `\x1b[2J`; capture stderr; assert the raw ESC byte does not appear and `?` does.
- [ ] `test_lock_or_recover_continues_on_poison` in `crates/ff-rdp-cli/src/daemon/server.rs`: inject a panic in a helper thread that holds a daemon mutex; assert the next `lock_or_recover!` call returns the inner value and `tracing` records one error event.
- [ ] Tick the five corresponding ACs in `kb/iterations/iteration-61w-security-hardening-and-cleanup.md` and update its AC header from `[7/12]` to `[12/12]`; flip status to `done` once 61w is merged AND these tests land.

## Acceptance Criteria [0/13]

- [ ] `grep -rn '"chromeContext"' crates/` returns 0; chrome-context eval round-trips through a different actor.
- [ ] `RdpError::Navigation{cause: NavCause, url: String}` is in `core::error`; `commands/navigate.rs` returns it.
- [ ] `--wait interactive` returns on `dom-interactive` without waiting for `dom-complete`.
- [ ] `live_screenshot_full_page_dpr2`: PNG is `2× scrollHeight` tall.
- [ ] `live_network_set_cookie_longstring`: ≥10 000-char `Set-Cookie` returned with full content and checksum-validated.
- [ ] `live_cache_disable_via_target_config`: second request bypasses cache after `set_cache_disabled(true)`.
- [ ] `wait_for_commit` is gone; `commands/navigate.rs` LOC drops by at least 40.
- [ ] `specs/screenshot.rs::CaptureArgs::snapshot_scale` is `Option<f64>`.
- [ ] `bench_bus_fanout_4_subscribers` p99 improved vs pre-change baseline.
- [ ] `live_eval_chrome_csp_bypass` passes; `--chrome-context` is genuinely privileged.
- [ ] iter-61u plan AC list shows `[8/8]` after the carry-over tests land.
- [ ] All five iter-61w carry-over tests (theme I) exist and pass; iter-61w plan shows `[12/12]` and `status: done`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- The descriptor path (theme A) is one extra round-trip on first chrome-context eval per session. Cache the `ProcessDescriptorFront` in the `Registry` so subsequent chrome evals don't pay it. iter-61t's `call_with_refresh` covers invalidation if the parent process restarts (it won't, in practice).
- Theme B's exit-code mapping is part of `RdpError`'s public contract; existing CLI callers that match on exit code will see new values. Document in `kb/rdp/from-our-codebase/cli-exit-codes.md` (create if needed).
- Theme G's `Option<f64>` change is a wire-shape adjustment but backwards-compatible since FF's server treats absent as 1.0.
- Theme H requires every subscriber to handle `Arc<Resource>` — touches `daemon/buffer.rs`, `commands/network.rs`, `commands/console.rs`. Mechanical change but spans files; do it as a single commit to keep the diff readable.

## References

- [[iter-61m-61s-postmortem-loose-ends]] §"How the pattern recurred in 61t..61v"
- [[iter-61y-iteration-discipline-tooling]] — the structural fix
- `devtools/shared/specs/descriptor.js` — getProcess shape
- `devtools/shared/specs/webconsole.js` — confirms chromeContext is NOT a field
- `devtools/server/actors/utils/capture-screenshot.js` — snapshotScale default
- [[stability-roadmap]]
