---
title: "Iteration 116: console command primes the cache — start_listeners before get_cached_messages"
type: iteration
date: 2026-07-10
status: done
branch: iter-116/console-cache-start-listeners
depends_on:
  - kb/iterations/iteration-114-live-suite-debt-zero.md
firefox_refs: []
kb_refs:
  - kb/rdp/actors/console.md
first_call_sites:
  - primitive: WebConsoleActor::start_listeners
    site: >-
      crates/ff-rdp-cli/src/commands/console.rs (called before get_cached_messages so
      a fresh connection's cache is primed)
dogfood_path: |
  ff-rdp --port <p> eval 'console.log("hello %s, you are %d", "world", 42)'
  ff-rdp --port <p> console
  # expected: the printf-formatted message "hello world, you are 42" appears
tags:
  - iteration
  - console
  - rdp
---

# Iteration 116: console command primes the cache

Discovered during [[iteration-114-live-suite-debt-zero]] (Theme B port of
`live_console_printf_e2e`, deliberately left red there): `commands::console::run`
calls `WebConsoleActor::get_cached_messages` without ever calling
`startListeners` first. Per the Firefox WebConsole actor protocol
([[console]] kb note), `getCachedMessages` only returns messages recorded
since listeners were started — so a fresh `--no-daemon` connection's `console`
command legitimately sees nothing that an earlier eval logged. Verified live
in iter-114, including with a temporary product patch adding `start_listeners`
that fixed the exact failing sequence. No existing CLI path primes this cache
(`--follow` uses an unrelated Watcher resource subscription).

## Scope

- Add `WebConsoleActor::start_listeners` to ff-rdp-core (real spec method,
  no drift) and call it in `commands::console::run` before
  `get_cached_messages`.
- Update `kb/rdp/actors/console.md` (check-actor-kb-sync pairs the actor
  change with the kb note).
- Un-red `live_console_printf_e2e` — it is the live coverage for this change.

## Out of scope

- `console --follow` rework (its Watcher subscription path is separate).

## Acceptance Criteria [2/2]

- [x] live_console_printf_e2e: printf-formatted message
      `hello world, you are 42` returned by `ff-rdp console` on a fresh
      connection after an eval logged it. Un-redded in this iteration;
      `commands::console::run` now calls `prime_console_cache`
      (`WebConsoleActor::start_listeners(["PageError","ConsoleAPI"])`) before
      `get_cached_messages`.
- [x] live_console_no_double_delivery: still green (priming the cache must
      not double-deliver against the follow path). The non-follow `console`
      read is a short-lived single request that never subscribes to the
      watcher bus, so the combined-path assertion is unchanged.

## Results

- Product change (`crates/ff-rdp-cli/src/commands/console.rs`): added the
  private `prime_console_cache(cli, transport, console_actor)` helper and call
  it at the top of both `run` and `run_get_errors`, before the
  `get_cached_messages` calls. It issues
  `WebConsoleActor::start_listeners(["PageError", "ConsoleAPI"])` best-effort
  (failure only warns under `--verbose`; the read still proceeds).
  `WebConsoleActor::start_listeners` already existed in ff-rdp-core (real spec
  method, no drift) — this iteration wires its first non-test consumer, which
  is what `first_call_sites` promised.
- Live test (`crates/ff-rdp-cli/tests/live/live_console_printf.rs`): removed the
  "LEFT RED by design" module doc and rewrote it as an "iter-116 status: GREEN"
  section; the assertion message now reads as a regression guard.
- Docs: `kb/rdp/actors/console.md` gains an iter-116 note under
  `getCachedMessages`; the `console` subcommand `long_about` in
  `crates/ff-rdp-cli/src/cli/args.rs` notes the priming behaviour.
- Gates: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace -q` all green; the 16 mock-server e2e console
  tests continue to pass (the `console_server()` helper already registered a
  `startListeners` handler, now actually exercised by the non-follow path).
