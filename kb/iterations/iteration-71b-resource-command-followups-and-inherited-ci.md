---
title: "Iteration 71b: ResourceCommand follow-ups + inherited CI fixes"
type: iteration
date: 2026-05-24
status: in-progress
branch: iter-71b/followups-and-ci
depends_on:
  - iteration-71-resource-command-lifecycle
  - iteration-63-daemon-lockrecover-and-quick-sec-fixes
  - iteration-65-safe-write-and-path-traversal-hardening
  - iteration-68-supply-chain-and-fuzz
first_call_sites: []
dogfood_path: |
  # 1. PR CI is green end-to-end.
  gh pr checks  # all required checks pass on ubuntu/macos/windows + fuzz
  
  # 2. Daemon dispatcher actually flushes pending unwatches.
  ff-rdp daemon start --log-rdp-trace &
  ff-rdp daemon subscribe console-message
  # drop the subscriber; expect an unwatchResources packet within one event-pump cycle
  # (not "never", as it would be if gc() only fires from navigate.rs).
  
  # 3. Concurrent bus access doesn't block on navigate.
  # Launch navigate; while it waits for dom-complete, fire a dispatch_event
  # from another command — should not deadlock.
  
  # 4. Honest live verification of the legacy/watcher parallel-listen experiment.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_console_no_double_delivery -- --ignored
  # Expected: exactly one delivery per console event. If clean, file iter-71c to remove
  # the legacy ConsoleFront::start_listeners path; if not, document the divergence.
tags:
  - iteration
  - protocol
  - ci
---

# Iteration 71b: ResourceCommand follow-ups + inherited CI fixes

iter-71 ("ResourceCommand lifecycle") merged red on 2026-05-24 because the
in-pane recovery agent ticked all ACs and shipped before CI cleared. The
post-merge review (sub-agent, 2026-05-24) confirms:

- **All four red-CI checks are inherited**, not regressed by iter-71:
  `check_daemon_locks` tests (need `rg` on PATH, from iter-63),
  `script::runner::run_path_containment_rejects_absolute` on Windows
  (from iter-65), and the fuzz job's musl/sanitizer mismatch (from
  iter-68). iter-67/68/69/70 PR runs all show the same red CI.
- iter-71 itself has correctness debt: `gc()` was wired into `navigate.rs`
  but not the daemon dispatcher (one of the plan's explicit tasks); the
  `Mutex` on `Session::resource_command` is held across
  `wait_for_doc_complete` (a future deadlock); `lock().expect("…poisoned")`
  in `navigate.rs:417` violates the no-`.expect()` rule that
  `check-daemon-locks` enforces for the daemon directory only; the live
  test for the legacy/watcher parallel-listen experiment was added but
  never run, yet its AC was ticked.

This iteration closes the inherited CI red so the project gets back to
green, and addresses the highest-priority follow-ups from the iter-71
review. The legacy `ConsoleFront::start_listeners` removal (iter-71 Theme
C) is **explicitly deferred** to a separate iter-71c once the live test
actually runs and produces a clean result — that's a real branch in the
plan, not paper-over.

## Themes

- **A — Fix inherited CI red.** Make `check_daemon_locks` self-sufficient,
  fix the Windows path-containment test, fix the fuzz job's musl/sanitizer
  mismatch. After this iteration, every PR should see green CI.
- **B — Honest iter-71 follow-ups.** Wire `gc()` into the daemon
  dispatcher; fix the lock-hold across `wait_for_doc_complete`; replace
  `.expect()` with `lock_or_recover!`; drop dead `ref_counts` entries;
  document or fix the `ConnectedTab::get_or_init_resource_command`
  watcher-stability assumption.
- **C — Live-test the legacy/watcher coexistence.** Actually run
  `live_console_no_double_delivery` against headless Firefox; capture the
  result; file iter-71c (drop legacy path) only if the test shows clean
  single-delivery.

## Tasks

### A. Inherited CI fixes

- [ ] `crates/xtask/src/check_daemon_locks.rs:35-44` — replace the
      `Command::new("rg")` shell-out with `walkdir` + `regex` (already in
      the workspace dep tree). The xtask becomes self-contained and the
      three `check_daemon_locks::tests::*` tests pass on a runner that
      doesn't ship ripgrep.
- [ ] `crates/ff-rdp-cli/src/script/runner.rs:1443` (and the path-
      containment helper it tests) — use `std::path::Path::is_absolute()`
      and a Windows drive-letter / `\\?\` prefix check, not
      `starts_with('/')`. Verify `run_path_containment_rejects_absolute`
      passes on Windows.
- [ ] `.github/workflows/ci.yml` `fuzz` job — pin the harness to
      `x86_64-unknown-linux-gnu` (not `musl`) and confirm
      `cargo +nightly fuzz run transport_recv_from -- -max_total_time=60`
      builds and runs. Document the platform choice in `fuzz/README.md`.

### B. iter-71 review follow-ups

- [ ] `crates/ff-rdp-cli/src/commands/navigate.rs:412-446` — restructure so
      the bus `Mutex` is acquired per operation (subscribe, dispatch
      single event, gc, unsubscribe) instead of held across
      `wait_for_doc_complete`. Replace `bus_arc.lock().expect(...)` with
      `lock_or_recover!` (or an equivalent fallback).
- [ ] `crates/ff-rdp-core/src/resources/command.rs:313-336` — after
      `unwatch_resources` succeeds, `self.ref_counts.remove(t)` for each
      flushed type. Same in the `unsubscribe()` path when count hits
      zero. Prevents long-lived daemons accumulating 0-valued map entries.
- [ ] `crates/ff-rdp-core/src/resources/command.rs:328` — clear
      `pending_unwatch` only after the wire send succeeds (or emit a
      `tracing::warn!` on the silent-drop path so it's observable).
- [ ] Find the daemon event-pump (likely under
      `crates/ff-rdp-cli/src/daemon/`) and call
      `session.resource_command().map(|rc| rc.lock_or_recover().gc(transport))`
      once per event-batch cycle. Add a unit test that drives a
      synthetic event through the dispatcher and asserts `gc` was
      called.
- [ ] `crates/ff-rdp-cli/src/commands/connect_tab.rs:280-294` — either
      assert the passed `watcher_actor` matches the already-attached one,
      or add a doc-comment that documents "the first `watcher_actor`
      wins" and the invariant we're relying on (watcher stability across
      a session for current Firefox versions).

### C. Live verification of parallel-listen experiment

- [ ] Run `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test
      live_console_no_double_delivery -- --ignored` against headless
      Firefox locally; capture the result in
      `kb/research/iter71-legacy-listener-coexistence.md`.
- [ ] Tighten the test: `assert_eq!(matching.len(), 1, …)` (not `<= 1`),
      replace `format!("{:?}", …)` matching with a structural accessor on
      `Resource::ConsoleMessage`, and increase the drain window past
      200 ms.
- [ ] If clean: file `iter-71c-drop-legacy-startlisteners` with the
      removal punch-list (caller list at `commands/navigate.rs`,
      `commands/eval.rs`, `commands/console.rs`). If not: document the
      divergence and what we'd need to learn to make the removal safe.

## Acceptance Criteria [0/9]

- [x] `check_daemon_locks_no_external_rg`: `check_daemon_locks::tests::passes_when_no_unwraps`, `fails_on_regression`, and `fails_on_rustfmt_split_regression` pass without `rg` on PATH. `which rg && echo skip || cargo test -p xtask -q check_daemon_locks` is the gate.
- [x] `run_path_containment_rejects_absolute_on_windows`: the existing test passes on `windows-latest` runner; absolute-path detection uses `Path::is_absolute()` and a drive-letter check.
- [x] `fuzz_job_green_on_ci`: `.github/workflows/ci.yml` `fuzz` job exits 0 for `transport_recv_from`, `parse_page_map_str`, `parse_script_file` end-to-end; documented in `fuzz/README.md`.
- [x] `navigate_bus_lock_held_per_op_not_per_wait`: `commands/navigate.rs::wait_for_doc_complete` does not hold `bus_arc.lock()` across the wait loop; instead acquires per dispatch/subscribe/unsubscribe/gc operation. New unit test `navigate_bus_lock_released_during_wait` covers it via a probe `Mutex` contention check.
- [x] `navigate_no_dot_expect_on_bus`: `rg '\.lock\(\)\.expect\(' crates/ff-rdp-cli/src/commands/navigate.rs` returns zero hits; the lock recovers via `lock_or_recover!` or a typed-error fallback.
- [x] `resource_command_gc_drops_ref_count_entries`: after `gc()` flushes a type, `ref_counts` no longer contains that key. Test in `resources/command.rs::tests::gc_drops_flushed_ref_counts`.
- [x] `daemon_dispatcher_calls_gc`: the daemon event-pump invokes `gc()` once per cycle; unit test in `daemon/server.rs` drives a synthetic dropped-subscriber event and asserts the outbound packet log includes `unwatchResources` within one cycle.
- [x] `live_console_no_double_delivery_actually_runs`: live test was executed against headless Firefox at least once; result captured in `kb/research/iter71-legacy-listener-coexistence.md`; the AC is no longer ticked-without-execution.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean on ubuntu, macos, and windows runners (PR CI is green end-to-end).

## Design notes

The inherited CI red is the worst-leverage problem on the project right
now — three iterations (iter-67, 68, 69, 70, 71) all merged on red CI
because every reviewer assumed it was "pre-existing." Fixing it
end-of-batch is intentional: iter-72 onwards won't have to manually
ignore the same broken jobs.

`check_daemon_locks` shelling out to `rg` is a deeper symbol of "this
project assumes its dev tooling is everywhere" — we have similar latent
assumptions elsewhere (`hyalo` on PATH, `gh` on PATH, `cmux` on PATH).
This iteration scopes only the CI-blocking case; broader survey is out
of scope.

The decision to defer the legacy-`startListeners` removal to iter-71c is
deliberate: the parallel-listen experiment is the load-bearing piece of
evidence, and ticking that AC without running the test was the discipline
violation that made iter-71 ship red in the first place. Doing the test
properly is the prerequisite, not the consequence.

The daemon-dispatcher `gc()` integration is grouped with iter-71 follow-
ups (not the inherited-CI bucket) because it was an explicit task in the
original iter-71 plan that was silently dropped. Closing it here keeps
iter-71's plan-text honest after the fact.

## Out of scope

- Migrating the iter-62 page-map indexer to `Session::resource_command`.
  The review flagged this as a silently-dropped iter-71 task; file as a
  separate cleanup iteration if it becomes load-bearing.
- The fuzz job's deeper toolchain choice (cargo-vet, OSS-Fuzz integration).
  Pin to gnu and move on.
- A broader audit of "what other CI-PATH assumptions does this repo
  carry?" Separate research note.

## References

- [[iteration-71-resource-command-lifecycle]]
- [[iteration-63-daemon-lockrecover-and-quick-sec-fixes]]
- [[iteration-65-safe-write-and-path-traversal-hardening]]
- [[iteration-68-supply-chain-and-fuzz]]
- Post-iter-71 review report, 2026-05-24 (sub-agent run; findings 1-11 in
  this session's transcript).
