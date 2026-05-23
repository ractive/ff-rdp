---
title: "Iteration 61o: Live-verify-by-default test architecture + mock watcher push events"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61o/live-verify-by-default
depends_on:
  - iteration-61m-wire-tracing-and-structured-errors
tags: [iteration, testing, mock-server, watcher, stability-roadmap]
---

# Iteration 61o: Live-verify-by-default test architecture

iter-61k passed 11 ACs' unit tests; live verification showed 7 of them broken. iter-61l added a mandate ("every AC requires a passing live test") but the cmux child still deferred 4 ACs to "next dogfooding". The pattern stops here by changing the substrate: live tests become the default path, the mock server gains the missing capability (watcher push-event streams) so unit tests stop lying, and a small harness makes the "launch FF + drive it + assert" loop a single helper call.

## Themes

- **A — Live-test helper crate.** Pulls together "launch headless FF on an ephemeral port", "wait for ready", "set up daemon", and "tear down everything cleanly on drop". Replaces the per-test boilerplate that currently nudges authors toward `#[ignore]`.
- **B — Mock-server watcher push events.** Today the mock returns canned replies but can't push `resources-available-array` events on its own. Until it can, any watcher-dependent code path has no unit-testable shape. Add an event-injection API.
- **C — `cargo test live_*` profile.** A workspace alias that includes live tests by default and surfaces them as a separate report. `cargo test --workspace -q` keeps short-suite fast; `cargo test --workspace --features live-tests` runs everything.
- **D — Test-author convention.** Every AC in a planned iteration cites a test name and the asserted post-condition in its checkbox text, e.g. `[x] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR`. Without that, the AC is not done.

## Tasks

### A. Live-test helper
- [ ] New crate `crates/ff-rdp-test-harness/` (or new module under `ff-rdp-cli/tests/common/`) exporting `Firefox::headless_on_random_port() -> Result<Firefox>` and `Firefox::with_daemon() -> Result<(Firefox, Daemon)>`.
- [ ] `Drop` impls cleanly kill FF + daemon and delete the temp profile.
- [ ] Retry-with-backoff on port allocation collisions (CI parallel runs).

### B. Mock-server push events
- [ ] In the mock server, expose `inject_event(actor: &str, event: serde_json::Value)` to fire arbitrary RDP notification packets at the connected client.
- [ ] Helper variant `inject_watcher_resource(resource_type, payload)` that wraps the packet in the standard `[[type, [resources]], ...]` envelope and routes to the right WatcherActor ID.
- [ ] Snapshot test covering `network-event`, `console-message`, `document-event` injection.

### C. Test profile
- [ ] `cargo` alias in `.cargo/config.toml`: `test-live = "test --workspace --features live-tests"`.
- [ ] CI workflow runs `cargo test-live` as a non-blocking job (with a known-good Firefox version pinned) so live failures are visible before merge.
- [ ] Document in `CONTRIBUTING.md` (or CLAUDE.md if appropriate).

### D. Convention
- [ ] Update `kb/iterations/.template.md` (or whatever the planning template is) to require AC checkboxes to name a test.
- [ ] Audit iter-61n's plan AC names — they already follow this convention; backfill iter-61m's.

## Acceptance Criteria [0/6]

- [ ] `Firefox::headless_on_random_port()` (or equivalent) returns a usable handle in ≤3 s; `Drop` kills FF cleanly.
- [ ] Mock-server `inject_watcher_resource` snapshot test passes for at least 3 resource types.
- [ ] `cargo test-live` alias works and a CI workflow surfaces live results.
- [ ] At least one previously-deferred AC (e.g. iter-61l N1 — `--detail --headers` keeps `meta.source = "watcher"`) gets converted to a passing live test in this iteration.
- [ ] iter-61j/61k/61l ACs that were green remain green; new live failures (if any) are filed as iter-61p/61q candidates.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q && cargo test-live` clean.

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
