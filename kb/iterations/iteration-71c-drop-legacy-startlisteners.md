---
title: "iter-71c: Drop legacy WebConsoleActor::start_listeners calls"
date: 2026-05-24
status: planned
branch: iter-71c/drop-legacy-startlisteners
tags: [iteration, console, watcher, cleanup]
---

# Context

iter-71b Theme C ran `live_console_no_double_delivery` against a real headless Firefox
and confirmed:

- `evaluateJSAsync`-triggered `console.log` calls arrive **only** via the legacy
  `consoleActor` push path (`consoleAPICall` events from the console actor).
- The watcher `resources-available-array` stream delivers **zero** console-message resources
  for these eval-triggered calls.
- Therefore **no double-delivery** is possible via the `startListeners` + `watchResources`
  combination in the current codebase.

See `kb/research/iter71-legacy-listener-coexistence.md` for the full trace and analysis.

# Goal

Remove the three `WebConsoleActor::start_listeners` call sites from `console.rs`. This
simplifies the code, removes a deprecated Firefox API path, and eliminates the
`startListeners → consoleAPICall` push events from the transport entirely (only watcher
resources will arrive going forward).

# Tasks

- [ ] Remove the three `start_listeners` call sites in
      `crates/ff-rdp-cli/src/commands/console.rs` (lines 21, 180, 266).
- [ ] Verify that the `WebConsoleActor::start_listeners` function in
      `crates/ff-rdp-core/src/fronts/console.rs` is no longer called from any
      non-test code after removal.  If it becomes dead code, remove it and its
      spec helpers in `crates/ff-rdp-core/src/specs/console.rs`.
- [ ] Run `cargo run -p xtask -- check-dead-primitives --since origin/main` to
      confirm no new dead public items.
- [ ] Update or remove unit tests in `console.rs` that test the `start_listeners`
      path (those tests may be testing mock-server behavior, not Firefox itself).
- [ ] Run `FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test
      live_console_no_double_delivery -- --nocapture` to confirm the test still
      passes after removal (the assertion is `<= 1` which is safe if the legacy
      path disappears, since the watcher still delivers 0 in this scenario).

# Acceptance Criteria

- [ ] `no_start_listeners_in_console_rs`: `grep -n 'start_listeners' crates/ff-rdp-cli/src/commands/console.rs` returns zero hits.
- [ ] `start_listeners_dead_or_removed`: `start_listeners` is either removed from `ff-rdp-core` or only used in tests.
- [ ] `live_console_no_double_delivery_post_removal`: live test passes with `FF_RDP_LIVE_TESTS=1` after the removal.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` all green.

# Out of scope

- Removing the `WebConsoleActor` struct itself — it may still be needed for other console
  operations (e.g. `getPreferences`, `evaluateJSAsync`).
- Changes to how the watcher delivers console events — that is upstream Firefox protocol.
- Making the watcher path deliver `evaluateJSAsync`-triggered events (requires protocol research).
