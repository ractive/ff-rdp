---
branch: iter-134/meta-route-all-commands
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-128-network-hint-always-present.md
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://example.com --jq '.meta.route'
  # → "daemon" (not gated by --verbose)
  ff-rdp --no-daemon navigate https://example.com --jq '.meta.route'
  # → "direct"
  ff-rdp click 'a' --jq '.meta.route'
  ff-rdp eval '1+1' --jq '.meta.route'
  ff-rdp screenshot --jq '.meta.route'
  # → same "daemon"/"direct" self-identification on every browser-touching command
first_call_sites: []
status: done
---

# Iteration 134: meta.route on every command (carry-over from iter-128 Theme D)

iter-128 Theme D asked for `meta.route` ("daemon" | "direct") on **all** commands
(dogfood-62 finding #10: an agent has no way to tell how a command executed without
shelling out to `daemon status`). iter-128 scoped this down to the two commands central
to that iteration — `network` and `navigate --with-network` — via
`connection_meta::merge_route(meta, via_daemon)`, called explicitly at each command's
existing `merge_into_if_verbose` call site. See [[iteration-128-network-hint-always-present]]
for why a process-global "remembered route" (mirroring `remember_version`) was rejected:
it would leak across unit tests sharing the same `cargo test` binary/statics.

This plan is the deferred full rollout: call `connection_meta::merge_route` at the
remaining ~30 commands' meta-building call sites (`grep -rn merge_into_if_verbose
crates/ff-rdp-cli/src/commands/` for the full list), each of which already has
`ctx.via_daemon` in scope from `connect_and_get_target`/`connect_direct`.

## Tasks

- [x] Sweep every `crate::connection_meta::merge_into_if_verbose(&mut meta, ...)` call
      site and add an adjacent `crate::connection_meta::merge_route(&mut meta,
      ctx.via_daemon)` (or the equivalent local `via_daemon` binding).
- [x] For commands using `connect_direct` (always `via_daemon: false`), still call
      `merge_route` so `meta.route` is present and consistently `"direct"`.
- [x] Consider whether commands that build a bespoke envelope (bypassing
      `output::envelope`/`envelope_with_truncation`) need a call-site audit too.
- [x] Grep `crates/ff-rdp-cli/src/commands/*.rs` for envelope construction NOT preceded
      by `merge_route` after the sweep — should be zero for browser-touching commands.

## Acceptance Criteria [1/1]

- [x] live_134_meta_route_all_commands: for a representative sample of browser-touching
      commands (e.g. `click`, `eval`, `screenshot`, `dom`), `meta.route` is present and
      correct (`"daemon"` by default, `"direct"` under `--no-daemon`) without `--verbose`.

## Notes

Non-browser-touching commands (`daemon status`, `doctor`, `launch`, `throttle status`) are
out of scope — they don't resolve a `via_daemon` in the first place (`launch` spawns
Firefox but never opens a `ConnectedTab`; `throttle status`/`daemon status` only read
registry files).

`tabs` and `screenshot` always report `"direct"` regardless of daemon mode: `tabs` uses a
raw `RdpConnection::connect` that bypasses `ConnectedTab`/the daemon entirely, and
`screenshot` always calls `connect_direct` (the daemon's watcher subscription breaks its
two-step capture protocol — see `run_core`'s doc comment). `meta.route` reports the
connection Firefox actually saw, not the flag the caller passed, so both are hardcoded
`merge_route(&mut meta, false)` rather than threading a `ctx.via_daemon` that would always
be `false` anyway.

`navigate`'s plain `run()`/`run_core()`, `click::run_core`, `type_text::run_core`, and
`wait::run_core` didn't expose `via_daemon` to their `run()` callers (only the inner
`run_core` held the `ConnectedTab`) — their signatures changed to return
`(Value, bool)` (or `(Value, Option<bool>)` for `wait`, whose `--sleep-ms` short-circuit
never opens a connection at all). The script runner (`src/script/runner.rs`) and
`index.rs`'s crawl path call these same `run_core`s; both were updated to discard or
destructure the added `bool`.

`sources.rs` (connects via `connect_direct`) turned up during the final envelope-construction
audit (Task 4): it builds its own `meta` object but never called
`merge_into_if_verbose` in the first place, so it wasn't in the original
`merge_into_if_verbose` grep. Only `merge_route` was added here — the missing connection
block is a separate, pre-existing gap left out of scope for this iteration.
