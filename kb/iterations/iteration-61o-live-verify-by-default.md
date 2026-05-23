---
title: "Iteration 61o: Live-verify-by-default test architecture + mock watcher push events"
type: iteration
date: 2026-05-23
status: done
branch: iter-61o/live-verify-by-default
depends_on:
  - iteration-61m-wire-tracing-and-structured-errors
tags:
  - iteration
  - testing
  - mock-server
  - watcher
  - stability-roadmap
---

# Iteration 61o: Live-verify-by-default test architecture

iter-61k passed 11 ACs' unit tests; live verification showed 7 of them broken. iter-61l added a mandate ("every AC requires a passing live test") but the cmux child still deferred 4 ACs to "next dogfooding". The pattern stops here by changing the substrate: live tests become the default path, the mock server gains the missing capability (watcher push-event streams) so unit tests stop lying, and a small harness makes the "launch FF + drive it + assert" loop a single helper call.

## Themes

- **A — Live-test helper crate.** Pulls together "launch headless FF on an ephemeral port", "wait for ready", "set up daemon", and "tear down everything cleanly on drop". Replaces the per-test boilerplate that currently nudges authors toward `#[ignore]`.
- **B — Mock-server watcher push events.** Today the mock returns canned replies but can't push `resources-available-array` events on its own. Until it can, any watcher-dependent code path has no unit-testable shape. Add an event-injection API.
- **C — `cargo test live_*` profile.** A workspace alias that includes live tests by default and surfaces them as a separate report. `cargo test --workspace -q` keeps short-suite fast; `cargo test --workspace --features live-tests` runs everything.
- **D — Test-author convention.** Every AC in a planned iteration cites a test name and the asserted post-condition in its checkbox text, e.g. `[x] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR`. Without that, the AC is not done.

## Tasks

### A. Live-test helper [2/3]
- [x] New module `crates/ff-rdp-cli/tests/common/mod.rs` exporting `LiveFirefox::headless_on_random_port()` and `LiveFirefox::with_daemon()`.
- [ ] `Drop` impls cleanly kill FF + daemon and delete the temp profile. *(Drop kills FF via `kill_pid`, but the temp profile created internally by `ff-rdp launch` is not tracked by the harness and is left for the OS to reap — deferred to a future iteration.)*
- [x] Retry-with-backoff on port allocation collisions (up to 3 attempts).

### B. Mock-server push events [3/3]
- [x] In the mock server, expose `inject_event(actor: &str, event: serde_json::Value)` to fire arbitrary RDP notification packets at the connected client.
- [x] Helper variant `inject_watcher_resource(resource_type, payload)` that wraps the packet in the standard `[[type, [resources]], ...]` envelope and routes to the right WatcherActor ID.
- [x] Snapshot test covering `network-event`, `console-message`, `document-event` injection (`mock_server_inject_test.rs`).

### C. Test profile [3/3]
- [x] `cargo` alias in `.cargo/config.toml`: `test-live = "test --workspace -- --include-ignored"`.
- [x] CI workflow runs `cargo test-live` as a non-blocking job (`.github/workflows/live.yml`, `continue-on-error: true`, pinned Firefox via setup-firefox v1.7.2).
- [x] Documented in `CLAUDE.md` (§ Live tests).

### D. Convention [2/2]
- [x] Updated `kb/iterations/.template.md` to require AC checkboxes to name a test.
- [x] Backfilled iter-61m's plan AC names to follow the convention.

## Acceptance Criteria [6/6]

- [x] `Firefox::headless_on_random_port()` (or equivalent) returns a usable handle in ≤3 s; `Drop` kills FF cleanly.
- [x] Mock-server `inject_watcher_resource` snapshot test passes for at least 3 resource types.
- [x] `cargo test-live` alias works and a CI workflow surfaces live results.
- [x] At least one previously-deferred AC (e.g. iter-61l N1 — `--detail --headers` keeps `meta.source = "watcher"`) gets converted to a passing live test in this iteration.
- [x] iter-61j/61k/61l ACs that were green remain green; new live failures (if any) are filed as iter-61p/61q candidates.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean (full `cargo test-live` gated by Firefox availability and runs in CI live-tests job).

## Design notes

- The mock-server push API is the single biggest hole — without it, every watcher-dependent unit test is misleading. Treat it as the most important task in this iteration.
- Don't over-engineer the harness — a `struct Firefox { port: u16, child: Child, profile: TempDir }` is fine. Resist the urge to model the entire DevToolsClient.
- Live tests must be flakiness-bounded: each test logs which port + PID it used, so post-mortems are tractable.
- **Concrete refactor candidates from iter-61n**: `tests/live_daemon_watch_targets.rs`, `tests/live_daemon_heavy_spa.rs`, and `tests/live_network_default_watcher.rs` each duplicate a `LiveFirefox` struct (launch via `ff-rdp launch --headless`, `free_port`, `wait_for_tcp`, cross-platform `kill_pid`). They are the prime first consumers of the new harness — migrating them after Theme A lands is a fast validator that the API is good enough.

## References

- [[ff-rdp-architecture-review]] §7 (Testing) — the mock-server gap
- [[firefox-devtools-patterns-for-ff-rdp]] §13 (Test architecture)
- [[dogfooding-session-53]] — the unit-pass/live-fail pattern that motivates this iteration
- [[stability-roadmap]]
