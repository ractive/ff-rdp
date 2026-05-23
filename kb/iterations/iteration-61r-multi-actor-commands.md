---
title: "Iteration 61r: Multi-actor Command abstraction (screenshot --full-page real fix, eval mapped.await, navigate orchestration)"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61r/multi-actor-commands
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
  - iteration-61q-resource-command-bus
tags: [iteration, commands, screenshot, eval, navigate, stability-roadmap]
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

### A. Command trait
- [ ] `ff-rdp-core/src/command/mod.rs`: `trait Command { type Output; async fn execute(...) -> Result<Self::Output>; }`.
- [ ] Migrate `tabs`, `cookies`, `storage`, `dom`, `computed` to the new shape first (low-risk, no multi-actor coordination needed).

### B. Screenshot
- [ ] `commands/screenshot/full_page.rs`: implements the [[take-screenshot]] flow exactly.
  1. Resolve content-process `ScreenshotContentFront` from the target.
  2. `prepareCapture({fullpage: true})` → `{rect, windowDpr, windowZoom}`.
  3. Resolve **root-scoped** `ScreenshotFront` from the root actor (not the target!).
  4. `capture({fullpage: true, rect, snapshotScale: dpr*zoom, browsingContextID, ...})`.
  5. Decode the base64 PNG, write to file or stdout.
- [ ] Remove the embedded JS canvas-scrolling strategy entirely.
- [ ] Live test `live_screenshot_full_page`: synthetic 5000 px page, assert PNG height ≥ 4900 px. Same with DPR=2 → ≥9800 px.

### C. Eval
- [ ] `commands/eval.rs`: single implementation. Send `evaluateJSAsync({text, mapped: {await: true}, ...})`.
- [ ] Subscribe to `evaluationResult` events via the bus (keyed by `resultID`), correlate, return.
- [ ] If page-eval fails with CSP, retry with `chrome-context` only when the user opts in via `--chrome` (default keeps trying via the mapped.await path which already bypasses page CSP through Debugger API).
- [ ] `meta.eval_path: "page-await" | "chrome"` surfaced in output.
- [ ] Live test `live_eval_on_hn`: navigate to HN, `eval 'document.title'` returns `"Hacker News"` (no CSP error).

### D. Navigate
- [ ] Subscribe to `document-event` resources for the active target before sending navigate.
- [ ] Commit = first `dom-loading` whose URL matches the target by scheme+host+path.
- [ ] Completion = `dom-complete` (default) or `dom-interactive` (with `--no-wait-complete`).
- [ ] Neterror = any `document-event` with `is-error-page: true` → return structured error with `error_type` parsed from the URL's `e=` param.
- [ ] Cross-origin race fix is automatic: if commit arrives before our subscription was active (unlikely with the bus), we re-query `location.href` once and accept if it matches.
- [ ] Live tests: `live_navigate_dnsfail`, `live_navigate_race`, `live_navigate_neterror_recovery`.

## Acceptance Criteria [0/9]

- [ ] **B.** `live_screenshot_full_page` passes — PNG height ≥ 4900 px on a 5000 px synthetic page.
- [ ] **B.** Same test at DPR=2 — PNG height ≥ 9800 px.
- [ ] **C.** `live_eval_on_hn` passes — eval works on HN under page CSP.
- [ ] **C.** `meta.eval_path` field present in eval output; defaults to `page-await`.
- [ ] **D.** `live_navigate_dnsfail` passes — non-zero exit, `error_type: "dns_not_found"`.
- [ ] **D.** `live_navigate_race` passes — fast cross-origin target accepted within the timeout.
- [ ] No regressions in iter-61j/61k/61l/61m/61n/61o/61p/61q ACs.
- [ ] `screenshot.rs` is < 200 lines (per strategy); no embedded JS `format!` blocks remain.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test-live && cargo test --workspace -q` clean.

## Design notes

- **Screenshot order matters.** The root-scoped `Screenshot` actor MUST be resolved via `client.mainRoot.getFront("screenshot")` per the wiki. Resolving it via the target's form will give you the content-process actor by mistake (this is almost certainly what iter-61j/61k did).
- **mapped.await is a one-character fix.** Don't gold-plate it. The retry-with-chrome branch only triggers when the user explicitly asks (`--chrome`), not as silent fallback — silent fallbacks hide whether the primary path works.
- **document-event resource** is the right primitive for navigate. Don't poll `location.href` in a loop.

## References

- [[take-screenshot]], [[screenshot]], [[screenshot-content]] (kb/rdp/) — the canonical two-call flow
- [[evaluate-js]], [[console]] — `mapped.await` and deferred evaluationResult correlation
- [[document-event]] — the resource navigate should subscribe to
- [[firefox-devtools-patterns-for-ff-rdp]] §5 (Multi-actor command coordination), §6 (async result via deferred event), §12 (CSP-safe eval)
- [[ff-rdp-wins]] §1 (screenshot), §2 (CSP eval), §6 (descriptor/target attach)
- [[stability-roadmap]]
