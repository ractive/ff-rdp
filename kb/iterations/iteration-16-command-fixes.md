---
title: "Iteration 16: Command Fixes & jq"
status: planned
date: 2026-04-06
tags:
  - iteration
  - bugfix
---

# Iteration 16: Command Fixes & jq

From dogfooding session (2026-04-06): several commands crash or return wrong results.
Each is an independent fix. Also includes fixture re-recording to catch protocol drift.

## Bugs

- [ ] **`perf vitals` returns pending Promise** — the JS expression evaluating web vitals
      returns an unresolved Promise grip (`"<state>": "pending"`) instead of awaiting the
      result. Needs `await` or a polling/callback approach since vitals (especially LCP, CLS)
      are observer-based and may not resolve immediately.

- [ ] **`screenshot` fails on non-headless profile** — error: `drawWindow is not available
      in this Firefox build`. The `drawWindow` API requires privileged context. Investigate
      alternative screenshot methods (e.g., `browsingContext.captureScreenshot` from BiDi
      protocol, or the `ScreenshotActor`).

- [ ] **`console` crashes with actor error** — `internal error: actor error... undefined
      passed where a value is required`. Likely a missing or null field in the console message
      payload that the deserializer doesn't handle.

- [ ] **`sources` crashes with actor error** — same class of error as `console`:
      `undefined passed where a value is required`. Probably an optional field in the
      source descriptor that isn't handled.

- [ ] **`--jq` filter broken on array results** — filters like `.[].url` fail with
      `cannot index`. The jq engine receives the results wrapped in the envelope
      (`{meta, results, total}`) but the filter is applied to the envelope, not to
      `.results`. Either auto-unwrap `.results` before applying the filter, or document
      that users must write `.results[].url`.

## Test & Fixture Validation

- [ ] **Run full e2e test suite** — run `cargo test --workspace` and fix any failures
- [ ] **Re-record all live fixtures** — run
      `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored`
      against a live Firefox instance and diff the results against the current fixtures
- [ ] **Verify fixtures match code expectations** — review each fixture to confirm the
      JSON structure still matches what the deserialization code expects. Firefox protocol
      responses may have changed fields, added new optional keys, or altered shapes since
      the fixtures were last recorded. Update serde structs and tests accordingly.
- [ ] **Check for new actor response fields** — look for unknown/ignored fields in fixture
      diffs that could be surfaced to users (e.g., new cookie attributes, new perf entry
      fields)

## Acceptance Criteria

- [ ] All 5 bugs are fixed with regression tests
- [ ] `--jq` works correctly on `perf --type resource` output
- [ ] `perf vitals` returns numeric values, not Promise grips
- [ ] `screenshot` works on non-headless profiles
- [ ] `console` and `sources` handle optional/null fields gracefully
- [ ] All e2e tests pass with freshly recorded fixtures
- [ ] No fixture drift — recorded fixtures match current Firefox protocol responses
