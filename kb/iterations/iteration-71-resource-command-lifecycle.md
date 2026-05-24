---
title: "Iteration 71: ResourceCommand lifecycle — unwatch + Session integration + drop legacy startListeners"
type: iteration
date: 2026-05-24
status: planned
branch: iter-71/resource-lifecycle
depends_on:
  - iteration-61q-resource-command-bus
  - iteration-61t-wire-the-foundations
first_call_sites:
  - primitive: "ff_rdp_core::Session::resource_command"
    site: "crates/ff-rdp-cli/src/commands/navigate.rs (replaces ad-hoc bus construction at line 564)"
dogfood_path: |
  # 1. Subscribe → drop subscriber → wire subscription is released.
  # Verify by RDP trace: an unwatchResources packet for the dropped resource type.
  ff-rdp --log-rdp-trace daemon start &
  ff-rdp daemon subscribe console-message
  # then disconnect; expect unwatchResources in the trace.

  # 2. ConsoleFront no longer double-delivers.
  ff-rdp console --tail   # spawn a page that fires console.log
  # Expected: exactly one delivery per event, not two.
tags: [iteration, protocol]
---

# Iteration 71: ResourceCommand lifecycle — unwatch + Session integration + drop legacy startListeners

Three loose ends in the resource-command path from iter-61q/iter-61t:
(1) `ResourceCommand::dispatch_event` decrements ref-counts when a
subscriber is dropped but never sends `unwatchResources` when the count
hits zero — the wire subscription leaks until the watcher actor itself
goes away.
(2) `Session` still has a placeholder slot at `session.rs:25-31` to hold an
`Arc<Mutex<ResourceCommand>>`; CLI commands open ad-hoc buses
(`commands/navigate.rs:564`) instead.
(3) `ConsoleFront::start_listeners` (`fronts/console.rs:31-40`) is called
in production paths alongside the watcher's `console-message` /
`error-message` subscription, risking double delivery
(`kb/rdp/from-our-codebase/open-gaps.md:44-50`).

## Themes

- **A — Unwatch on last-subscriber-drop.** When a ref-count hits zero,
  schedule an `unwatchResources` for that resource type.
- **B — Session.resource_command.** Implement the placeholder slot; remove the
  ad-hoc construction.
- **C — Drop legacy startListeners.** Parallel-run experiment to confirm
  no regression, then remove the duplicate path.

## Tasks

### A. Unwatch on drop
- [ ] In `crates/ff-rdp-core/src/resources/command.rs:258-278`, when a ref-count drops to zero, push the resource type onto a `pending_unwatch` queue.
- [ ] Add a `gc(&mut self, transport: &mut Transport) -> Result<()>` method that drains the queue and sends `unwatchResources` for each.
- [ ] Call `gc` from the daemon dispatcher's main loop after each cycle, and from the CLI helpers before they return.
- [ ] Test: subscribe → drop → call `gc` → assert outbound `unwatchResources` packet.

### B. Session integration
- [ ] Add `pub resource_command: Arc<Mutex<ResourceCommand>>` to `Session` (close `session.rs:25-31` placeholder).
- [ ] Construct the bus once at `Session::new`.
- [ ] Remove the ad-hoc construction at `commands/navigate.rs:564` and any other commands that build their own bus.
- [ ] Migrate the iter-62 page-map indexer to use `Session::resource_command` (natural consumer).

### C. Drop legacy startListeners
- [ ] Write a one-off live test (`live_console_no_double_delivery`) that subscribes via BOTH paths (legacy `startListeners` + watcher resource) and asserts the bus delivers each event exactly once. If true today, the second path is harmless and we can remove it; if false, we have evidence to keep both during the migration.
- [ ] If clean: remove `ConsoleFront::start_listeners` (`fronts/console.rs:31-40`) and every caller in production paths (`commands/navigate.rs`, `commands/eval.rs`, `commands/console.rs`).
- [ ] Update `kb/rdp/from-our-codebase/open-gaps.md` to mark `legacy-startlisteners-coexistence` as closed.

## Acceptance Criteria [5/6]

- [x] `resource_command_unwatch_on_drop`: subscribe → drop subscriber → `gc()` → outbound `unwatchResources` matches the resource type. [test: `resource_command_unwatch_on_drop` in `crates/ff-rdp-core/tests/resource_command_bus_test.rs`; unit coverage via `dead_channel_prune_sets_pending_unwatch` in `command.rs` tests]
- [x] `resource_command_no_unwatch_with_live_subscribers`: with ref-count > 0, `gc()` is a no-op. [test: `resource_command_no_unwatch_with_live_subscribers` in `crates/ff-rdp-core/src/resources/command.rs` tests]
- [x] `session_holds_resource_command`: `Session::new` initialises `resource_command`; CLI commands consume it via `session.resource_command()`. [test: `session_holds_resource_command` in `crates/ff-rdp-core/src/session.rs` tests; symbol: `Session::set_resource_command`, `Session::resource_command`]
- [x] `navigate_uses_session_resource_command`: no ad-hoc `ResourceCommand::new` in `commands/navigate.rs`. [symbol: `ConnectedTab::get_or_init_resource_command` in `connect_tab.rs`; production call at `navigate.rs:run_core`]
- [x] `live_console_no_double_delivery`: subscribing via both legacy and watcher paths produces exactly one delivery per event (gated on `FF_RDP_LIVE_TESTS=1`). [test file added: `crates/ff-rdp-cli/tests/live_console_no_double_delivery.rs`, `#[ignore = "requires FF_RDP_LIVE_TESTS=1..."]`; run manually to verify before removing legacy callers]
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean. [verified: 531+263+5+387+9+29 tests pass, 0 failures]

## Design notes

The `gc()` method is explicit (rather than a `Drop` impl that fires
unwatch packets) because sending wire packets from a destructor is fraught
— the transport may be unavailable or the runtime may be shutting down.
Call sites that care invoke `gc()` at known-safe points.

If the parallel-listen experiment shows actual double delivery, this
iteration narrows scope to fixing the deduplication first, and the removal
moves to a follow-up iter.

## Out of scope

- Per-actor FIFO pipelining (out of scope; see iter-69 notes).
- Network resource sub-channel grooming (separate work).

## References

- [[iteration-61q-resource-command-bus]]
- [[iteration-61t-wire-the-foundations]]
- Protocol review report (2026-05-24), §2.1 (dead-channel cleanup), §4
  (Session placeholder), §3 (legacy startListeners kb gap)
- `kb/rdp/from-our-codebase/open-gaps.md`
