---
title: "Iteration 9: Live Fixture Recording"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - testing
  - fixtures
  - live-tests
  - recording
status: in-progress
branch: iter-9/live-fixture-recording
---

# Iteration 9: Live Fixture Recording

Unify fixture recording with live testing so that recorded fixtures stay authentic and up to date.

## Background

An audit on 2026-04-06 found that ~27 of 45 CLI e2e test fixtures were hand-crafted rather than recorded from a real Firefox instance. They share copy-pasted timestamps, placeholder `input` fields, and fabricated result values. If Firefox changes its RDP protocol format, these fixtures won't catch the divergence.

Meanwhile, separate `record_*.rs` scripts exist in `ff-rdp-core/tests/` for recording fixtures, but they're disconnected from the tests that consume them — so fixtures drift silently.

### Design

Instead of maintaining separate recorder scripts, the live tests themselves become the fixture source. Two env vars control behavior:

- `FF_RDP_LIVE_TESTS=1` — run live tests against a real Firefox instance (existing, expand coverage)
- `FF_RDP_LIVE_TESTS_RECORD=1` — implies `FF_RDP_LIVE_TESTS=1`; additionally writes every RDP response to the corresponding fixture file on disk

This means:
1. Each live test exercises a real Firefox command and validates the result
2. When `RECORD=1`, the validated response is also persisted as the fixture file
3. CLI e2e tests (mock-based) continue to load these fixture files as before
4. Running `RECORD=1` once refreshes all fixtures from real Firefox — single command, no manual copy-paste

### Architecture

Live tests live in `ff-rdp-core/tests/` (direct transport access). They need to produce fixture files that the CLI e2e tests in `ff-rdp-cli/tests/fixtures/` consume. The recorder writes directly to both fixture directories.

A shared helper module provides:
- `should_run_live()` — checks `FF_RDP_LIVE_TESTS` or `FF_RDP_LIVE_TESTS_RECORD`
- `should_record()` — checks `FF_RDP_LIVE_TESTS_RECORD`
- `save_fixture(crate, name, value)` — writes pretty JSON to the correct `tests/fixtures/` directory
- `record_eval(transport, console_actor, js, fixture_name)` — sends evaluateJSAsync, captures immediate + result, optionally saves both, returns both

## Part A: Recording infrastructure

Build the shared helpers and convert existing fixtures.

### Tasks

- [ ] Create `ff-rdp-core/tests/support/recording.rs` with:
  - `should_run_live() -> bool` — true if `FF_RDP_LIVE_TESTS` or `FF_RDP_LIVE_TESTS_RECORD` is set
  - `should_record() -> bool` — true if `FF_RDP_LIVE_TESTS_RECORD` is set
  - `save_cli_fixture(name, value)` — writes to `ff-rdp-cli/tests/fixtures/{name}`
  - `save_core_fixture(name, value)` — writes to `ff-rdp-core/tests/fixtures/{name}`
  - `record_eval(transport, console_actor, js) -> (Value, Value)` — sends evaluateJSAsync, returns (immediate_ack, evaluation_result); if `should_record()`, saves both
  - Path resolution: use `CARGO_MANIFEST_DIR` for core fixtures, derive CLI fixture path relative to that
  - `normalize_fixture(value) -> Value` — normalizes only actor connection IDs (`conn\d+` → `conn0`) for cross-fixture consistency; leaves timestamps and other volatile fields as raw Firefox output
  - Add `json-matcher` as dev-dependency to `ff-rdp-cli` for flexible assertions on volatile output fields (timestamps, IDs)
- [ ] Export `recording` module from `ff-rdp-core/tests/support/mod.rs`
- [ ] Add `save_fixture` calls to existing `live_firefox_test.rs` tests (handshake, list_tabs_response) so `RECORD=1` refreshes those too

### Acceptance Criteria

1. `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-core -- --ignored` runs live tests (existing behavior preserved)
2. `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core -- --ignored` runs live tests AND writes fixture files to disk
3. Written fixture files are valid JSON, pretty-printed, and match the format the CLI e2e tests expect
4. `save_cli_fixture` writes to the correct `ff-rdp-cli/tests/fixtures/` path

## Part B: Live tests for protocol fixtures

Record the foundational RDP fixtures that multiple tests share.

### Tasks

- [ ] `live_handshake` — connect, capture greeting → `handshake.json`
- [ ] `live_list_tabs` — listTabs → `list_tabs_response.json`
- [ ] `live_get_target` — getTarget → `get_target_response.json`
- [ ] `live_get_watcher` — getWatcher → `get_watcher_response.json`
- [ ] `live_watch_resources` — watchResources → `watch_resources_response.json`
- [ ] `live_navigate` — navigateTo → `navigate_response.json`; also captures `reload_response.json`
- [ ] `live_eval_immediate` — evaluateJSAsync immediate ack → `eval_immediate_response.json`
- [ ] `live_start_listeners` — startListeners → `start_listeners_response.json`
- [ ] `live_get_cached_messages` — getCachedMessages → `get_cached_messages_response.json`
### Acceptance Criteria

1. All protocol-level fixtures are recorded from real Firefox
2. Fixture files have normalized actor IDs (`conn0`) for internal consistency; timestamps and other volatile fields are raw Firefox output
3. Each live test validates the response structure (has expected fields) before saving

## Part C: Live tests for eval-based commands

Record the evaluationResult fixtures for all CLI commands that use evaluateJSAsync.

### Tasks

- [ ] `live_eval_string` — `document.title` → `eval_result_string.json`
- [ ] `live_eval_number` — `1 + 41` → `eval_result_number.json`
- [ ] `live_eval_undefined` — `undefined` → `eval_result_undefined.json`
- [ ] `live_eval_object` — `({a: 1, b: [2,3]})` → `eval_result_object.json`
- [ ] `live_eval_exception` — `throw new Error('test error')` → `eval_result_exception.json`
- [ ] `live_eval_long_string` — `'x'.repeat(50000)` → `eval_result_long_string.json`
- [ ] `live_eval_null` — `null` → `eval_result_dom_null.json`, verify Firefox's null representation
- [ ] `live_page_text` — `document.body.innerText` on example.com → `eval_result_page_text.json`
- [ ] `live_wait_true` — `document.querySelector('h1') !== null` → `eval_result_wait_true.json`
- [ ] `live_wait_false` — `document.querySelector('.never-appears') !== null` → `eval_result_wait_false.json`
- [ ] `live_dom_text_single` — DOM text query on `h1` → `eval_result_dom_text.json`
- [ ] `live_dom_html_single` — DOM outerHTML on `h1` → `eval_result_dom_single.json`
- [ ] `live_dom_text_multi` — DOM text on `p` elements → `eval_result_dom_multi_text.json`
- [ ] `live_dom_attrs` — DOM attrs on `a` → `eval_result_dom_attrs.json`
- [ ] `live_click` — inject button + click → `eval_result_click.json`
- [ ] `live_click_missing` — click nonexistent element → `eval_result_element_not_found.json`
- [ ] `live_type_text` — inject input + type → `eval_result_type.json`
- [ ] `live_cookies` — set + read cookies → `eval_result_cookies.json`
- [ ] `live_cookies_empty` — empty cookie jar → `eval_result_cookies_empty.json`
- [ ] `live_storage_all` — set + read all localStorage → `eval_result_storage.json`
- [ ] `live_storage_key` — read single key → `eval_result_storage_key.json`
- [ ] `live_storage_null` — read nonexistent key → `eval_result_storage_null.json`
- [ ] `live_screenshot` — drawWindow screenshot → `eval_result_screenshot.json` (may be null in headless)
- [ ] `live_screenshot_null` — null result fixture → `eval_result_screenshot_null.json`
- [ ] `live_perf_timing` — Performance API → `eval_result_perf_timing.json`

### Acceptance Criteria

1. All eval-based fixtures are recorded from real Firefox
2. Each test sends the same JS expression the CLI command uses (import or duplicate the exact string)
3. Cookie/storage tests set up state before reading, matching the real usage pattern
4. Tests validate result structure (e.g., cookies test checks result parses as JSON array)

## Part D: Cleanup and edge-case fixtures

Handle longString/substring fixtures and remove the standalone recorder scripts.

### Tasks

- [ ] `live_long_string_substring` — fetch a longString, then call `substring` → `substring_screenshot_response.json`, `substring_page_text_response.json`
- [ ] `live_page_text_long` — long page text as longString → `eval_result_page_text_long.json`
- [ ] `live_screenshot_longstring` — screenshot as longString → `eval_result_screenshot_longstring.json`
- [ ] `live_cached_longstring` — Performance API longString → `eval_result_cached_longstring.json`
- [ ] `live_cached_exception` — Performance API exception → `eval_result_cached_exception.json`
- [ ] `live_network_resources` — navigate + watchResources → `resources_available_network.json`, `resources_updated_network.json`
- [ ] `live_network_details` — getRequestHeaders, getResponseHeaders, getResponseContent, getEventTimings → 4 fixture files
- [ ] Delete `record_fixtures.rs`, `record_iter4_fixtures.rs`, `record_net_details.rs`, `record_iter7_fixtures.rs`
- [ ] Update `kb/research/e2e-test-strategy.md` "How to Capture / Refresh Fixtures" section to reference the new `RECORD=1` workflow
- [ ] Verify all CLI e2e tests still pass with the newly recorded fixtures (`cargo test --workspace`)

### Acceptance Criteria

1. All ~45 CLI fixture files are produced by live tests when `FF_RDP_LIVE_TESTS_RECORD=1`
2. No standalone `record_*.rs` scripts remain
3. `cargo test --workspace` passes (mock-based tests consume the recorded fixtures)
4. `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core -- --ignored` is the single command to refresh all fixtures

## Design Notes

### Fixture normalization

Fixtures should stay as close to raw Firefox output as possible. Only one thing **must** be normalized: actor connection IDs (`conn\d+` → `conn0`), because they must be internally consistent across fixture files — the `consoleActor` in `get_target_response.json` must match the `from` field in `eval_result_string.json`, since `MockRdpServer` sends them verbatim and the CLI uses the IDs for subsequent requests.

Everything else (timestamps, window IDs, process IDs) is left as-is. This means re-recording produces diffs in volatile fields — but those diffs are harmless because the CLI e2e tests use flexible assertions for them (see below).

### Flexible assertions for volatile fields

Some volatile fields flow through to CLI stdout: `console` outputs `msg.timestamp`, `network` outputs `startTime`. CLI e2e tests must not assert exact values for these.

Use `json-matcher` crate (dev-dependency) for CLI e2e test assertions. It provides:
- `AnyMatcher::new()` — field exists, value doesn't matter
- `AnyMatcher::not_null()` — field exists and is not null
- Typed matchers (`U64Matcher`, etc.) and custom matchers via `JsonMatcher` trait
- Reports all mismatches, not just the first

Tests assert exact values for structural fields (result content, field names, nesting) and use `AnyMatcher` for volatile fields (timestamps, IDs). This way fixtures can change on re-recording without breaking tests.

### Fixture path resolution

Core tests know their own `CARGO_MANIFEST_DIR`. The CLI fixture path is derived as a sibling crate: `{core_manifest_dir}/../ff-rdp-cli/tests/fixtures/`. This avoids hardcoding absolute paths and works in any checkout location.

### Test ordering

Some fixtures require page state setup (cookies, storage, injected DOM elements). Group these tests so setup happens once per reconnect cycle, matching the pattern in the existing `record_iter7_fixtures.rs`.

### What about fixtures that need specific page content?

Tests that depend on example.com content (DOM queries returning "Example Domain", page-text returning specific strings) will produce fixtures matching example.com's actual content. If example.com changes, re-recording updates the fixtures and the CLI e2e tests automatically match. This is better than hand-crafted fixtures that might diverge from both real Firefox and real page content.
