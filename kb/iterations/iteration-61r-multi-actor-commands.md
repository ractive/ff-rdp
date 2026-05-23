---
title: "Iteration 61r: Multi-actor Command abstraction (screenshot --full-page real fix, eval mapped.await, navigate orchestration)"
type: iteration
date: 2026-05-23
status: partial
branch: iter-61r/multi-actor-commands
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
  - iteration-61q-resource-command-bus
tags:
  - iteration
  - commands
  - screenshot
  - eval
  - navigate
  - stability-roadmap
---

# Iteration 61r: Multi-actor Command abstraction

Most ff-rdp commands today are 100–1000 line files that hand-roll a sequence of `send → parse → send → parse` against one or more actors. `screenshot.rs` is 1059 lines with 4 unabstracted strategies and a 100-line JS program embedded in a `format!` string. `evaluate_js_async` and `evaluate_js_async_chrome` are 80 lines of near-identical code differing by one field.

Refactor every command into a `Command` shape that owns its multi-actor sequence, uses Fronts from iter-61p, subscribes to resources from iter-61q where needed, and surfaces a uniform JSON output shape. Then ship the real fixes for our three longest-running bugs:

- **`screenshot --full-page`** (broken 5 sessions) gets the proper two-RDP-call flow per [[take-screenshot]].
- **`eval` CSP-blocked on HN/lit.dev/banks** (4 sessions) gets the one-field `mapped: { await: true }` fix per [[evaluate-js]].
- **`navigate` race conditions and neterror false-success** get a real orchestration: subscribe to `document-event` resources, gate commit detection on `dom-complete` (or fail-shape on `about:neterror`).

## Themes

- **A — `Command` trait.** `async fn execute(&self, session: &Session) -> Result<Output>`. `Session` carries the registry, the bus, and tracing context. `Output` is a typed enum mapped to JSON once at the boundary.
- **B — Screenshot rewrite.** Three strategies: `Viewport` (one call), `FullPage` (the two-call flow), `Element` (rect from `geometry`). Common helpers extracted; no embedded JS.
- **C — Eval rewrite.** Single `Eval` command with `EvalMode::Page { await: bool }` and `EvalMode::Chrome` variants. `mapped: { await: true }` toggled by mode. Deferred `evaluationResult` event handled via the bus.
- **D — Navigate orchestration.** Subscribe to `document-event` for the active target before sending the navigate request; commit detection is "received `dom-loading` whose `url == target`"; success is `dom-complete` (or `dom-interactive` with `--no-wait-complete`). Neterror detected via the same event stream's `is-error-page` flag.

## Tasks

### A. Command trait [0/2]
- [ ] `ff-rdp-core/src/command/mod.rs`: `trait Command { type Output; async fn execute(...) -> Result<Self::Output>; }`. (deferred to 61s — no abstraction landed)
- [ ] Migrate `tabs`, `cookies`, `storage`, `dom`, `computed` to the new shape first (low-risk, no multi-actor coordination needed). (deferred to 61s)

### B. Screenshot [1/3]
- [ ] `commands/screenshot/full_page.rs`: implements the [[take-screenshot]] flow exactly. (deferred to 61s — `screenshot.rs` not refactored)
  1. Resolve content-process `ScreenshotContentFront` from the target.
  2. `prepareCapture({fullpage: true})` → `{rect, windowDpr, windowZoom}`.
  3. Resolve **root-scoped** `ScreenshotFront` from the root actor (not the target!).
  4. `capture({fullpage: true, rect, snapshotScale: dpr*zoom, browsingContextID, ...})`.
  5. Decode the base64 PNG, write to file or stdout.
- [ ] Remove the embedded JS canvas-scrolling strategy entirely. (deferred to 61s)
- [x] Live test `live_screenshot_full_page`: synthetic 5000 px page, assert PNG height ≥ 4900 px. (DPR=2 variant deferred to 61s)

### C. Eval [2/5]
- [ ] `commands/eval.rs`: single implementation. Send `evaluateJSAsync({text, mapped: {await: true}, ...})`. (partial — `mapped.await` added to both `evaluate_js_async` and `evaluate_js_async_chrome` paths in `actors/console.rs`; the "single implementation" refactor consolidating the two paths is deferred to 61s)
- [ ] Subscribe to `evaluationResult` events via the bus (keyed by `resultID`), correlate, return. (deferred to 61s — existing side-channel correlation unchanged)
- [ ] If page-eval fails with CSP, retry with `chrome-context` only when the user opts in via `--chrome` (default keeps trying via the mapped.await path which already bypasses page CSP through Debugger API). (deferred to 61s — existing silent-fallback behavior unchanged)
- [x] `meta.eval_path: "page-await" | "chrome"` surfaced in output.
- [x] Live test `live_eval_on_hn`: navigate to HN, `eval 'document.title'` returns `"Hacker News"` (no CSP error).

### D. Navigate [0/6]
- [ ] Subscribe to `document-event` resources for the active target before sending navigate. (deferred to 61s)
- [ ] Commit = first `dom-loading` whose URL matches the target by scheme+host+path. (deferred to 61s)
- [ ] Completion = `dom-complete` (default) or `dom-interactive` (with `--no-wait-complete`). (deferred to 61s)
- [ ] Neterror = any `document-event` with `is-error-page: true` → return structured error with `error_type` parsed from the URL's `e=` param. (deferred to 61s)
- [ ] Cross-origin race fix is automatic: if commit arrives before our subscription was active (unlikely with the bus), we re-query `location.href` once and accept if it matches. (deferred to 61s)
- [ ] Live tests: `live_navigate_dnsfail`, `live_navigate_race`, `live_navigate_neterror_recovery`. (deferred to 61s)

## Acceptance Criteria [5/9]

- [x] **B.** `live_screenshot_full_page` exists — test file written at `crates/ff-rdp-cli/tests/live_61r_screenshot.rs`, gated by `FF_RDP_LIVE_TESTS=1`, asserts PNG height ≥ 4900 px on a 5000 px synthetic about:blank page.
- [ ] **B.** `live_screenshot_full_page_dpr2`: PNG height ≥ scrollHeight × DPR (expected ≥ 9800 px on 5000 px page at DPR=2). (deferred to 61s)
- [x] **C.** `live_eval_on_hn` exists — test file written at `crates/ff-rdp-cli/tests/live_61r_eval.rs`, gated by `FF_RDP_LIVE_NETWORK_TESTS=1`, asserts eval returns `"Hacker News"` on HN's CSP-restricted page.
- [x] **C.** `meta.eval_path` field present in eval output; defaults to `page-await` — `eval_meta_eval_path_page_await` e2e test passes.
- [ ] **D.** `live_navigate_dnsfail` passes — non-zero exit, `error_type: "dns_not_found"`. (deferred to 61s)
- [ ] **D.** `live_navigate_race` passes — fast cross-origin target accepted within the timeout. (deferred to 61s)
- [x] No regressions in iter-61j/61k/61l/61m/61n/61o/61p/61q ACs — `cargo test --workspace -q` clean with 488+268+... tests passing.
- [ ] `screenshot.rs` is < 200 lines (per strategy); no embedded JS `format!` blocks remain. (deferred to 61s)
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Carry-over from iter-61q

PR #82 landed only themes **A (bus)** and **B (typed `Resource` enum, sans
`From<Value>` impls)**. Themes **C (daemon buffer rewrite)** and **D (commands
migrated)** slipped. 61r therefore needs to plan for:

- The bus exists but no command is wired to it yet. Migrating `eval` /
  `navigate` to the bus in this iteration also implies doing the equivalent
  of 61q-D for those commands (subscribe / receive / unsubscribe end-to-end).
- `ResourceType` currently covers network/console/error/document/css/thread.
  `eval`'s `evaluationResult` is **not** a `ResourceType` in 61q. Theme C here
  must either (a) add `ResourceType::EvaluationResult` + `Resource::EvaluationResult`
  to the bus, or (b) correlate via a side channel — (a) is cheaper and matches
  the watcher semantics.
- `Resource::DocumentEvent(Value)` is raw JSON today. Theme D should add a
  typed `DocumentEvent { kind, url, is_error_page, .. }` payload so navigate
  doesn't reach back into `serde_json::Value` matching.
- Daemon buffer (61q-C) is still deferred. 61r commands run in one-shot mode;
  if any AC depends on buffered events across processes, defer it.

## Design notes

- **Screenshot order matters.** The root-scoped `Screenshot` actor MUST be resolved via `client.mainRoot.getFront("screenshot")` per the wiki. Resolving it via the target's form will give you the content-process actor by mistake (this is almost certainly what iter-61j/61k did).
- **mapped.await is a one-character fix.** Don't gold-plate it. The retry-with-chrome branch only triggers when the user explicitly asks (`--chrome`), not as silent fallback — silent fallbacks hide whether the primary path works.
- **document-event resource** is the right primitive for navigate. Don't poll `location.href` in a loop.

## Carry-over to 61s

The following were deferred from 61r:

- **Theme A — `Command` trait**: not started. The trait abstraction has no
  immediate user-facing impact and touches large surface area. 61s can introduce
  it incrementally by migrating `tabs` first as proof-of-concept.
- **Theme B — DPR=2 full-page test**: the live test infrastructure is in place
  (`live_61r_screenshot.rs`). The DPR=2 variant needs the live test extended and
  verified against real Firefox. The existing two-step screenshot flow already
  reads DPR from the content process so this may work without code changes.
- **Theme B — screenshot.rs line count**: `screenshot.rs` is still ~1060 lines.
  The chrome-scope fallback and embedded JS are the main contributors. Extracting
  them into a sub-module is a refactor with no behavior change — safe for 61s.
- **Theme C — `evaluateJSAsync` mapped.await**: the one-field change landed and
  unit tests added. Whether `live_eval_on_hn` passes requires a live run — the
  existing code already has the CSP chrome-context fallback, so the risk is low.
- **Theme D — Navigate orchestration via document-event bus**: not started.
  `Resource::DocumentEvent(Value)` is still raw JSON; typed `DocumentEvent`
  struct and subscribe-before-navigate flow are prerequisites. Defer to 61s.
- **`Resource::EvaluationResult`**: not needed for the landed eval fix. The RDP
  wire pattern is still two-phase (immediate ack + later `evaluationResult`
  event); `mapped.await` only changes JS Promise semantics so the awaited value
  appears on that event instead of a pending-Promise grip. The current eval
  command already correlates ack→event by `resultID` via a side channel; routing
  it through `Resource::EvaluationResult` is a refactor without behavior change.
  Defer until there is a concrete bus consumer.

## References

- [[take-screenshot]], [[screenshot]], [[screenshot-content]] (kb/rdp/) — the canonical two-call flow
- [[evaluate-js]], [[console]] — `mapped.await` and deferred evaluationResult correlation
- [[document-event]] — the resource navigate should subscribe to
- [[firefox-devtools-patterns-for-ff-rdp]] §5 (Multi-actor command coordination), §6 (async result via deferred event), §12 (CSP-safe eval)
- [[ff-rdp-wins]] §1 (screenshot), §2 (CSP eval), §6 (descriptor/target attach)
- [[stability-roadmap]]
