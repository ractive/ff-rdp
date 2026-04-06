---
title: "Iteration 16: Command Fixes & jq"
status: done
date: 2026-04-06
tags:
  - iteration
  - bugfix
---

# Iteration 16: Command Fixes & jq

From dogfooding session (2026-04-06): several commands crash or return wrong results.
Each is an independent fix. Also includes fixture re-recording to catch protocol drift.

## Bugs

- [x] **`perf vitals` returns pending Promise** — the JS expression evaluating web vitals
      returns an unresolved Promise grip (`"<state>": "pending"`) instead of awaiting the
      result. Needs `await` or a polling/callback approach since vitals (especially LCP, CLS)
      are observer-based and may not resolve immediately.

- [x] **`screenshot` fails on non-headless profile** — error: `drawWindow is not available
      in this Firefox build`. The `drawWindow` API requires privileged context. Investigate
      alternative screenshot methods (e.g., `browsingContext.captureScreenshot` from BiDi
      protocol, or the `ScreenshotActor`).

- [x] **`console` crashes with actor error** — `internal error: actor error... undefined
      passed where a value is required`. Likely a missing or null field in the console message
      payload that the deserializer doesn't handle.

- [x] **`sources` crashes with actor error** — same class of error as `console`:
      `undefined passed where a value is required`. Probably an optional field in the
      source descriptor that isn't handled.

- [x] **`--jq` filter broken on array results** — filters like `.[].url` fail with
      `cannot index`. The jq engine receives the results wrapped in the envelope
      (`{meta, results, total}`) but the filter is applied to the envelope, not to
      `.results`. Either auto-unwrap `.results` before applying the filter, or document
      that users must write `.results[].url`.

## Test & Fixture Validation

- [x] **Run full e2e test suite** — run `cargo test --workspace` and fix any failures
- [x] **Re-record all live fixtures** — run
      `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored`
      against a live Firefox instance and diff the results against the current fixtures
- [x] **Verify fixtures match code expectations** — review each fixture to confirm the
      JSON structure still matches what the deserialization code expects. Firefox protocol
      responses may have changed fields, added new optional keys, or altered shapes since
      the fixtures were last recorded. Update serde structs and tests accordingly.
- [x] **Check for new actor response fields** — look for unknown/ignored fields in fixture
      diffs that could be surfaced to users (e.g., new cookie attributes, new perf entry
      fields)

## Acceptance Criteria

- [x] All 5 bugs are fixed with regression tests
- [x] `--jq` works correctly on `perf --type resource` output
- [x] `perf vitals` returns numeric values, not Promise grips
- [x] `screenshot` works on non-headless profiles
- [x] `console` and `sources` handle optional/null fields gracefully
- [x] All e2e tests pass with freshly recorded fixtures
- [x] No fixture drift — recorded fixtures match current Firefox protocol responses
