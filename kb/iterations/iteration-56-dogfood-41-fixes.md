---
title: "Iteration 56: Dogfooding Session 41 Fixes"
type: iteration
date: 2026-05-10
status: in-progress
branch: iter-56/dogfood-41-fixes
tags:
  - iteration
  - bugfix
  - dx
  - exit-codes
  - timeout
  - help-text
  - ergonomics
---

# Iteration 56: Dogfooding Session 41 Fixes

Bugfix pass driven by [[dogfooding/dogfooding-session-41]]. Most findings are small individually, but together they're the difference between an AI agent treating ff-rdp's `--help` and exit codes as ground truth or learning to second-guess them.

Two themes:

- **A — Trust the contract.** The CLI promises things in `--help` and via exit codes that the implementation doesn't deliver: documented EXIT CODES never fire, `responsive` schema doesn't match docs, `screenshot` fallback never engages despite iter-53 promising it would.
- **B — Sensible defaults.** `network` drain times out at 5000 ms on real-world pages, `geometry` floods output with hidden zero-sized matches, `eval --stringify` double-encodes strings. All "works but surprises a careful user".

## Tasks

## Status

- A1, A2, B1, B2, B3, B4 — landed in this iteration. Tests + quality gates clean.
- A3 (screenshot fallback) and C1 (`navigate --wait-text` `noSuchActor`) — **deferred**. Both require live recordings across Firefox versions to identify the exact actor failure shape. Tracked in [[backlog/future-features]] for a focused follow-up that can iterate against a Firefox 150 instance.

### A. Trust the contract

#### A1. Make EXIT CODES actually match the documented schema [3/3]

`ff-rdp --help` documents `0 ok / 1 runtime / 2 usage / 3 connection failure / 124 timeout`. Reality: every non-success returns `1` (except clap usage errors which correctly return `2`). Critical for AI agents that branch on exit codes.

- [x] Map the existing `AppError` variants to the documented exit codes in `crates/ff-rdp-cli/src/main.rs`. New surface: `AppError::Connection(...)` → 3, `AppError::Timeout(...)` → 124. Audit all `AppError::User(format!("... timed out ..."))` and `... connect ...` sites and route them to the new variants.
- [x] Cover the cases dogfooding hit: wait-timeout, operation timeout, daemon RPC drain timeout, "could not connect to Firefox", "could not connect to daemon".
- [x] Tests: e2e snippets that assert exit code per failure class. One test per documented code (`0`, `1`, `2`, `3`, `124`).

#### A2. Align `responsive` `--help` output schema with reality [2/2]

Help advertises `{"results": {"320": [...], "768": [...]}}`. Actual output is `{"results": {"breakpoints": [{"width": 320, "elements": [...]}, ...], "original_viewport": {...}}}`. The actual shape is the better one (preserves order, includes original viewport). Just fix the docs.

- [x] Update the `Responsive` `long_about` block in `crates/ff-rdp-cli/src/cli/args.rs` to describe the actual `breakpoints[]` shape with `width`, `elements[]`, `viewport`, plus the top-level `original_viewport`.
- [x] Audit the other subcommand schemas added in iter-55 C2 (`click`, `wait`, `storage`, `responsive`, `a11y`, `scroll`, `geometry`, `snapshot`, `styles`, `console`, `tabs`, `reload`, `back`, `forward`, `inspect`, `page-text`, `computed`, `sources`) for similar doc/code drift; fix any that don't match a fresh sample.

#### A3. Make `screenshot` fallback actually fire — or stop promising it [0/3] — deferred

iter-53 task 3 promised: when `screenshotActor.capture` fails with the known module-load error, fall back to a DOM-based capture (`canvas.drawWindow` / `html2canvas`-style). On Firefox 150 the error path produces a clean message but no fallback runs. Either the trigger condition doesn't match Firefox 150's actual error, or the fallback was never wired up.

Deferred: needs a live Firefox 150 recording to capture the exact actor error string. Tracked in [[backlog/future-features]].

- [ ] Reproduce on Firefox 150 with `RUST_LOG=debug` and capture the *exact* error string the actor returns. Compare against the trigger condition in the screenshot command implementation.
- [ ] Either widen the trigger condition to cover the FF 150 error shape, or implement a real fallback if one is missing. The fallback should use the existing `eval` machinery to render the page to a base64 PNG via `canvas.drawWindow` (chrome-only API; will fail on non-headless if the page is in a content sandbox — surface that distinction).
- [ ] Live e2e fixture against a Firefox version that triggers the actor failure; assert the fallback succeeds and the resulting bytes are a valid PNG (4-byte magic check).

### B. Sensible defaults

#### B1. Bump default timeout for daemon-drain commands and improve the error class [3/3]

On Comparis (~100 requests during initial load), `ff-rdp network` with default 5000 ms times out: `internal error: receiving drain response from daemon: operation timed out`. Two issues stacked.

- [x] Increase the default timeout for the daemon-drain path (`network` non-follow, `console` non-follow if it ever drains) to 15000 ms. Keep `--timeout` overridable. CLI default stays at 5000 ms; only the drain operation uses the higher floor.
- [x] Reclassify the timeout from `AppError::Internal` to a user-actionable error: `error: network drain timed out — try --timeout 15000`. Pairs with A1's `124` exit code.
- [x] Test: synthetic daemon that delays its drain response past 5 s; assert with default timeout the new error message + exit `124`, with `--timeout 30000` it succeeds.

#### B2. `geometry` and `responsive` skip hidden zero-sized matches by default [2/2]

Querying `geometry "h1" "header" "main"` on Comparis returns 18 zero-sized hidden `header` matches alongside the visible one. `--visible-only` exists but defaults to off. For an agent the default should be visible-only with `--include-hidden` to opt in.

- [x] Flip the default: `geometry` (and `responsive`, since it shares the geometry collection path) skips hidden / zero-rect elements unless `--include-hidden` is passed. Update `--help` accordingly.
- [x] Migration note: anyone scripting against the old default needs to add `--include-hidden`. Surface in the relevant `--help` and the README changelog if there is one.

#### B3. `eval --stringify` double-encodes strings [2/2]

`ff-rdp eval --stringify "document.title"` returns `"results": "\"Example Domain\""` — JSON-encoded twice (the outer is the envelope JSON, the inner is the `JSON.stringify` wrap that `--stringify` injects into the page-side script). For a value that's already a string the second wrap is gratuitous and confuses agents.

- [x] When the eval result type is already a string, skip the `JSON.stringify` wrap on the page side, or unwrap one level on the client side before serializing. The end result for `document.title` should be `"results": "Example Domain"` — single level of JSON encoding. Object/array results stay JSON-stringified (that's the whole point of `--stringify`).
- [x] Test: `eval --stringify "'foo'"` → `"results": "foo"`; `eval --stringify "({a:1})"` → `"results": "{\"a\":1}"` (still stringified, since it's an object); `eval --stringify "42"` → `"results": "42"`.

#### B4. Suppress `debug:` stderr lines outside `--verbose` / `RUST_LOG` [2/2]

`sources` (and any command with a fallback path) prints `debug: sources thread actor failed ... falling back to JS DOM/Performance API` to stderr alongside the JSON on stdout. Agents capturing `2>&1` see noise.

- [x] Audit `eprintln!("debug: ...")` call sites across `crates/ff-rdp-cli/src/commands/` and gate them behind a `verbose` predicate (`cli.verbose` or `RUST_LOG=debug`).
- [x] Default behaviour: silent fallback. Verbose: as today. Test asserting clean stderr on the happy path of `sources` against a real page.

### C. iter-53 follow-up

#### C1. Re-investigate `navigate --wait-text` first-call `noSuchActor` [0/2] — deferred

iter-53 task 1 was supposed to eliminate the `noSuchActor (unknownActor) — No such actor for ID: server1.connN.childM/consoleActorN` failure on the first `navigate --wait-text` after a fresh launch. Reproduced once during dogfooding session 41 against `https://news.ycombinator.com`. Retry succeeded.

Deferred: only reproduced once and intermittently; needs a deliberate live recording session to capture the failing RDP transcript. Tracked in [[backlog/future-features]].

- [ ] Reproduce with a live recording: fresh `launch --headless`, immediately `navigate URL --wait-text "..."` on a few different sites until it fires. Capture the full RDP transcript.
- [ ] Verify the iter-53 fix (defer console-actor resolution until after the navigation event) is hit on the failing path. If it is and it still fails, the failure is a different stale-actor case — likely the *target actor* is being invalidated and re-resolved while a request is in flight. Tighten the re-resolution scope or retry the wait once on `noSuchActor` before surfacing the error.

## Acceptance Criteria

- [x] `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -q` clean.
- [x] EXIT CODES section in `--help` exactly matches the codes the binary returns under each documented condition (verified by e2e tests in A1).
- [x] `ff-rdp network` against a heavy page (~100 requests) succeeds with default timeout.
- [x] `ff-rdp geometry SEL...` returns only visible elements by default.
- [x] `ff-rdp eval --stringify "document.title"` returns `"results": "Example Domain"` (single level of JSON encoding, not double-encoded).
- [x] No `debug:` lines on stderr from `ff-rdp sources` against a real page without `--verbose`.

## Design Notes

**Exit codes are a contract.** Once an agent's harness branches on `124` for "retry with longer timeout", silently emitting `1` makes the harness do the wrong thing. A1 is the highest-value task in this iteration even though it's mechanical work — every other failure-class refactor in the project gets easier once the mapping is explicit.

**Screenshot fallback** (A3) might justify being deferred again if the FF 150 actor failure has a specific upstream fix coming. Worth a quick check before starting: search [bugzilla.mozilla.org](https://bugzilla.mozilla.org) for `screenshot actor` regressions in 149/150.

**Default-changing tasks (B2 — `geometry --include-hidden`) deserve a release-note line** even though there's no formal changelog. Consider whether to ship B2 as a breaking change in a 0.3.0 bump or as an opt-in flag with a deprecation warning when an agent appears to want hidden elements.

## References

- [[dogfooding/dogfooding-session-41]] — source of all findings
- [[iterations/iteration-53-stability-fixes]] — the prior `noSuchActor` fix that didn't fully take
- [[iterations/iteration-55-daemon-hardening-docs]] — introduced the EXIT CODES doc whose codes don't fire
- [[backlog/future-features]] — overlapping LOW items for `--jq` exclusion docs, ANSI color, etc.
